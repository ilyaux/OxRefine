// Regenerate tests/sources/fixtures.json from a real OpenRefine 3.10.1:  node tests/sources/gen.mjs
// Each case goes through OpenRefine's import wizard with its defaults: load the clipboard text or
// download the URL (from a local server), guess the format, take the parser defaults, create the project.
import { createServer } from "node:http"
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, FILES } from "./cases.mjs"

const PORT = 41873
const server = createServer((req, res) => {
  const f = FILES[req.url]
  if (!f) return res.writeHead(404).end("not found")
  res.writeHead(200, { "content-type": f[0] }).end(Buffer.from(f[1], f[2] ?? "utf8"))
}).listen(PORT, "127.0.0.1")

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-sources-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const token = async () => (await (await fetch(`${or.url}/command/core/get-csrf-token`)).json()).token
/** OpenRefine reads multipart bodies only when loading raw data; other calls are urlencoded. */
async function post(path, fields = {}, multipart = false) {
  const form = multipart ? new FormData() : new URLSearchParams()
  for (const [k, v] of Object.entries(fields)) form.append(k, typeof v === "string" ? v : JSON.stringify(v))
  const r = await fetch(`${or.url}/command/core/${path}${path.includes("?") ? "&" : "?"}csrf_token=${await token()}`, { method: "POST", body: form })
  const text = await r.text()
  try {
    return JSON.parse(text)
  } catch {
    return {}
  }
}

try {
  const out = []
  for (const c of CASES) {
    const { jobID } = await post("create-importing-job")
    const ctl = (sub, extra = "") => `importing-controller?controller=core%2Fdefault-importing-controller&jobID=${jobID}&subCommand=${sub}${extra}`
    const status = async () => (await post(`get-importing-job-status?jobID=${jobID}`)).job?.config ?? {}
    await post(ctl("load-raw-data"), c.clipboard !== undefined ? { clipboard: c.clipboard } : { download: `http://127.0.0.1:${PORT}${c.url}` }, true)
    let cfg = await status()
    while (cfg.state === "loading-raw-data") (await sleep(100), (cfg = await status()))
    if (cfg.state === "error") throw new Error(`${c.name}: ${JSON.stringify(cfg)}`)
    const format = cfg.rankedFormats[0]
    const { options } = await post(ctl("initialize-parser-ui", `&format=${encodeURIComponent(format)}`))
    delete options.dom
    // the wizard page turns the sheet list into the `sheets` option the Excel parser reads
    const sheets = options.sheetRecords?.filter((r) => r.selected).map(({ name, fileNameAndSheetIndex }) => ({ name, fileNameAndSheetIndex, selected: true }))
    await post(ctl("create-project"), { format, options: { ...options, ...(sheets ? { sheets } : {}), ...c.choose, projectName: "sources" } })
    for (let i = 0; !(cfg = await status()).projectID; i++) {
      if (cfg.state === "error" || i > 600) throw new Error(`${c.name}: no project: ${JSON.stringify(cfg).slice(0, 600)}`)
      await sleep(100)
    }
    const project = cfg.projectID
    const cols = (await or.get(`/command/core/get-models?project=${project}`)).columnModel.columns
    const rows = (await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000`)).rows
    const file = cfg.retrievalRecord.files[0]
    out.push({ ...c, fileName: file.fileName, format, options, columns: cols.map((x) => x.name), rows: rows.map((r) => cols.map((x) => r.cells[x.cellIndex] ?? null)) })
    await or.command("delete-project", { project })
    console.log(`${c.name.padEnd(32)} ${format.padEnd(26)} ${cols.length} cols x ${rows.length} rows`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", files: FILES, cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/sources/fixtures.json`)
} finally {
  or.stop()
  server.close()
}
