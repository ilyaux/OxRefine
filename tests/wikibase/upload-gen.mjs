// Regenerate tests/wikibase/upload.json:  node tests/wikibase/upload-gen.mjs
// OpenRefine 3.10.1 logs in to a mock MediaWiki API (a test bot password, never a real wiki) and
// uploads the edits of one schema. Recorded: every API request OpenRefine sent (tokens, batch ids
// and times normalized), what the mock answered, the history operation and the table afterwards.
import { writeFileSync } from "node:fs"
import { createServer } from "node:http"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CSV, MATCHES, NEW, SITE } from "./cases.mjs"

// what the mock wiki already holds: Douglas Adams has a country and a label
export const EXISTING = {
  Q42: { labels: { en: { language: "en", value: "Douglas Adams" } }, descriptions: {}, aliases: {},
    claims: { P27: [{ mainsnak: { snaktype: "value", property: "P27", datavalue: { value: { "entity-type": "item", "numeric-id": 145, id: "Q145" }, type: "wikibase-entityid" }, datatype: "wikibase-item" }, type: "statement", id: "Q42$11111111-1111-1111-1111-111111111111", rank: "normal" }] } },
  Q7259: { labels: {}, descriptions: {}, aliases: {}, claims: {} },
}
export const DATATYPES = { P27: "wikibase-item", P31: "wikibase-item", P569: "time", P580: "time", P854: "url", P813: "time", P106: "wikibase-item" }
let next = 1000
const requests = []
const api = createServer((req, res) => {
  const chunks = []
  req.on("data", (c) => chunks.push(c))
  req.on("end", () => {
    const params = Object.fromEntries(new URLSearchParams(req.url.split("?")[1] ?? ""))
    const body = Buffer.concat(chunks).toString()
    const type = req.headers["content-type"] ?? ""
    if (type.startsWith("multipart/")) {
      for (const m of body.matchAll(/name="([^"]+)"\r\n(?:[^\r\n]*\r\n)*\r\n([\s\S]*?)\r\n--/g)) params[m[1]] = m[2]
    } else for (const [k, v] of new URLSearchParams(body)) params[k] = v
    const reply = answer(params)
    requests.push({ method: req.method, params, reply })
    res.writeHead(200, { "content-type": "application/json", ...(params.action === "login" ? { "set-cookie": "mocksession=1; Path=/" } : {}) }).end(JSON.stringify(reply))
  })
}).listen(0, "127.0.0.1")
await new Promise((r) => api.once("listening", r))
const API = `http://127.0.0.1:${api.address().port}/w/api.php`

function answer(q) {
  if (q.action === "query" && q.meta?.includes("tokens")) return { batchcomplete: "", query: { tokens: q.type === "login" ? { logintoken: "logintoken+\\" } : { csrftoken: "csrftoken+\\" } } }
  if (q.action === "login") return { login: { result: "Success", lguserid: 7, lgusername: "Tester" } }
  if (q.action === "query" && q.meta?.includes("userinfo")) return { batchcomplete: "", query: { userinfo: { id: 7, name: "Tester", rights: ["edit", "createpage"] } } }
  if (q.action === "query" && q.meta === "wbcontentlanguages") return { batchcomplete: "", query: { wbcontentlanguages: Object.fromEntries(["en", "de", "fr", "mul"].map((code) => [code, { code }])) } }
  if (q.action === "query" && q.meta?.includes("siteinfo")) return { batchcomplete: "", query: { general: { sitename: "Mock", lang: "en" } } }
  if (q.action === "wbgetentities") {
    const entities = {}
    for (const id of (q.ids ?? "").split("|").filter(Boolean)) {
      const base = { id, pageid: 1, ns: id.startsWith("P") ? 120 : 0, title: id.startsWith("P") ? `Property:${id}` : id, lastrevid: 500, modified: "2024-01-01T00:00:00Z" }
      entities[id] = id.startsWith("P") ? { ...base, type: "property", datatype: DATATYPES[id] ?? "string", labels: {}, descriptions: {}, aliases: {}, claims: {} }
        : EXISTING[id] ? { ...base, type: "item", sitelinks: {}, ...EXISTING[id] } : { id, missing: "" }
    }
    return { entities, success: 1 }
  }
  if (q.action === "wbeditentity") {
    const data = JSON.parse(q.data ?? "{}")
    const id = q.new ? `Q${next++}` : q.id
    // Wikibase answers with the whole entity, claims as a map by property
    const claims = Array.isArray(data.claims) ? {} : data.claims ?? {}
    return { entity: { labels: {}, descriptions: {}, aliases: {}, sitelinks: {}, ...data, claims, id, type: "item", lastrevid: 600 + next++ }, success: 1 }
  }
  return { error: { code: "unknown", info: `mock: ${JSON.stringify(q)}` } }
}

const col = (columnName) => ({ type: "wbentityvariable", columnName })
const str = (columnName) => ({ type: "wbstringvariable", columnName })
const prop = (pid, label) => ({ type: "wbpropconstant", pid, label, datatype: DATATYPES[pid] })
const lang = (id) => ({ type: "wblanguageconstant", id, label: id })
const term = (name_type, value) => ({ name_type, value: { type: "wbmonolingualexpr", language: lang("en"), value } })
const strict = { type: "qualifiers", valueMatcher: { type: "strict" }, pids: [] }
const statement = (value, extra = {}) => ({ value, qualifiers: [], references: [], mode: "add_or_merge", mergingStrategy: strict, ...extra })
export const SCHEMA = { siteIri: SITE, entityTypeSiteIRI: { item: SITE, property: SITE }, mediaWikiApiEndpoint: "{API}", entityEdits: [{
  type: "wbitemeditexpr", subject: col("item"),
  nameDescs: [term("LABEL_IF_NEW", str("name")), term("DESCRIPTION", { type: "wbstringconstant", value: "test person" }), term("ALIAS", str("alias"))],
  statementGroups: [
    { property: prop("P27", "country of citizenship"), statements: [statement(col("country"), { references: [{ snaks: [{ prop: prop("P854", "reference URL"), value: str("website") }] }] })] },
    { property: prop("P31", "instance of"), statements: [statement({ type: "wbentityidvalueconstant", id: "Q5", label: "human" })] },
    { property: prop("P569", "date of birth"), statements: [statement({ type: "wbdatevariable", columnName: "born" }, { qualifiers: [{ prop: prop("P580", "start time"), value: { type: "wbdateconstant", value: "2000" } }] })] },
    { property: prop("P106", "occupation"), statements: [statement(null, { mode: "delete", mergingStrategy: { type: "property" } })] },
  ],
}] }

const or = await startOpenRefine({ port: 3341, workspace: join(tmpdir(), "oxrefine-wikibase-upload"), jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"] })
const csrf = async () => (await or.get("/command/core/get-csrf-token")).token
const post = async (path, fields) => {
  const r = await fetch(`${or.url}/command/${path}${path.includes("?") ? "&" : "?"}csrf_token=${await csrf()}`, { method: "POST", body: new URLSearchParams(fields) })
  const text = await r.text()
  try { return JSON.parse(text) } catch { return { status: r.status, text: text.slice(0, 500) } }
}
const only = (columnName, value) => ({ mode: "row-based", facets: [{ type: "list", name: columnName, columnName, expression: "value", omitBlank: false, omitError: false, selection: [{ v: { v: value, l: value } }], selectBlank: false, selectError: false, invert: false }] })
const spaces = { identifierSpace: SITE, schemaSpace: "http://www.wikidata.org/prop/direct/" }
try {
  const project = await or.createProject(CSV, { name: "wikibase upload" })
  const setup = [
    ...MATCHES.map(([columnName, id, name]) => ({ op: "core/recon-match-specific-topic-to-cells", engineConfig: only(columnName, id), columnName, match: { id, name, types: [] }, ...spaces })),
    ...NEW.map(([columnName, value, shareNewTopics]) => ({ op: "core/recon-mark-new-topics", engineConfig: only(columnName, value), columnName, shareNewTopics, service: "https://wikidata.reconci.link/en/api", ...spaces })),
  ]
  if ((await or.runOps(project, setup)).code !== "ok") throw new Error("recon setup")
  const schema = JSON.parse(JSON.stringify(SCHEMA).replaceAll("{API}", API))
  const saved = await post("wikidata/save-wikibase-schema", { project, schema: JSON.stringify(schema) })
  if (saved.code !== "ok") throw new Error(`save: ${JSON.stringify(saved)}`)
  const login = await post("wikidata/login", { "wb-username": "Tester", "wb-password": "test-bot-password", "wb-api-endpoint": API, "remember-credentials": "on" })
  if (!login.logged_in) { console.log(JSON.stringify(requests, null, 0).slice(0, 3000)); throw new Error(`login: ${JSON.stringify(login).slice(0, 200)}`) }
  const before = requests.length
  const upload = { summary: "test batch", maxlag: "5", editGroupsUrlSchema: "([[:toollabs:editgroups/b/OR/${batch_id}|details]])", tag: "openrefine-${version}", maxEditsPerMinute: "600", resultsColumnName: "Wikibase editing results" }
  const performed = await post(`wikidata/perform-wikibase-edits?project=${project}`, { ...upload, engine: JSON.stringify({ facets: [], mode: "row-based" }) })
  await or.idle(project)
  const operations = (await or.get(`/command/core/get-operations?project=${project}`)).entries
  const table = await or.table(project, { recon: "brief" })
  // OpenRefine's batch id and version are random or local; recorded as placeholders
  const batch = JSON.stringify(requests).match(/editgroups\/b\/OR\/([0-9a-f]+)/)?.[1]
  const normalize = (v) => JSON.parse(JSON.stringify(v).replaceAll(API, "{API}").replaceAll(API.replace("/w/api.php", ""), "{HOST}").replaceAll(batch ?? "\u0000", "{BATCH}"))
  writeFileSync(new URL("upload.json", import.meta.url), JSON.stringify(normalize({
    openrefine: (await or.get("/command/core/get-version")).version, generator: "tests/wikibase/upload-gen.mjs",
    schema: SCHEMA, existing: EXISTING, datatypes: DATATYPES, upload, performed, operation: operations.at(-1), columns: table.columns,
    rows: table.rows.map((r) => r.cells), login: requests.slice(0, before), requests: requests.slice(before),
  }), null, 1) + "\n")
  console.log(`login ${before} requests, upload ${requests.length - before} requests; history: ${operations.at(-1)?.description}`)
  for (const r of requests.slice(before)) console.log(r.method, r.params.action, r.params.new ?? r.params.id ?? r.params.ids ?? r.params.meta ?? "", (r.params.data ?? "").slice(0, 150))
} finally {
  or.stop()
  api.close()
}
