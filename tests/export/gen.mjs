// Regenerate tests/export/fixtures.json from a real OpenRefine 3.10.1:  node tests/export/gen.mjs
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, CORPUS, SETUP } from "./cases.mjs"

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-export-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
try {
  const project = await or.createProject(CORPUS, { name: "people" })
  const res = await or.command("apply-operations", { project, operations: SETUP })
  if (res.code !== "ok") throw new Error(JSON.stringify(res))
  await or.idle(project)
  const out = []
  for (const c of CASES) {
    const engine = c.engine ?? { facets: [], mode: "row-based" }
    const r = await or.command("export-rows", { project, format: c.format, engine, ...(c.params ?? {}) }, { json: false })
    const bytes = Buffer.from(await r.arrayBuffer())
    out.push({ ...c, engine, output: c.binary ? bytes.toString("base64") : bytes.toString("utf8") })
    console.log(`${c.name.padEnd(42)} ${bytes.length} bytes`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", project: "people", corpus: CORPUS, setup: SETUP, cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/export/fixtures.json`)
} finally {
  or.stop()
}
