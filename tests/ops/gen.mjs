// Regenerate tests/ops/fixtures.json from a real OpenRefine 3.10.1:  node tests/ops/gen.mjs
// Each case gets a fresh project from CORPUS; the result is the table in display order:
// cells are null | {"v": value} | {"v": iso, "t": "date"} | {"e": message}.
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, CORPUS } from "./cases.mjs"

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-ops-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
try {
  const out = []
  for (const c of CASES) {
    const project = await or.createProject(CORPUS, { name: "ops" })
    const res = await or.command("apply-operations", { project, operations: c.ops })
    await or.idle(project)
    const models = await or.get(`/command/core/get-models?project=${project}`)
    const rows = (await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000`)).rows
    const cols = models.columnModel.columns
    const table = {
      columns: cols.map((x) => x.name),
      rows: rows.map((r) => ({ starred: r.starred, flagged: r.flagged, cells: cols.map((x) => r.cells[x.cellIndex] ?? null) })),
    }
    const entry = { name: c.name, ops: c.ops, ...table }
    if (res.code !== "ok") entry.error = res.message ?? JSON.stringify(res)
    out.push(entry)
    await or.command("delete-project", { project })
    console.log(`${c.name.padEnd(36)} ${res.code}  ${table.columns.length} cols x ${table.rows.length} rows`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", corpus: CORPUS, cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/ops/fixtures.json`)
} finally {
  or.stop()
}
