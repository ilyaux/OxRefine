// Regenerate tests/orproject/ from a real OpenRefine 3.10.1:  node tests/orproject/gen.mjs
// For each case: build the project, export it to archives/<n>.openrefine.tar.gz, then undo to every
// history step and record the table there. fixtures.json: per case the history (descriptions, past
// and future), the cursor, and `states[i]` = the table after i steps.
import { mkdirSync, rmSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, CORPUS } from "./cases.mjs"
import { LOOKUP } from "../ops/cases.mjs"
import { serve, withBase } from "../ops/server.mjs"

const dir = new URL("archives/", import.meta.url)
rmSync(dir, { recursive: true, force: true })
mkdirSync(dir)
// a fresh workspace, so there is exactly one project named "lookup" for cross()
rmSync(join(tmpdir(), "oxrefine-orproject-fixtures"), { recursive: true, force: true })
const server = await serve()
const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-orproject-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
try {
  await or.createProject(LOOKUP, { name: "lookup" })
  const out = []
  for (const [i, c] of CASES.entries()) {
    const project = await or.createProject(c.csv ?? CORPUS, { name: c.name, guessTypes: !!c.guessTypes })
    const res = await or.runOps(project, withBase(c.ops, server.base))
    if (res.code !== "ok") throw new Error(`${c.name}: ${res.message ?? JSON.stringify(res)}`)
    let h = await or.get(`/command/core/get-history?project=${project}`)
    const steps = h.past.map((e) => e.id)
    const goto = async (n) => (await or.command("undo-redo", { project, lastDoneID: n ? steps[n - 1] : 0 }), await or.idle(project))
    if (c.undo) await goto(steps.length - c.undo)
    h = await or.get(`/command/core/get-history?project=${project}`)
    const archive = `${String(i).padStart(2, "0")}.openrefine.tar.gz`
    const r = await fetch(`${or.url}/command/core/export-project?project=${project}`, { method: "POST" })
    writeFileSync(new URL(archive, dir), Buffer.from(await r.arrayBuffer()))
    const states = []
    for (let n = 0; n <= steps.length; n++) {
      await goto(n)
      states.push(await or.table(project))
    }
    out.push({ name: c.name, archive, history: [...h.past, ...h.future].map((e) => e.description), cursor: h.past.length, states })
    await or.command("delete-project", { project })
    console.log(`${c.name.padEnd(36)} ${steps.length} steps, at ${h.past.length}`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/orproject/fixtures.json`)
} finally {
  or.stop()
  server.close()
}
