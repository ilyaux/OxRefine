// Regenerate tests/wikibase/constraints.json:  node tests/wikibase/constraints-gen.mjs
// OpenRefine 3.10.1's constraint checks: a mock MediaWiki API serves made-up properties (P9001...)
// that each carry one Wikidata property constraint (P2302), and one schema breaks every one of
// them. Recorded: the schema preview's warnings and wikibaseIssues() on every row.
import { readFileSync, writeFileSync } from "node:fs"
import { createServer } from "node:http"
import { join } from "node:path"
import { tmpdir } from "node:os"
import vm from "node:vm"
import { OR_HOME, startOpenRefine } from "../../tools/openrefine.mjs"
import { CSV, MATCHES, NEW, SITE } from "./cases.mjs"

const item = (id) => ({ value: { "entity-type": id.startsWith("P") ? "property" : "item", "numeric-id": Number(id.slice(1)), id }, type: "wikibase-entityid" })
const snak = (property, datavalue, datatype) => ({ snaktype: "value", property, datavalue, datatype })
/** A property constraint statement: the constraint item and its parameters (qualifiers). */
const constraint = (qid, parameters = {}) => ({
  mainsnak: snak("P2302", item(qid), "wikibase-item"), type: "statement", rank: "normal", id: `P$${qid}`,
  qualifiers: Object.fromEntries(Object.entries(parameters).map(([pid, values]) => [pid, values.map((v) => typeof v === "string" && /^[QP]\d+$/.test(v) ? snak(pid, item(v), "wikibase-item") : snak(pid, { value: v, type: "string" }, "string"))])),
  "qualifiers-order": Object.keys(parameters),
})
// each made-up property: its datatype and constraints (the Wikidata manifest's constraint ids)
export const PROPERTIES = {
  P9001: ["string", [constraint("Q21502404", { P1793: ["[A-Z][a-z]+ [A-Z][a-z]+"] })]],
  P9002: ["string", [constraint("Q19474404")]],
  P9003: ["string", [constraint("Q21502410")]],
  P9004: ["wikibase-item", [constraint("Q21510859", { P2305: ["Q1", "Q2"] })]],
  P9005: ["wikibase-item", [constraint("Q52558054", { P2305: ["Q145"] })]],
  P9006: ["wikibase-item", [constraint("Q21502838", { P2306: ["P9004"] })]],
  P9007: ["wikibase-item", [constraint("Q21510851", { P2306: ["P580"] }), constraint("Q21510856", { P2306: ["P585"] })]],
  P9008: ["string", [constraint("Q53869507", { P5314: ["Q54828449"] })]],
  P9009: ["quantity", [constraint("Q52848401"), constraint("Q51723761"), constraint("Q21514353", { P2305: ["Q11573"] })]],
  P9010: ["wikibase-item", [constraint("Q52004125", { P2305: ["Q29934218"] })]],
  P9011: ["string", [constraint("Q21510857")]],
  P9012: ["wikibase-item", [constraint("Q21510855", { P2306: ["P9012"] })]],
  P580: ["time", []],
  P585: ["time", []],
}

const requests = []
const api = createServer((req, res) => {
  const chunks = []
  req.on("data", (c) => chunks.push(c))
  req.on("end", () => {
    const q = Object.fromEntries(new URLSearchParams((req.url.split("?")[1] ?? "") + "&" + Buffer.concat(chunks).toString()))
    requests.push(q)
    const entities = {}
    for (const id of (q.ids ?? "").split("|").filter(Boolean)) {
      const base = { id, labels: {}, descriptions: {}, aliases: {}, lastrevid: 1, modified: "2024-01-01T00:00:00Z", pageid: 1, ns: id.startsWith("P") ? 120 : 0, title: id.startsWith("P") ? `Property:${id}` : id }
      const [datatype, constraints] = PROPERTIES[id] ?? ["wikibase-item", []]
      entities[id] = id.startsWith("P") ? { ...base, type: "property", datatype, claims: constraints.length ? { P2302: constraints } : {} } : { ...base, type: "item", claims: {}, sitelinks: {} }
    }
    const languages = Object.fromEntries(["en", "de", "fr", "mul"].map((code) => [code, { code }]))
    const body = q.action === "wbgetentities" ? { entities, success: 1 } : q.meta === "wbcontentlanguages" ? { batchcomplete: "", query: { wbcontentlanguages: languages } } : {}
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(body))
  })
}).listen(0, "127.0.0.1")
await new Promise((r) => api.once("listening", r))
const API = `http://127.0.0.1:${api.address().port}/w/api.php`
const context = {}
vm.runInNewContext(readFileSync(join(OR_HOME, "webapp/extensions/wikibase/module/scripts/wikidata-manifest-v1.0.js"), "utf8") + "\nthis.manifest = WikidataManifestV1_0;", context)
const manifest = { ...context.manifest, mediawiki: { ...context.manifest.mediawiki, api: API } }

const col = (columnName) => ({ type: "wbentityvariable", columnName })
const str = (columnName) => ({ type: "wbstringvariable", columnName })
const constant = (value) => ({ type: "wbstringconstant", value })
const entity = (id) => ({ type: "wbentityidvalueconstant", id, label: id })
const prop = (pid) => ({ type: "wbpropconstant", pid, label: pid, datatype: PROPERTIES[pid][0] })
const strict = { type: "qualifiers", valueMatcher: { type: "strict" }, pids: [] }
const statement = (value, extra = {}) => ({ value, qualifiers: [], references: [{ snaks: [{ prop: { type: "wbpropconstant", pid: "P854", label: "reference URL", datatype: "url" }, value: constant("https://example.org/source") }] }], mode: "add_or_merge", mergingStrategy: strict, ...extra })
const group = (pid, ...statements) => ({ property: prop(pid), statements })
export const SCHEMA = { siteIri: SITE, entityTypeSiteIRI: { item: SITE, property: SITE }, mediaWikiApiEndpoint: "{API}", entityEdits: [{
  type: "wbitemeditexpr", subject: col("item"),
  nameDescs: [{ name_type: "LABEL_IF_NEW", value: { type: "wbmonolingualexpr", language: { type: "wblanguageconstant", id: "en", label: "en" }, value: str("name") } },
    { name_type: "DESCRIPTION_IF_NEW", value: { type: "wbmonolingualexpr", language: { type: "wblanguageconstant", id: "en", label: "en" }, value: constant("someone") } }],
  statementGroups: [
    group("P9001", statement(str("name"))),
    group("P9002", statement(str("name")), statement(str("alias"))),
    group("P9003", statement(constant("same everywhere"))),
    group("P9004", statement(col("country"))),
    group("P9005", statement(col("country"))),
    group("P9006", statement(entity("Q5"))),
    group("P9007", statement(col("country"), { qualifiers: [{ prop: prop("P580"), value: { type: "wbdatevariable", columnName: "born" } }, { prop: prop("P9008"), value: constant("as qualifier") }] })),
    group("P9008", statement(constant("as main value"))),
    group("P9009", statement({ type: "wbquantityexpr", amount: str("population"), unit: null }), statement({ type: "wbquantityexpr", amount: constant("1e3"), unit: entity("Q712226") })),
    group("P9010", statement(col("country"))),
    group("P9011", statement(str("name"))),
    group("P9012", statement(col("country"))),
  ],
}] }

const or = await startOpenRefine({ port: 3342, workspace: join(tmpdir(), "oxrefine-wikibase-constraints"), jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"] })
const csrf = async () => (await or.get("/command/core/get-csrf-token")).token
const post = async (path, fields) => {
  const r = await fetch(`${or.url}/command/${path}${path.includes("?") ? "&" : "?"}csrf_token=${await csrf()}`, { method: "POST", body: new URLSearchParams(fields) })
  const text = await r.text()
  try { return JSON.parse(text) } catch { return { status: r.status, text: text.slice(0, 300) } }
}
const only = (columnName, value) => ({ mode: "row-based", facets: [{ type: "list", name: columnName, columnName, expression: "value", omitBlank: false, omitError: false, selection: [{ v: { v: value, l: value } }], selectBlank: false, selectError: false, invert: false }] })
const spaces = { identifierSpace: SITE, schemaSpace: "http://www.wikidata.org/prop/direct/" }
try {
  await post(`core/set-preference?name=wikibase.manifests`, { value: JSON.stringify([manifest]) })
  const project = await or.createProject(CSV, { name: "wikibase constraints" })
  const setup = [
    ...MATCHES.map(([columnName, id, name]) => ({ op: "core/recon-match-specific-topic-to-cells", engineConfig: only(columnName, id), columnName, match: { id, name, types: [] }, ...spaces })),
    ...NEW.map(([columnName, value, shareNewTopics]) => ({ op: "core/recon-mark-new-topics", engineConfig: only(columnName, value), columnName, shareNewTopics, service: "https://wikidata.reconci.link/en/api", ...spaces })),
  ]
  if ((await or.runOps(project, setup)).code !== "ok") throw new Error("recon setup")
  const schema = JSON.parse(JSON.stringify(SCHEMA).replaceAll("{API}", API))
  if ((await post("wikidata/save-wikibase-schema", { project, schema: JSON.stringify(schema) })).code !== "ok") throw new Error("save")
  const all = { facets: [], mode: "row-based" }
  const preview = await post(`wikidata/preview-wikibase-schema?project=${project}`, { schema: JSON.stringify(schema), manifest: JSON.stringify(manifest), engine: JSON.stringify(all) })
  const issues = await or.command("preview-expression", { project, cellIndex: "0", rowIndices: JSON.stringify([0, 1, 2, 3, 4, 5]), expression: "grel:wikibaseIssues()", repeat: "false", repeatCount: "10" })
  const strip = (v) => JSON.parse(JSON.stringify(v).replaceAll(API, "{API}"))
  writeFileSync(new URL("constraints.json", import.meta.url), JSON.stringify(strip({
    openrefine: (await or.get("/command/core/get-version")).version, generator: "tests/wikibase/constraints-gen.mjs",
    properties: PROPERTIES, schema: SCHEMA, warnings: preview.warnings ?? preview, issues: issues.results ?? issues,
    api: [...new Set(requests.map((q) => JSON.stringify(q)))].map((q) => JSON.parse(q)),
  }), null, 1) + "\n")
  for (const w of preview.warnings ?? []) console.log(w.type.padEnd(55), w.severity.padEnd(9), w.count, JSON.stringify(w.properties ?? {}).slice(0, 140))
  console.log(JSON.stringify(issues.results ?? issues).slice(0, 600))
} finally {
  or.stop()
  api.close()
}
