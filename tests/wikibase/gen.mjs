// Regenerate tests/wikibase/fixtures.json from a real OpenRefine 3.10.1:  node tests/wikibase/gen.mjs
// For each schema in cases.mjs: the schema OpenRefine saves, the history operation, the
// QuickStatements and schema exports, the schema preview (edits and warnings) and wikibaseIssues()
// on every row. The schema's MediaWiki API is a local mock; its requests are recorded in `api`.
import { readFileSync, writeFileSync } from "node:fs"
import { createServer } from "node:http"
import { join } from "node:path"
import { tmpdir } from "node:os"
import vm from "node:vm"
import { OR_HOME, startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, CSV, MATCHES, NEW, SITE } from "./cases.mjs"

// every property a case uses, with its datatype, as Wikidata describes them
const DATATYPES = {}
JSON.stringify(CASES, (key, value) => {
  if (value?.type === "wbpropconstant") DATATYPES[value.pid] = value.datatype
  return value
})
const requests = []
const api = createServer((req, res) => {
  const chunks = []
  req.on("data", (c) => chunks.push(c))
  req.on("end", () => {
    const params = new URLSearchParams(req.url.split("?")[1] ?? "")
    for (const [k, v] of new URLSearchParams(Buffer.concat(chunks).toString())) params.set(k, v)
    const q = Object.fromEntries(params)
    requests.push(q)
    const entities = {}
    for (const id of (q.ids ?? "").split("|").filter(Boolean)) {
      const base = { id, labels: {}, descriptions: {}, aliases: {}, claims: {}, lastrevid: 1, modified: "2024-01-01T00:00:00Z", pageid: 1, ns: id.startsWith("P") ? 120 : 0, title: id.startsWith("P") ? `Property:${id}` : id }
      entities[id] = id.startsWith("P") ? { ...base, type: "property", datatype: DATATYPES[id] ?? "string" } : { ...base, type: "item", sitelinks: {} }
    }
    const languages = Object.fromEntries(["en", "de", "fr", "mul"].map((code) => [code, { code }]))
    const body = q.action === "wbgetentities" ? { entities, success: 1 } : q.meta === "wbcontentlanguages" ? { batchcomplete: "", query: { wbcontentlanguages: languages } } : {}
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(body))
  })
}).listen(0, "127.0.0.1")
await new Promise((r) => api.once("listening", r))
const API = `http://127.0.0.1:${api.address().port}/w/api.php`

// the Wikidata manifest the schema editor sends, pointed at the mock API
const manifestFile = join(OR_HOME, "webapp/extensions/wikibase/module/scripts/wikidata-manifest-v1.0.js")
const context = {}
vm.runInNewContext(readFileSync(manifestFile, "utf8") + "\nthis.manifest = WikidataManifestV1_0;", context)
const manifest = { ...context.manifest, mediawiki: { ...context.manifest.mediawiki, api: API } }

const or = await startOpenRefine({
  port: 3340,
  workspace: join(tmpdir(), "oxrefine-wikibase-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
const all = { facets: [], mode: "row-based" }
const only = (columnName, value) => ({ mode: "row-based", facets: [{ type: "list", name: columnName, columnName, expression: "value", omitBlank: false, omitError: false, selection: [{ v: { v: value, l: value } }], selectBlank: false, selectError: false, invert: false }] })
const spaces = { identifierSpace: SITE, schemaSpace: "http://www.wikidata.org/prop/direct/" }
const withApi = (value) => JSON.parse(JSON.stringify(value).replaceAll("{API}", API))
const csrf = async () => (await or.get("/command/core/get-csrf-token")).token
async function wikidata(command, query, fields) {
  const r = await fetch(`${or.url}/command/wikidata/${command}?${new URLSearchParams({ ...query, csrf_token: await csrf() })}`, { method: "POST", body: new URLSearchParams(fields) })
  const text = await r.text()
  try { return JSON.parse(text) } catch { return { status: r.status, text } }
}

try {
  const out = []
  for (const c of CASES) {
    const project = await or.createProject(CSV, { name: "wikibase" })
    const ops = [
      ...MATCHES.map(([columnName, id, name]) => ({ op: "core/recon-match-specific-topic-to-cells", engineConfig: only(columnName, id), columnName, match: { id, name, types: [] }, ...spaces })),
      ...NEW.map(([columnName, value, shareNewTopics]) => ({ op: "core/recon-mark-new-topics", engineConfig: only(columnName, value), columnName, shareNewTopics, service: "https://wikidata.reconci.link/en/api", ...spaces })),
    ]
    const ran = await or.runOps(project, ops)
    if (ran.code !== "ok") throw new Error(`${c.name}: recon ${JSON.stringify(ran)}`)
    const schema = withApi(c.schema)
    const saved = await wikidata("save-wikibase-schema", {}, { project, schema: JSON.stringify(schema) })
    await or.idle(project)
    const models = await or.get(`/command/core/get-models?project=${project}`)
    const operations = (await or.get(`/command/core/get-operations?project=${project}`)).entries
    const exported = {}
    for (const format of ["quickstatements", "wikibase-schema"]) {
      try {
        const r = await or.command("export-rows", { project, format, engine: all }, { json: false })
        exported[format] = await r.text()
      } catch (e) {
        // OpenRefine fails on some schemas (NullPointerException); recorded as the failure
        exported[format] = { error: e.message.match(/HTTP (\d+)/)?.[1] ?? e.message, exception: e.message.match(/(java\.[\w.]+Exception)/)?.[1] ?? null }
      }
    }
    const preview = await wikidata("preview-wikibase-schema", { project }, { schema: JSON.stringify(schema), manifest: JSON.stringify(manifest), engine: JSON.stringify(all) })
    const issues = await or.command("preview-expression", { project, cellIndex: "0", rowIndices: JSON.stringify([0, 1, 2, 3, 4, 5]), expression: "grel:wikibaseIssues()", repeat: "false", repeatCount: "10" })
    const strip = (v) => JSON.parse(JSON.stringify(v).replaceAll(API, "{API}"))
    out.push(strip({ name: c.name, schema: c.schema, saved: saved.code ?? saved, overlay: models.overlayModels?.wikibaseSchema ?? null, operation: operations.at(-1), exports: exported, preview, issues: issues.results ?? issues, table: (await or.table(project, { recon: "brief" })).rows.map((r) => r.cells) }))
    await or.command("delete-project", { project })
    const qs = exported.quickstatements
    console.log(c.name.padEnd(30), (typeof qs === "string" ? qs.split("\n").filter(Boolean).length + " QS lines" : `QS ${JSON.stringify(qs)}`).padEnd(14), `preview ${preview.edit_count ?? JSON.stringify(preview).slice(0, 120)}`)
  }
  const version = await or.get("/command/core/get-version")
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: version.version, generator: "tests/wikibase/gen.mjs", csv: CSV, setup: withApi([...MATCHES.map(([columnName, id, name]) => ({ op: "core/recon-match-specific-topic-to-cells", engineConfig: only(columnName, id), columnName, match: { id, name, types: [] }, ...spaces })), ...NEW.map(([columnName, value, shareNewTopics]) => ({ op: "core/recon-mark-new-topics", engineConfig: only(columnName, value), columnName, shareNewTopics, service: "https://wikidata.reconci.link/en/api", ...spaces }))]), languages: ["en", "de", "fr", "mul"], cases: out, api: [...new Set(requests.map((q) => JSON.stringify(q)))].map((q) => JSON.parse(q)) }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/wikibase/fixtures.json; ${requests.length} MediaWiki API requests`)
} finally {
  or.stop()
  api.close()
}
