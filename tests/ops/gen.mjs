// Regenerate tests/ops/fixtures.json from a real OpenRefine 3.10.1:  node tests/ops/gen.mjs
// Each case gets a fresh project from CORPUS; the result is the table in display order:
// cells are null | {"v": value} | {"v": iso, "t": "date"} | {"e": message}.
import { rmSync, writeFileSync } from "node:fs"
import { serve, withBase } from "./server.mjs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, CORPUS, LOOKUP } from "./cases.mjs"

const server = await serve()
const BASE = server.base

// a fresh workspace, so there is exactly one project named "lookup"
rmSync(join(tmpdir(), "oxrefine-ops-fixtures"), { recursive: true, force: true })
const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-ops-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
try {
  await or.createProject(LOOKUP, { name: "lookup" })
  const out = []
  for (const c of CASES) {
    const project = await or.createProject(CORPUS, { name: "ops" })
    const res = await or.runOps(project, withBase(c.ops, BASE))
    const history = (await or.get(`/command/core/get-history?project=${project}`)).past.map((h) => h.description)
    const operations = (await or.get(`/command/core/get-operations?project=${project}`)).entries.filter((e) => e.operation).length
    const table = await or.table(project)
    const entry = { name: c.name, ops: c.ops, ...table, history, operations }
    if (res.code !== "ok") entry.error = res.message ?? JSON.stringify(res)
    out.push(entry)
    await or.command("delete-project", { project })
    console.log(`${c.name.padEnd(36)} ${res.code}  ${table.columns.length} cols x ${table.rows.length} rows`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", corpus: CORPUS, lookup: LOOKUP, cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/ops/fixtures.json`)
} finally {
  or.stop()
  server.close()
}
