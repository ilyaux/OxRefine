// Generate actual OpenRefine 3.10.1 imports. No live Wikipedia/Wikidata requests.
import { createHash } from "node:crypto"
import { createServer } from "node:http"
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { CASES } from "./cases.mjs"

process.env.OPENREFINE_HOME ??= resolve(import.meta.dirname, "../../target/reference-3.10.1/openrefine-3.10.1")
const mainJar = join(process.env.OPENREFINE_HOME, "webapp/WEB-INF/lib/openrefine-main.jar")
const jarHash = createHash("sha256").update(readFileSync(mainJar)).digest("hex")
if (jarHash !== "b1bee728e85b4aa44967427707ecdb501a6b4aa3d87608ba7b29115c858f0957") throw new Error("Reference jar checksum differs")
const { startOpenRefine } = await import("../../tools/openrefine.mjs")
const portProbe = createServer()
await new Promise((done, fail) => {
  portProbe.once("error", fail)
  portProbe.listen(3339, "127.0.0.1", done)
})
await new Promise((done) => portProbe.close(done))
const requests = []
const service = createServer(async (req, res) => {
  let body = ""
  for await (const chunk of req) body += chunk
  const query = new URLSearchParams(body || new URL(req.url, "http://localhost").search)
  const jobs = JSON.parse(query.get("queries") ?? "{}")
  const result = {}
  for (const [key, job] of Object.entries(jobs)) {
    requests.push(job)
    const id = job.query?.endsWith("Alpha") ? "Q111" : "Q222"
    result[key] = { result: [{ id, name: id === "Q111" ? "Alpha entity" : "Beta entity", type: [], score: 100, match: true }] }
  }
  res.setHeader("Content-Type", "application/json")
  res.end(JSON.stringify(result))
})
await new Promise((done) => service.listen(0, "127.0.0.1", done))
const serviceUrl = `http://127.0.0.1:${service.address().port}/api`
const workspace = mkdtempSync(join(tmpdir(), "oxrefine-wikitext-"))
let or
const normalize = (v) => JSON.parse(JSON.stringify(v).replaceAll(serviceUrl, "{SERVICE}"))
try {
  or = await startOpenRefine({ port: 3339, workspace, jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"] })
  const results = []
  for (const c of CASES) {
    const before = requests.length
    const options = normalize(c.options)
    if (options.reconService) options.reconService = serviceUrl
    let res
    try {
      res = await or.command("create-project-from-upload", {
        "project-file": new File([c.text], "table.wiki"), "project-name": "wikitext",
        format: "text/wiki", options,
      }, { json: false })
    } catch (error) {
      const status = Number(/HTTP (\d+)/.exec(error.message)?.[1])
      if (!status) throw error
      results.push({ ...c, error: { status } })
      console.log(`${c.name}: HTTP ${status}`)
      continue
    }
    const project = new URL(res.headers.get("location") ?? "", or.url).searchParams.get("project")
    if (!project) throw new Error(`${c.name}: ${res.status} ${(await res.text()).slice(0, 300)}`)
    const models = await or.get(`/command/core/get-models?project=${project}`)
    const cols = models.columnModel.columns
    const { rows, pool } = await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000`)
    const cells = rows.map((r) => cols.map((col) => {
      const cell = structuredClone(r.cells[col.cellIndex] ?? null)
      if (cell?.r) {
        cell.r = structuredClone(pool.recons[cell.r])
        delete cell.r.id
      }
      return cell
    }))
    results.push(normalize({ ...c, columns: cols.map((col) => col.name), rows: cells,
      columnRecon: cols.map((col) => ({ config: col.reconConfig ?? null, stats: col.reconStats ?? null })),
      queries: requests.slice(before) }))
    await or.command("delete-project", { project })
    console.log(`${c.name}: ${cols.length} columns, ${rows.length} rows`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({
    openrefine: "3.10.1", referenceMainJarSha256: jarHash,
    source: "https://github.com/OpenRefine/OpenRefine/blob/3.10.1/main/src/com/google/refine/importers/WikitextImporter.java",
    method: "create-project-from-upload text/wiki; get-models and get-rows; random recon IDs removed; local mock reconciliation only",
    cases: results,
  }, null, 1) + "\n")
} finally {
  or?.stop()
  service.close()
}
