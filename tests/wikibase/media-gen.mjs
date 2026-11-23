// Regenerate tests/wikibase/media.json:  node tests/wikibase/media-gen.mjs
// OpenRefine 3.10.1 uploads media to a mock Commons (MediaWiki API; a test bot password, never a
// real wiki): a new file from a local path with its wikitext, captions and a depicts statement, and
// an existing file (M100) whose captions, statements and wikitext change. Recorded: every request
// (file contents as size and SHA-256), the mock's answers, the history operation and the table.
import { createHash } from "node:crypto"
import { mkdirSync, writeFileSync } from "node:fs"
import { createServer } from "node:http"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"

const COMMONS = "https://commons.wikimedia.org/entity/"
const WIKIDATA = "http://www.wikidata.org/entity/"
// the files to upload, written next to the workspace; their paths go in the project
const files = join(tmpdir(), "oxrefine-media-files")
mkdirSync(files, { recursive: true })
export const FILES = { "sunset.png": "\x89PNG\r\n\x1a\n fake image bytes for a test", "notes.pdf": "%PDF-1.4 fake document for a test" }
for (const [name, body] of Object.entries(FILES)) writeFileSync(join(files, name), body, "latin1")
export const CSV = `media,file,name,caption,depicts
NEW1,${join(files, "sunset.png")},Sunset test.png,A sunset,Q145
M100,,Existing test.pdf,Some notes,Q30
`
export const EXISTING = { M100: { labels: { en: { language: "en", value: "Old caption" } }, statements: {} } }

let page = 2000
const requests = []
const api = createServer((req, res) => {
  const chunks = []
  req.on("data", (c) => chunks.push(c))
  req.on("end", () => {
    const body = Buffer.concat(chunks)
    const params = Object.fromEntries(new URLSearchParams(req.url.split("?")[1] ?? ""))
    const type = req.headers["content-type"] ?? ""
    const boundary = type.match(/boundary=([^;]+)/)?.[1]
    if (boundary) {
      for (const part of body.toString("latin1").split(`--${boundary}`)) {
        const m = part.match(/name="([^"]+)"(?:; filename="([^"]*)")?[\s\S]*?\r\n\r\n([\s\S]*)\r\n$/)
        if (!m) continue
        params[m[1]] = m[2] !== undefined ? { filename: m[2], size: Buffer.byteLength(m[3], "latin1"), sha256: createHash("sha256").update(Buffer.from(m[3], "latin1")).digest("hex") } : m[3]
      }
    } else for (const [k, v] of new URLSearchParams(body.toString())) params[k] = v
    const reply = answer(params)
    requests.push({ params, reply })
    res.writeHead(200, { "content-type": "application/json", ...(params.action === "login" ? { "set-cookie": "mocksession=1; Path=/" } : {}) }).end(JSON.stringify(reply))
  })
}).listen(0, "127.0.0.1")
await new Promise((r) => api.once("listening", r))
const API = `http://127.0.0.1:${api.address().port}/w/api.php`

const uploaded = {}
function answer(q) {
  if (q.action === "query" && q.meta?.includes("tokens")) return { batchcomplete: "", query: { tokens: q.type === "login" ? { logintoken: "logintoken+\\" } : { csrftoken: "csrftoken+\\" } } }
  if (q.action === "login") return { login: { result: "Success", lguserid: 7, lgusername: "Tester" } }
  if (q.action === "query" && q.meta?.includes("userinfo")) return { batchcomplete: "", query: { userinfo: { id: 7, name: "Tester" } } }
  if (q.action === "query" && q.meta === "wbcontentlanguages") return { batchcomplete: "", query: { wbcontentlanguages: Object.fromEntries(["en", "de", "fr", "mul"].map((code) => [code, { code }])) } }
  if (q.action === "upload") {
    const id = page++
    uploaded[q.filename] = id
    return { upload: { result: "Success", filename: q.filename, pageid: id, imageinfo: { url: `https://upload.example/${q.filename}` } } }
  }
  if (q.action === "query" && q.titles) {
    const pages = {}
    for (const title of q.titles.split("|")) {
      const name = title.replace(/^File:/, "")
      pages[uploaded[name] ?? "-1"] = uploaded[name] ? { pageid: uploaded[name], ns: 6, title } : { ns: 6, title, missing: "" }
    }
    return { batchcomplete: "", query: { pages } }
  }
  if (q.action === "wbgetentities") {
    const entities = {}
    for (const id of (q.ids ?? "").split("|").filter(Boolean)) {
      entities[id] = EXISTING[id] ? { id, type: "mediainfo", pageid: Number(id.slice(1)), ns: 6, title: `File:Existing test.pdf`, lastrevid: 700, modified: "2024-01-01T00:00:00Z", descriptions: {}, ...EXISTING[id] }
        : id.startsWith("M") ? { id, type: "mediainfo", pageid: Number(id.slice(1)), ns: 6, title: "File:Sunset test.png", lastrevid: 701, modified: "2024-01-01T00:00:00Z", labels: {}, descriptions: {}, statements: {} }
        : id.startsWith("P") ? { id, type: "property", datatype: "wikibase-item", labels: {}, claims: {} } : { id, missing: "" }
    }
    return { entities, success: 1 }
  }
  if (q.action === "wbeditentity") return { entity: { id: q.id, type: "mediainfo", lastrevid: 800 + page++, labels: {}, statements: {} }, success: 1 }
  if (q.action === "purge") return { batchcomplete: "", purge: (q.pageids ?? "").split("|").map((id) => ({ ns: 6, pageid: Number(id), purged: "" })) }
  if (q.action === "edit") return { edit: { result: "Success", pageid: 100, title: q.title, newrevid: 900 + page++ } }
  return { error: { code: "unknown", info: `mock: ${JSON.stringify(q).slice(0, 200)}` } }
}

const col = (columnName) => ({ type: "wbentityvariable", columnName })
const str = (columnName) => ({ type: "wbstringvariable", columnName })
const strict = { type: "qualifiers", valueMatcher: { type: "strict" }, pids: [] }
export const SCHEMA = { siteIri: COMMONS, entityTypeSiteIRI: { item: WIKIDATA, property: WIKIDATA, mediainfo: COMMONS }, mediaWikiApiEndpoint: "{API}", entityEdits: [{
  type: "wbmediainfoeditexpr", subject: col("media"), filePath: str("file"), fileName: str("name"),
  wikitext: { type: "wbstringconstant", value: "== {{int:filedesc}} ==\n{{Information|description=Test}}" }, overrideWikitext: true,
  nameDescs: [{ name_type: "LABEL", value: { type: "wbmonolingualexpr", language: { type: "wblanguageconstant", id: "en", label: "en" }, value: str("caption") } }],
  statementGroups: [{ property: { type: "wbpropconstant", pid: "P180", label: "depicts", datatype: "wikibase-item" }, statements: [{ value: col("depicts"), qualifiers: [], references: [], mode: "add_or_merge", mergingStrategy: strict }] }],
}] }

const or = await startOpenRefine({ port: 3343, workspace: join(tmpdir(), "oxrefine-wikibase-media"), jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"] })
const csrf = async () => (await or.get("/command/core/get-csrf-token")).token
const post = async (path, fields) => {
  const r = await fetch(`${or.url}/command/${path}${path.includes("?") ? "&" : "?"}csrf_token=${await csrf()}`, { method: "POST", body: new URLSearchParams(fields) })
  const text = await r.text()
  try { return JSON.parse(text) } catch { return { status: r.status, text: text.slice(0, 500) } }
}
const only = (columnName, value) => ({ mode: "row-based", facets: [{ type: "list", name: columnName, columnName, expression: "value", omitBlank: false, omitError: false, selection: [{ v: { v: value, l: value } }], selectBlank: false, selectError: false, invert: false }] })
try {
  const project = await or.createProject(CSV, { name: "commons upload" })
  const setup = [
    { op: "core/recon-mark-new-topics", engineConfig: only("media", "NEW1"), columnName: "media", shareNewTopics: false, service: "https://commonsreconcile.toolforge.org/en/api", identifierSpace: COMMONS, schemaSpace: COMMONS },
    { op: "core/recon-match-specific-topic-to-cells", engineConfig: only("media", "M100"), columnName: "media", match: { id: "M100", name: "Existing test.pdf", types: [] }, identifierSpace: COMMONS, schemaSpace: COMMONS },
    ...[["Q145", "United Kingdom"], ["Q30", "United States of America"]].map(([id, name]) => ({ op: "core/recon-match-specific-topic-to-cells", engineConfig: only("depicts", id), columnName: "depicts", match: { id, name, types: [] }, identifierSpace: WIKIDATA, schemaSpace: "http://www.wikidata.org/prop/direct/" })),
  ]
  if ((await or.runOps(project, setup)).code !== "ok") throw new Error("recon setup")
  const schema = JSON.parse(JSON.stringify(SCHEMA).replaceAll("{API}", API))
  const saved = await post("wikidata/save-wikibase-schema", { project, schema: JSON.stringify(schema) })
  if (saved.code !== "ok") throw new Error(`save: ${JSON.stringify(saved)}`)
  const login = await post("wikidata/login", { "wb-username": "Tester", "wb-password": "test-bot-password", "wb-api-endpoint": API, "remember-credentials": "on" })
  if (!login.logged_in) throw new Error(`login: ${JSON.stringify(login).slice(0, 300)}`)
  const before = requests.length
  const upload = { summary: "media test", maxlag: "5", editGroupsUrlSchema: "([[:toollabs:editgroups-commons/b/OR/${batch_id}|details]])", tag: "", maxEditsPerMinute: "600", resultsColumnName: "Wikibase editing results" }
  const performed = await post(`wikidata/perform-wikibase-edits?project=${project}`, { ...upload, engine: JSON.stringify({ facets: [], mode: "row-based" }) })
  await or.idle(project)
  const operations = (await or.get(`/command/core/get-operations?project=${project}`)).entries
  const table = await or.table(project, { recon: "brief" })
  const batch = JSON.stringify(requests).match(/\/b\/OR\/([0-9a-f]+)/)?.[1]
  const normalize = (v) => JSON.parse(JSON.stringify(v).replaceAll(API, "{API}").replaceAll(API.replace("/w/api.php", ""), "{HOST}").replaceAll(batch ?? "\u0000", "{BATCH}").replaceAll(JSON.stringify(files).slice(1, -1), "{FILES}"))
  writeFileSync(new URL("media.json", import.meta.url), JSON.stringify(normalize({
    openrefine: (await or.get("/command/core/get-version")).version, generator: "tests/wikibase/media-gen.mjs",
    csv: CSV.replaceAll(files, "{FILES}"), files: FILES, existing: EXISTING, schema: SCHEMA, upload, performed, operation: operations.at(-1),
    columns: table.columns, rows: table.rows.map((r) => r.cells), requests: requests.slice(before),
  }), null, 1) + "\n")
  for (const r of requests.slice(before)) console.log(r.params.action, r.params.meta ?? r.params.id ?? r.params.ids ?? r.params.titles ?? r.params.filename ?? r.params.title ?? "", JSON.stringify(r.params).slice(0, 220))
  console.log("rows", JSON.stringify(table.rows.map((r) => r.cells)).slice(0, 600))
} finally {
  or.stop()
  api.close()
}
