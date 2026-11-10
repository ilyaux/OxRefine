// Regenerate tests/import/fixtures.json from a real OpenRefine 3.10.1:  node tests/import/gen.mjs
// Cells are null | {"v": value} | {"v": iso, "t": "date"} | {"e": message}, in display column order.
import { readFileSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES } from "./cases.mjs"

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-import-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
try {
  const out = []
  for (const c of CASES) {
    const res = await or.command("create-project-from-upload", {
      "project-file": new File([readFileSync(new URL(c.file, import.meta.url))], c.file),
      "project-name": "import",
      format: c.format,
      options: c.options,
    }, { json: false })
    const project = new URL(res.headers.get("location") ?? "", or.url).searchParams.get("project")
    if (!project) throw new Error(`${c.name}: import failed ${res.status} ${(await res.text()).slice(0, 300)}`)
    const cols = (await or.get(`/command/core/get-models?project=${project}`)).columnModel.columns
    const rows = (await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000`)).rows
    out.push({ ...c, columns: cols.map((x) => x.name), rows: rows.map((r) => cols.map((x) => r.cells[x.cellIndex] ?? null)) })
    await or.command("delete-project", { project })
    console.log(`${c.name.padEnd(36)} ${cols.length} cols x ${rows.length} rows`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/import/fixtures.json`)
} finally {
  or.stop()
}
