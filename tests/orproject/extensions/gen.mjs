// Generate the extension fixture with a real OpenRefine 3.10.1; only local schema saving, no uploads.
import assert from "node:assert/strict"
import { mkdirSync, writeFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { startOpenRefine } from "../../../tools/openrefine.mjs"

const workspace = fileURLToPath(new URL("../../../target/extension-fixture-workspace/", import.meta.url))
const transform = (columnName, expression) => ({ op: "core/text-transform", engineConfig: { facets: [], mode: "row-based" }, columnName, expression: "grel:" + expression, onError: "keep-original", repeat: false, repeatCount: 10 })
const or = await startOpenRefine({ port: 3338, workspace, jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"] })
try {
  const project = await or.createProject("name,qty\n alice ,1\nbob,2\n", { name: "Wikibase preservation" })
  const first = await or.runOps(project, [transform("name", "value.trim()")])
  assert.equal(first.code, "ok", JSON.stringify(first))
  const schema = { entityEdits: [], siteIri: "http://www.wikidata.org/entity/", entityTypeSiteIRI: { item: "http://www.wikidata.org/entity/" }, mediaWikiApiEndpoint: "https://www.wikidata.org/w/api.php" }
  const { token } = await or.get("/command/core/get-csrf-token")
  const saved = await fetch(`${or.url}/command/wikidata/save-wikibase-schema?csrf_token=${token}`, { method: "POST", body: new URLSearchParams({ project, schema: JSON.stringify(schema) }) })
  assert.equal(saved.status, 200)
  const result = await saved.json()
  assert.ok(result.code === "ok" || result.historyEntry, JSON.stringify(result))
  await or.idle(project)
  const last = await or.runOps(project, [transform("qty", "value.toNumber()")])
  assert.equal(last.code, "ok", JSON.stringify(last))
  const models = await or.get(`/command/core/get-models?project=${project}`)
  assert.ok(models.overlayModels.wikibaseSchema)
  const history = await or.get(`/command/core/get-history?project=${project}`)
  const archive = await fetch(`${or.url}/command/core/export-project?project=${project}`, { method: "POST" })
  assert.equal(archive.status, 200)
  mkdirSync(new URL("./", import.meta.url), { recursive: true })
  writeFileSync(new URL("wikibase.openrefine.tar.gz", import.meta.url), Buffer.from(await archive.arrayBuffer()))
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", schema: models.overlayModels.wikibaseSchema, history: history.past.map((h) => h.description), table: await or.table(project) }, null, 2) + "\n")
  console.log(`Saved Wikibase schema and ${history.past.length} history steps from OpenRefine 3.10.1`)
} finally {
  or.stop()
}
