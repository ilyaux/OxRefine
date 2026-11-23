// Regenerate tests/rdf/fixtures.json from a real OpenRefine 3.10.1:  node tests/rdf/gen.mjs
// Cells are as get-rows shows them. OpenRefine names blank nodes with random ids, so they are
// recorded as they came and compared by structure, not by id. Relative IRIs resolve against
// OpenRefine's working folder; that folder is recorded as <base>.
import { readFileSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { pathToFileURL } from "node:url"
import { OR_HOME, startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, WIZARD } from "./cases.mjs"

const or = await startOpenRefine({
  port: 3339,
  workspace: join(tmpdir(), "oxrefine-rdf-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
const base = pathToFileURL(OR_HOME).href
const input = (file) => readFileSync(new URL(`inputs/${file}`, import.meta.url))
try {
  const cases = []
  for (const c of CASES) {
    const res = await or.command("create-project-from-upload", { "project-file": new File([input(c.file)], c.file), "project-name": "rdf", format: c.format, options: {} }, { json: false })
    const project = new URL(res.headers.get("location") ?? "", or.url).searchParams.get("project")
    if (!project) {
      cases.push({ ...c, error: { status: res.status } })
      console.log(c.name.padEnd(30), "no project", res.status)
      continue
    }
    const models = await or.get(`/command/core/get-models?project=${project}`)
    if (!models.columnModel) {
      cases.push({ ...c, error: { models: { ...models, message: String(models.message ?? "").replaceAll(project, "<project>") } } })
      console.log(c.name.padEnd(30), "error", JSON.stringify(models).slice(0, 200))
      continue
    }
    const table = await or.table(project)
    cases.push({ ...c, columns: table.columns, rows: JSON.parse(JSON.stringify(table.rows.map((r) => r.cells)).replaceAll(base, "<base>")), keyCellIndex: models.columnModel.keyCellIndex, hasRecords: models.recordModel?.hasRecords ?? false })
    await or.command("delete-project", { project })
    console.log(c.name.padEnd(30), `${table.columns.length} cols x ${table.rows.length} rows`)
  }
  const wizard = []
  for (const file of WIZARD) {
    const w = await or.wizard({ upload: new File([input(file)], file) })
    wizard.push({ file, format: w.format, options: w.options, columns: w.columns.length, rows: w.rows.length })
    console.log("wizard", file, w.format, JSON.stringify(w.options))
  }
  const version = await or.get("/command/core/get-version")
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: version.version, generator: "tests/rdf/gen.mjs", cases, wizard }, null, 1) + "\n")
  console.log(`${cases.length} cases -> tests/rdf/fixtures.json`)
} finally {
  or.stop()
}
