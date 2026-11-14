// After cargo test: verify preserved schemas, tables and every history step in OpenRefine itself.
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { startOpenRefine } from "../../../tools/openrefine.mjs"

const fx = JSON.parse(readFileSync(new URL("fixtures.json", import.meta.url)))
const or = await startOpenRefine({ port: 3338, workspace: fileURLToPath(new URL("../../../target/extension-check-workspace/", import.meta.url)), jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"] })
async function inspect(path) {
  const result = await or.command("import-project", { "project-file": new File([readFileSync(path)], "project.openrefine.tar.gz"), "project-name": "preservation check" }, { json: false })
  const project = new URL(result.headers.get("location") ?? "", or.url).searchParams.get("project")
  assert.ok(project, `archive was not imported: ${result.status}`)
  const h = await or.get(`/command/core/get-history?project=${project}`)
  const steps = [...h.past, ...h.future]
  const states = []
  for (let i = 0; i <= steps.length; i++) {
    await or.command("undo-redo", { project, lastDoneID: i ? steps[i - 1].id : 0 })
    await or.idle(project)
    const models = await or.get(`/command/core/get-models?project=${project}`)
    states.push({ table: await or.table(project), overlays: models.overlayModels })
  }
  await or.command("delete-project", { project })
  return { history: steps.map((s) => s.description), cursor: h.past.length, states }
}
try {
  const original = await inspect(new URL("wikibase.openrefine.tar.gz", import.meta.url))
  assert.deepEqual(original.history, fx.history)
  assert.deepEqual(original.states.at(-1).overlays.wikibaseSchema, fx.schema)
  for (const name of ["wikibase", "reopened"]) {
    assert.deepEqual(await inspect(new URL(`../../../target/extension-export/${name}.openrefine.tar.gz`, import.meta.url)), original)
    console.log(`${name}: Wikibase schema and all ${original.history.length} history steps preserved in OpenRefine 3.10.1`)
  }
} finally {
  or.stop()
}
