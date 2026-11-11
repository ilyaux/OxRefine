// Open OxRefine's exported archives in a real OpenRefine 3.10.1 and check every history step:
//   cargo test -p ox-core --test orproject_conformance   (writes target/orproject-export/)
//   node tests/orproject/check-export.mjs
// Each archive must import, show OxRefine's history, and give the fixture's table at every step.
import { readFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"

const fx = JSON.parse(readFileSync(new URL("fixtures.json", import.meta.url)))
const dir = new URL("../../target/orproject-export/", import.meta.url)
const norm = (c) => (c && typeof c === "object" && !Object.keys(c).length ? null : c?.e !== undefined ? { e: true } : c)
const shape = (t) => JSON.stringify({ columns: t.columns, rows: t.rows.map((r) => ({ ...r, cells: r.cells.map(norm) })) })
const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-orproject-check"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"],
})
let bad = 0
try {
  for (const c of fx.cases) {
    const file = new File([readFileSync(new URL(c.archive, dir))], c.archive)
    const r = await or.command("import-project", { "project-file": file, "project-name": c.name }, { json: false })
    const project = new URL(r.headers.get("location") ?? "", or.url).searchParams.get("project")
    const fail = (why) => (bad++, console.log(`FAIL ${c.name}: ${why}`))
    if (!project) {
      fail(`not imported: ${r.status} ${(await r.text()).slice(0, 300)}`)
      continue
    }
    const h = await or.get(`/command/core/get-history?project=${project}`)
    const steps = [...h.past, ...h.future]
    const merged = c.history.length - steps.length
    const ok = JSON.stringify(steps.map((e) => e.description)) === JSON.stringify(c.history.slice(merged)) && h.past.length === c.cursor - merged
    if (!ok) fail(`history ${JSON.stringify(steps.map((e) => e.description))} at ${h.past.length}`)
    // OpenRefine reads each operation back (Extract lists all but single-cell and single-row edits)
    const single = /^(Edit single cell|Star row|Unstar row|Flag row|Unflag row) /
    const ops = (await or.get(`/command/core/get-operations?project=${project}`)).entries.filter((e) => e.operation).length
    if (ops !== h.past.filter((e) => !single.test(e.description)).length) fail(`${ops} extracted operations`)
    for (let n = 0; n <= steps.length; n++) {
      await or.command("undo-redo", { project, lastDoneID: n ? steps[n - 1].id : 0 })
      await or.idle(project)
      if (shape(await or.table(project, { recon: "brief" })) !== shape(c.states[merged + n])) {
        fail(`table after ${merged + n} steps differs`)
        break
      }
    }
    await or.command("delete-project", { project })
  }
  console.log(`${fx.cases.length - bad}/${fx.cases.length} exported archives open in OpenRefine with every step intact`)
} finally {
  or.stop()
}
process.exit(bad ? 1 : 0)
