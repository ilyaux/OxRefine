// Regenerate tests/recon/fixtures.json from a real OpenRefine 3.10.1:  node tests/recon/gen.mjs
// Cells are as get-rows shows them, with recon ids renumbered in order of first appearance and
// judgmentHistoryEntry replaced by the index of the history step (both are random in OpenRefine).
// `answers` holds every query OpenRefine sent to service.mjs with the answer it got.
import { rmSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, CORPUS } from "./cases.mjs"
import { serve, MANIFEST, extend } from "./service.mjs"
import { withBase } from "../ops/server.mjs"

const svc = await serve()
rmSync(join(tmpdir(), "oxrefine-recon-fixtures"), { recursive: true, force: true })
const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-recon-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})

/** Recon ids → 1, 2, … in order of first appearance; history entry ids → step index. */
export function normalize(rows, history) {
  const ids = new Map()
  for (const r of rows)
    for (const c of r.cells) {
      if (!c?.r) continue
      if (!ids.has(c.r.id)) ids.set(c.r.id, ids.size + 1)
      c.r.id = ids.get(c.r.id)
      if ("judgmentHistoryEntry" in c.r) c.r.judgmentHistoryEntry = history.indexOf(c.r.judgmentHistoryEntry)
    }
  return rows
}

try {
  const out = []
  for (const c of CASES) {
    const project = await or.createProject(CORPUS, { name: "recon" })
    const res = await or.runOps(project, withBase(c.ops, svc.base))
    await or.idle(project)
    const past = (await or.get(`/command/core/get-history?project=${project}`)).past
    const table = await or.table(project)
    // get-rows sends recons once, in a pool, and cells name them by id
    const { pool } = await or.get(`/command/core/get-rows?project=${project}&start=0&limit=100000`)
    for (const r of table.rows) for (const c of r.cells) if (c?.r) c.r = structuredClone(pool.recons[c.r])
    normalize(table.rows, past.map((h) => h.id))
    const models = await or.get(`/command/core/get-models?project=${project}`)
    const recon = models.columnModel.columns.map((col) => ({ name: col.name, reconConfig: col.reconConfig ?? null, reconStats: col.reconStats ?? null }))
    const entry = { name: c.name, ops: c.ops, ...table, history: past.map((h) => h.description), columnRecon: recon }
    if (res.code !== "ok") entry.error = res.message ?? JSON.stringify(res)
    out.push(entry)
    await or.command("delete-project", { project })
    console.log(`${c.name.padEnd(34)} ${res.code}  ${past.length} steps`)
  }
  // one answer per distinct query, keyed by the query with its object keys sorted
  const sorted = (v) => (Array.isArray(v) ? v.map(sorted) : v && typeof v === "object" ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, sorted(v[k])])) : v)
  const answers = {}
  for (const { query, answer } of svc.seen) answers[JSON.stringify(sorted(query))] = answer
  // extend answers per id and property list, so a request batched differently gets the same rows
  const extensions = {}
  for (const q of svc.extended) {
    const a = extend(q)
    for (const id of q.ids) extensions[JSON.stringify([id, q.properties.map((p) => p.id)])] = a.rows[id] ?? null
    for (const m of a.meta) extensions[JSON.stringify(["meta", m.id])] = m
  }
  writeFileSync(
    new URL("fixtures.json", import.meta.url),
    JSON.stringify({ openrefine: "3.10.1", corpus: CORPUS, manifest: MANIFEST("{BASE}"), answers, extensions, cases: out }, null, 1) + "\n",
  )
  console.log(`${out.length} cases, ${Object.keys(answers).length} queries -> tests/recon/fixtures.json`)
} finally {
  or.stop()
  svc.close()
}
