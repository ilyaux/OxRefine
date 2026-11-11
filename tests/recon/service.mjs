// A small reconciliation service (W3C Reconciliation API 0.2) for the recon fixtures. gen.mjs runs
// OpenRefine against it and records every query's answer in fixtures.json, so the Rust test can
// answer the same queries without this file. Routes:
//   /svc       manifest (GET) and queries (POST form `queries`)
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
})

/** Starts the service; `seen` collects every query object OpenRefine sent, with its answer. */
export async function serve() {
  const seen = []
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
      if (path !== "/svc") return json(404, {})
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
  return { base: `http://127.0.0.1:${server.address().port}`, seen, close: () => server.close() }
}
