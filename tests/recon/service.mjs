// A small reconciliation service (W3C Reconciliation API 0.2) for the recon fixtures. gen.mjs runs
// OpenRefine against it and records every query's answer in fixtures.json, so the Rust test can
// answer the same queries without this file. Routes:
//   /svc       manifest (GET), queries (POST form `queries`), data extension (POST form `extend`)
//   /svc500    every query fails with HTTP 500
//   /svcnull   answers `null`
import { createServer } from "node:http"

export const ENTITIES = [
  { id: "Q1", name: "Douglas Adams", types: ["Q5"], country: "Q145" },
  { id: "Q2", name: "Douglas Adam", types: ["Q5"], country: "Q30" },
  { id: "Q3", name: "Ada Lovelace", types: ["Q5"], country: "Q145" },
  { id: "Q4", name: "Paris Hilton", types: ["Q5"], country: "Q30" },
  { id: "Q10", name: "Paris", types: ["Q515"], country: "Q142" },
  { id: "Q11", name: "Paris", types: ["Q515"], country: "Q30" },
  { id: "Q12", name: "London", types: ["Q515"], country: "Q145" },
  { id: "Q13", name: "York", types: ["Q515"], country: "Q145" },
  { id: "Q14", name: "New York City", types: ["Q515"], country: "Q30" },
  { id: "Q145", name: "United Kingdom", types: ["Q6256"], country: "Q145" },
  { id: "Q30", name: "United States", types: ["Q6256"], country: "Q30" },
  { id: "Q142", name: "France", types: ["Q6256"], country: "Q142" },
]
const TYPES = { Q5: "human", Q515: "city", Q6256: "country" }

/** Data extension: per entity, its property values (several values make several rows). */
export const PROPERTIES = {
  P17: { name: "country", type: { id: "Q6256", name: "country" } },
  P1082: { name: "population" },
  P742: { name: "pseudonym" },
  P569: { name: "date of birth" },
  P2: { name: "score" },
  P3: { name: "alive" },
}
const VALUES = {
  Q1: { P569: [{ date: "1952-03-11T00:00:00+00:00" }], P742: [{ str: "DNA" }, { str: "Ford Prefect" }], P2: [{ float: 0.42 }], P3: [{ bool: false }] },
  Q2: { P742: [{ str: "Doug" }] },
  Q3: { P569: [{ date: "1815-12-10T00:00:00+00:00" }], P742: [{ str: "Ada" }, { str: "Countess" }, { str: "Enchantress of Numbers" }], P3: [{ bool: false }] },
  Q4: { P569: [{ date: "1981-02-17T00:00:00+00:00" }], P3: [{ bool: true }] },
  Q10: { P1082: [{ int: 2145906 }] },
  Q11: { P1082: [{ int: 24476 }] },
  Q12: { P1082: [{ int: 8799800 }], P742: [{ str: "The Smoke" }] },
  Q13: { P1082: [{ int: 141685 }] },
  Q14: { P1082: [{ int: 8804190 }], P742: [{ str: "Big Apple" }, { str: "Gotham" }] },
}
const name = (id) => ENTITIES.find((e) => e.id === id)?.name ?? id

/** The answer to one extend request: meta for the properties, rows for the ids. */
export function extend(q) {
  const meta = q.properties.map((p) => ({ id: p.id, name: PROPERTIES[p.id]?.name ?? p.id, ...(PROPERTIES[p.id]?.type ? { type: PROPERTIES[p.id].type } : {}) }))
  const rows = {}
  for (const id of q.ids) {
    if (id === "Q404") continue // an id the service doesn't know: no row
    const e = ENTITIES.find((x) => x.id === id)
    rows[id] = {}
    for (const p of q.properties) {
      rows[id][p.id] = p.id === "P17" ? (e ? [{ id: e.country, name: name(e.country) }] : []) : (VALUES[id]?.[p.id] ?? [])
    }
  }
  return { meta, rows }
}
const CODES = { UK: "Q145", US: "Q30", FR: "Q142" }

function lev(a, b) {
  const d = Array.from({ length: a.length + 1 }, (_, i) => [i, ...Array(b.length).fill(0)])
  for (let j = 1; j <= b.length; j++) d[0][j] = j
  for (let i = 1; i <= a.length; i++)
    for (let j = 1; j <= b.length; j++) d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1))
  return d[a.length][b.length]
}

/** The answer to one query object: {result: [...]}, or {} for the query "Broken". */
export function answer(q) {
  if (q.query === "Broken") return {}
  const text = String(q.query).toLowerCase()
  let found = []
  for (const e of ENTITIES) {
    const name = e.name.toLowerCase()
    const d = lev(text, name)
    // country codes (UK, US, FR) name their country exactly
    const code = CODES[q.query] === e.id
    if (!(code || name.includes(text) || text.includes(name) || d <= 2)) continue
    let score = code ? 100 : 100 - (d * 100) / Math.max(text.length, name.length)
    if (q.type && !e.types.includes(q.type)) score -= 30
    for (const p of q.properties ?? []) {
      const v = typeof p.v === "object" && p.v ? p.v.id : CODES[p.v] ?? p.v
      if ((p.pid === "P17" || p.pid === "P27") && v !== e.country) score -= 20
    }
    found.push({ id: e.id, name: e.name, type: e.types.map((t) => ({ id: t, name: TYPES[t] })), score })
  }
  // listed by id, not by score: OpenRefine sorts the candidates itself
  found.sort((a, b) => (a.id < b.id ? -1 : 1))
  const best = [...found].sort((a, b) => b.score - a.score)
  for (const r of found) r.match = r === best[0] && r.score === 100 && !(best[1]?.score === 100)
  if (q.limit) found = best.slice(0, q.limit)
  return { result: found }
}

export const MANIFEST = (base) => ({
  versions: ["0.2"],
  name: "OxRefine test service",
  identifierSpace: "http://ox.test/entity/",
  schemaSpace: "http://ox.test/prop/",
  defaultTypes: [{ id: "Q5", name: "human" }, { id: "Q515", name: "city" }],
  view: { url: "http://ox.test/entity/{{id}}" },
  preview: { url: `${base}/svc/preview?id={{id}}`, width: 300, height: 100 },
  extend: { propose_properties: { service_url: base, service_path: "/svc/properties" }, property_settings: [] },
})

/** Starts the service; `seen` collects every query object OpenRefine sent, with its answer, and
 * `extended` every extend request. */
export async function serve() {
  const seen = []
  const extended = []
  const server = createServer((req, res) => {
    let body = ""
    req.on("data", (d) => (body += d))
    req.on("end", () => {
      const path = req.url.split("?")[0]
      const base = `http://127.0.0.1:${server.address().port}`
      const json = (code, v) => res.writeHead(code, { "content-type": "application/json" }).end(typeof v === "string" ? v : JSON.stringify(v))
      const form = new URLSearchParams(req.method === "POST" ? body : req.url.split("?")[1] ?? "")
      if (path === "/svc500") return json(500, { error: "down" })
      if (path === "/svcnull") return json(200, "null")
      // propose properties for a type (data extension)
      if (path === "/svc/properties") {
        const type = form.get("type")
        const ids = type === "Q515" ? ["P17", "P1082", "P742"] : ["P17", "P742", "P569", "P2", "P3"]
        return json(200, { type, properties: ids.map((id) => ({ id, name: PROPERTIES[id].name })) })
      }
      if (path !== "/svc") return json(404, {})
      const ext = form.get("extend")
      if (ext) {
        const q = JSON.parse(ext)
        extended.push(q)
        return json(200, extend(q))
      }
      const queries = form.get("queries")
      if (!queries) return json(200, MANIFEST(base))
      const out = {}
      for (const [k, q] of Object.entries(JSON.parse(queries))) {
        out[k] = answer(q)
        seen.push({ query: q, answer: out[k] })
      }
      json(200, out)
    })
  }).listen(0, "127.0.0.1")
  await new Promise((r) => server.once("listening", r))
  return { base: `http://127.0.0.1:${server.address().port}`, seen, extended, close: () => server.close() }
}
