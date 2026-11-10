// OpenRefine vs OxRefine, same data, same recipe, same HTTP-driven steps.
// node benchmarks/compare.mjs [rows...]   (defaults: 100000 250000 500000 1000000)
// Each engine gets a fresh server process per dataset; peak RAM = OS peak working set of that process.
import { spawn, execFileSync } from "node:child_process"
import { existsSync, openAsBlob, openSync, readFileSync, writeFileSync, mkdirSync } from "node:fs"
import { join, resolve } from "node:path"

const HERE = import.meta.dirname
const DATA = join(HERE, "data")
const OR_HOME = resolve(HERE, "../../OpenRefine/openrefine-3.10.1")
const OR_MEM = process.env.OR_MEM ?? "1400M" // refine.ini default (the .exe launcher uses 1024M)
const OX = resolve(HERE, "../target/release/ox.exe")
const sizes = process.argv.slice(2).map(Number)
const SIZES = sizes.length ? sizes : [100_000, 250_000, 500_000, 1_000_000]
const recipe = JSON.parse(readFileSync(join(HERE, "recipe.json"), "utf8"))
const countryFacet = {
  facets: [{ type: "list", name: "country", columnName: "country", expression: "value", omitBlank: false, omitError: false,
    selection: [], selectBlank: false, selectError: false, invert: false }],
  mode: "row-based",
}
const noFacets = { facets: [], mode: "row-based" }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const now = () => performance.now()
mkdirSync(DATA, { recursive: true })

function peakMB(pid) {
  const out = execFileSync("powershell", ["-NoProfile", "-Command", `(Get-Process -Id ${pid}).PeakWorkingSet64`]).toString()
  return Math.round(Number(out.trim()) / 2 ** 20)
}

async function waitFor(url, init, ms = 120_000) {
  const t = now()
  while (now() - t < ms) {
    try {
      const r = await fetch(url, init)
      if (r.status < 500) return
    } catch {}
    await sleep(200)
  }
  throw new Error(`server at ${url} did not come up`)
}

function kill(p) {
  try { execFileSync("taskkill", ["/PID", String(p.pid), "/T", "/F"], { stdio: "ignore" }) } catch {}
}

// ---------------------------------------------------------------- OxRefine
const OXURL = "http://127.0.0.1:3030/api"
async function ox(cmd, args = {}) {
  const r = await fetch(`${OXURL}/${cmd}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(args) })
  const j = await r.json()
  if (!r.ok) throw new Error(`ox ${cmd}: ${j}`)
  return j.data
}

async function runOx(csv, out) {
  const p = spawn(OX, ["serve", "--port", "3030"], { stdio: "ignore" })
  try {
    await waitFor(`${OXURL}/summary`, { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" })
    const idle = peakMB(p.pid)
    const r = { idle }
    let t = now(); const s = await ox("open", { path: csv }); r.import = now() - t; r.rows = s.rows
    t = now(); const f = await ox("facets", { engine: countryFacet }); r.facet = now() - t; r.facetChoices = f[0].distinct
    t = now(); const c = await ox("cluster", { engine: noFacets, column: "city", method: "fingerprint" }); r.cluster = now() - t; r.clusters = c.length ?? console.log("cluster response", JSON.stringify(c).slice(0, 500))
    t = now(); await ox("apply_recipe", { ops: recipe }); r.recipe = now() - t
    t = now(); await ox("export_csv", { engine: noFacets, path: out }); r.export = now() - t
    r.peak = peakMB(p.pid)
    return r
  } finally { kill(p) }
}

// ---------------------------------------------------------------- OpenRefine
const ORURL = "http://127.0.0.1:3333"
async function token() {
  return (await (await fetch(`${ORURL}/command/core/get-csrf-token`)).json()).token
}
async function or(cmd, fields, { json = true, query = "" } = {}) {
  // OpenRefine reads multipart only for uploads; every other command wants urlencoded fields
  const upload = Object.values(fields).some((v) => v instanceof Blob)
  const form = upload ? new FormData() : new URLSearchParams()
  for (const [k, v] of Object.entries(fields)) form.append(k, typeof v === "string" || v instanceof Blob ? v : JSON.stringify(v))
  const r = await fetch(`${ORURL}/command/core/${cmd}?csrf_token=${await token()}${query}`, { method: "POST", body: form, redirect: "manual" })
  if (r.status >= 400) throw new Error(`OpenRefine ${cmd}: HTTP ${r.status} ${(await r.text()).slice(0, 300)}`)
  return json ? r.json() : r
}
async function idleOR(project) {
  for (;;) {
    const j = await (await fetch(`${ORURL}/command/core/get-processes?project=${project}`)).json()
    if (!j.processes?.length) return
    await sleep(50)
  }
}

async function runOR(csv, out) {
  const ws = join(DATA, "or-workspace")
  mkdirSync(ws, { recursive: true })
  const java = join(OR_HOME, "server/target/jre/bin/java.exe")
  const lib = join(OR_HOME, "server/target/lib")
  const p = spawn(java, [
    "-cp", `${join(OR_HOME, "server/classes")};${lib}\\*`,
    `-Xms256M`, `-Xmx${OR_MEM}`, `-Drefine.memory=${OR_MEM}`, "-Drefine.max_form_content_size=1048576",
    "-Drefine.port=3333", "-Drefine.interface=127.0.0.1", "-Drefine.host=127.0.0.1", `-Drefine.webapp=${join(OR_HOME, "webapp")}`,
    `-Drefine.data_dir=${ws}`, "-Drefine.headless=true", `-Djava.library.path=${lib}/native/windows`, "com.google.refine.Refine",
  ], { cwd: OR_HOME, stdio: "ignore" })
  try {
    await waitFor(`${ORURL}/command/core/get-csrf-token`)
    await sleep(1000)
    const idle = peakMB(p.pid)
    const r = { idle }
    let t = now()
    const res = await or("create-project-from-upload", {
      "project-file": new File([await openAsBlob(csv)], "people.csv", { type: "text/csv" }),
      "project-name": "bench",
      format: "text/line-based/*sv",
      options: { encoding: "UTF-8", separator: ",", ignoreLines: -1, headerLines: 1, skipDataLines: 0, limit: -1,
        storeBlankRows: true, guessCellValueTypes: false, processQuotes: true, quoteCharacter: '"',
        storeBlankCellsAsNulls: true, includeFileSources: false, includeArchiveFileName: false, trimStrings: false },
    }, { json: false })
    const project = new URL(res.headers.get("location") ?? "", ORURL).searchParams.get("project")
    if (!project) throw new Error(`import failed: ${res.status} ${(await res.text()).slice(0, 300)}`)
    r.import = now() - t
    r.rows = (await (await fetch(`${ORURL}/command/core/get-rows?project=${project}&start=0&limit=0`)).json()).total
    t = now(); const f = await or("compute-facets", { project, engine: countryFacet }); r.facet = now() - t
    r.facetChoices = f.facets?.[0]?.choices?.length ?? console.log("facet response", JSON.stringify(f).slice(0, 500))
    t = now()
    const c = await or("compute-clusters", { project, engine: noFacets, clusterer: { type: "binning", function: "fingerprint", column: "city", params: {} } })
    r.cluster = now() - t; r.clusters = c.length ?? console.log("cluster response", JSON.stringify(c).slice(0, 500))
    t = now()
    const a = await or("apply-operations", { project, operations: recipe })
    if (a.code === "error") throw new Error(`apply-operations: ${a.message}`)
    await idleOR(project)
    r.recipe = now() - t
    t = now()
    const e = await or("export-rows", { project, format: "csv", engine: noFacets }, { json: false })
    writeFileSync(out, Buffer.from(await e.arrayBuffer()))
    r.export = now() - t
    r.peak = peakMB(p.pid)
    await or("delete-project", { project }).catch(() => {})
    return r
  } finally { kill(p) }
}

// ---------------------------------------------------------------- output equivalence
function parse(line) {
  const out = []; let cur = "", q = false
  for (let i = 0; i < line.length; i++) {
    const ch = line[i]
    if (q) { if (ch === '"') { if (line[i + 1] === '"') { cur += '"'; i++ } else q = false } else cur += ch }
    else if (ch === '"') q = true
    else if (ch === ",") { out.push(cur); cur = "" }
    else cur += ch
  }
  out.push(cur)
  return out
}
function diff(a, b) {
  const A = readFileSync(a, "utf8").replace(/\r\n/g, "\n").trimEnd().split("\n")
  const B = readFileSync(b, "utf8").replace(/\r\n/g, "\n").trimEnd().split("\n")
  const header = parse(A[0])
  const perCol = Object.fromEntries(header.map((h) => [h, 0]))
  const samples = []
  let cells = 0, same = 0
  for (let i = 0; i < Math.max(A.length, B.length); i++) {
    const x = parse(A[i] ?? ""), y = parse(B[i] ?? "")
    header.forEach((h, j) => {
      cells++
      if (x[j] === y[j]) same++
      else { perCol[h]++; if (samples.length < 12) samples.push({ row: i, column: h, oxrefine: x[j], openrefine: y[j] }) }
    })
  }
  return { linesOx: A.length, linesOR: B.length, cells, same, perCol, samples }
}

// ---------------------------------------------------------------- main
const results = []
for (const n of SIZES) {
  const csv = join(DATA, `people_${n}.csv`)
  if (!existsSync(csv)) execFileSync(process.execPath, [join(HERE, "gen.mjs"), String(n)], { stdio: ["ignore", openSync(csv, "w"), "inherit"] })
  const row = { rows: n, mb: Math.round(readFileSync(csv).length / 2 ** 20) }
  const oxOut = join(DATA, `ox_${n}.csv`), orOut = join(DATA, `or_${n}.csv`)
  console.log(`\n== ${n.toLocaleString()} rows (${row.mb} MB)`)
  row.ox = await runOx(csv, oxOut)
  console.log("OxRefine  ", row.ox)
  try {
    row.or = await runOR(csv, orOut)
    console.log("OpenRefine", row.or)
    row.diff = diff(oxOut, orOut)
    console.log(`identical cells: ${row.diff.same}/${row.diff.cells}`, row.diff.perCol)
  } catch (e) {
    row.or = { error: String(e.message ?? e) }
    console.log("OpenRefine FAILED:", row.or.error)
  }
  results.push(row)
  writeFileSync(join(HERE, OR_MEM === "1400M" ? "results.json" : `results-${OR_MEM}.json`), JSON.stringify({ when: new Date().toISOString(), orMemory: OR_MEM, results }, null, 2))
}
