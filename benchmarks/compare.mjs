// Same people data and recipe through each engine's HTTP API. No server runs when imported.
import { spawn } from "node:child_process"
import { openAsBlob, readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync, statSync } from "node:fs"
import { join, resolve, dirname } from "node:path"
import { tmpdir, cpus, totalmem, platform, release } from "node:os"
import { pathToFileURL } from "node:url"
import { createHash } from "node:crypto"
import { createServer } from "node:net"
import { csvDigest } from "./cardinality.mjs"
import { HERE, DATA, OX, STEPS, HTTP_CONNECTION, recipe, countryFacet, noFacets, sleep, peakMB, waitFor, kill, dataset, runOx } from "./ox.mjs"

const OR_HOME = process.env.OPENREFINE_HOME ?? resolve(HERE, "../../OpenRefine/openrefine-3.10.1")
const usage = "node benchmarks/compare.mjs [--runs 3] [--ox <binary>] [--out <report.json>] [--or-port 3333] [--ox-port 3030] [--timeout-seconds 180] [rows...]"
const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex")
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)]
const total = (r) => STEPS.reduce((n, step) => n + r[step], 0)

export function options(args) {
  const result = { runs: 1, ox: OX, out: null, orPort: 3333, oxPort: 3030, timeout: 180_000, sizes: [] }
  const flags = { "--runs": "runs", "--ox": "ox", "--out": "out", "--or-port": "orPort", "--ox-port": "oxPort", "--timeout-seconds": "timeout" }
  for (let i = 0; i < args.length; i++) {
    if (args[i] in flags) {
      const key = flags[args[i]], value = args[++i]
      if (!value || value.startsWith("--")) throw new Error(usage)
      result[key] = ["ox", "out"].includes(key) ? value : Number(value) * (key === "timeout" ? 1000 : 1)
    } else if (args[i].startsWith("--")) throw new Error(usage)
    else result.sizes.push(Number(args[i]))
  }
  if (![result.runs, result.orPort, result.oxPort, result.timeout, ...result.sizes].every((v) => Number.isSafeInteger(v) && v > 0) || result.orPort > 65535 || result.oxPort > 65535 || result.orPort === result.oxPort) throw new Error(usage)
  if (!result.sizes.length) result.sizes = [100_000, 250_000, 500_000, 1_000_000]
  return result
}

export function failure(error, log = "") {
  const message = String(error.message ?? error)
  const status = /OutOfMemoryError|Java heap space|GC overhead limit exceeded/i.test(`${message}\n${log}`) ? "out-of-memory"
    : /timeout|timed out|did not come up/i.test(`${error.name ?? ""} ${message}`) ? "timeout" : "error"
  return { status, error: status === "out-of-memory" ? "OpenRefine reported a Java heap allocation failure" : message.slice(0, 600) }
}

export function client(url, timeout) {
  const headers = { Connection: HTTP_CONNECTION }
  const get = (path) => fetch(`${url}${path}`, { headers, signal: AbortSignal.timeout(timeout) })
  const token = async () => (await (await get("/command/core/get-csrf-token")).json()).token
  const command = async (cmd, fields, { json = true } = {}) => {
    const upload = Object.values(fields).some((v) => v instanceof Blob)
    const form = upload ? new FormData() : new URLSearchParams()
    for (const [k, v] of Object.entries(fields)) form.append(k, typeof v === "string" || v instanceof Blob ? v : JSON.stringify(v))
    const r = await fetch(`${url}/command/core/${cmd}?csrf_token=${await token()}`, { method: "POST", headers, body: form, redirect: "manual", signal: AbortSignal.timeout(timeout) })
    if (r.status >= 400) throw new Error(`OpenRefine ${cmd}: HTTP ${r.status} ${(await r.text()).slice(0, 300)}`)
    return json ? r.json() : r
  }
  const idle = async (project) => {
    const until = performance.now() + timeout
    while ((await (await get(`/command/core/get-processes?project=${project}`)).json()).processes?.length) {
      if (performance.now() > until) throw new Error("OpenRefine operation timed out")
      await sleep(50)
    }
  }
  return { get, command, idle }
}

/** Fresh private workspace and JVM per run; captures allocation-failure evidence before cleanup. */
export async function runOR(csv, out, { port = 3333, memory = "1400M", timeout = 180_000 } = {}) {
  await new Promise((ok, fail) => {
    const probe = createServer()
    probe.once("error", fail)
    probe.listen({ host: "127.0.0.1", port, exclusive: true }, () => probe.close(ok))
  })
  const workspace = mkdtempSync(join(tmpdir(), "ox-openrefine-bench-"))
  const lib = join(OR_HOME, "server/target/lib"), url = `http://127.0.0.1:${port}`
  const p = spawn(join(OR_HOME, "server/target/jre/bin/java.exe"), [
    "-cp", `${join(OR_HOME, "server/classes")};${lib}\\*`, "-Xms256M", `-Xmx${memory}`, `-Drefine.memory=${memory}`,
    "-Drefine.max_form_content_size=1048576", `-Drefine.port=${port}`, "-Drefine.interface=127.0.0.1", "-Drefine.host=127.0.0.1",
    `-Drefine.webapp=${join(OR_HOME, "webapp")}`, `-Drefine.data_dir=${workspace}`, "-Drefine.headless=true", `-Djava.library.path=${lib}/native/windows`, "com.google.refine.Refine",
  ], { cwd: OR_HOME, stdio: ["ignore", "pipe", "pipe"] })
  let log = "", spawnError
  for (const pipe of [p.stdout, p.stderr]) pipe.on("data", (chunk) => { log = `${log}${chunk}`.slice(-65536) })
  p.on("error", (error) => { spawnError = error })
  const check = () => { if (spawnError) throw spawnError; if (p.exitCode !== null) throw new Error(`OpenRefine exited with ${p.exitCode}`) }
  const or = client(url, timeout)
  const r = { httpConnection: HTTP_CONNECTION }
  try {
    await waitFor(`${url}/command/core/get-csrf-token`, { headers: { Connection: HTTP_CONNECTION } }, timeout, check)
    await sleep(1000)
    r.idle = peakMB(p.pid)
    let t = performance.now()
    const imported = await or.command("create-project-from-upload", {
      "project-file": new File([await openAsBlob(csv)], "people.csv", { type: "text/csv" }), "project-name": "bench", format: "text/line-based/*sv",
      options: { encoding: "UTF-8", separator: ",", ignoreLines: -1, headerLines: 1, skipDataLines: 0, limit: -1,
        storeBlankRows: true, guessCellValueTypes: false, processQuotes: true, quoteCharacter: '"', storeBlankCellsAsNulls: true,
        includeFileSources: false, includeArchiveFileName: false, trimStrings: false },
    }, { json: false })
    const project = new URL(imported.headers.get("location") ?? "", url).searchParams.get("project")
    if (!project) throw new Error(`import failed: ${imported.status} ${(await imported.text()).slice(0, 300)}`)
    r.import = performance.now() - t
    r.rows = (await (await or.get(`/command/core/get-rows?project=${project}&start=0&limit=0`)).json()).total
    t = performance.now(); const facets = await or.command("compute-facets", { project, engine: countryFacet }); r.facet = performance.now() - t
    r.facetChoices = facets.facets?.[0]?.choices?.length
    if (!Number.isInteger(r.facetChoices)) throw new Error("OpenRefine returned no facet choices")
    t = performance.now(); const clusters = await or.command("compute-clusters", { project, engine: noFacets, clusterer: { type: "binning", function: "fingerprint", column: "city", params: {} } }); r.cluster = performance.now() - t
    if (!Array.isArray(clusters)) throw new Error("OpenRefine returned no clusters")
    r.clusters = clusters.length
    t = performance.now(); const applied = await or.command("apply-operations", { project, operations: recipe })
    if (applied.code === "error") throw new Error(`apply-operations: ${applied.message}`)
    await or.idle(project); r.recipe = performance.now() - t
    t = performance.now(); const exported = await or.command("export-rows", { project, format: "csv", engine: noFacets }, { json: false })
    writeFileSync(out, Buffer.from(await exported.arrayBuffer())); r.export = performance.now() - t
    r.peak = peakMB(p.pid)
    return { ...r, total: total(r), status: "completed" }
  } catch (error) {
    try { r.peak = peakMB(p.pid) } catch {}
    return { ...r, ...failure(error, log) }
  } finally {
    kill(p)
    await sleep(200)
    rmSync(workspace, { recursive: true, force: true })
  }
}

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

/** The people generator has no embedded line breaks; compare every parsed output cell. */
export function diff(a, b) {
  const lines = (path) => readFileSync(path, "utf8").replace(/\r\n/g, "\n").replace(/\n$/, "").split("\n")
  const A = lines(a), B = lines(b)
  const header = parse(A[0]), otherHeader = parse(B[0])
  const perCol = Object.fromEntries(header.map((h) => [h, 0])), samples = []
  let cells = 0, same = 0, widthsMatch = true
  for (let i = 0; i < Math.max(A.length, B.length); i++) {
    const x = parse(A[i] ?? ""), y = parse(B[i] ?? "")
    widthsMatch &&= x.length === header.length && y.length === header.length
    header.forEach((h, j) => {
      cells++
      if (x[j] === y[j]) same++
      else { perCol[h]++; if (samples.length < 12) samples.push({ row: i, column: h, oxrefine: x[j], openrefine: y[j] }) }
    })
  }
  return { linesOx: A.length, linesOR: B.length, cells, same, perCol, samples,
    equal: A.length === B.length && widthsMatch && JSON.stringify(header) === JSON.stringify(otherHeader) && cells === same }
}

export function aggregate(runs) {
  if (runs.some((r) => r.status !== "completed")) {
    const statuses = [...new Set(runs.map((r) => r.status))]
    return { status: statuses.length === 1 ? statuses[0] : "inconsistent", error: runs.find((r) => r.error)?.error ?? "Not every repetition completed" }
  }
  const keys = ["idle", "rows", "facetChoices", "clusters", ...STEPS, "total", "peak"]
  if (runs.some((run) => keys.some((key) => !Number.isFinite(run[key]) || run[key] < 0))) throw new Error("Benchmark returned an invalid metric")
  return { status: "completed", ...Object.fromEntries(keys.map((key) => [key, median(runs.map((r) => r[key]))])) }
}

export async function main(args) {
  const config = options(args), memory = process.env.OR_MEM ?? "1400M"
  if (process.platform !== "win32") throw new Error("This OpenRefine launcher uses the Windows 3.10.1 installation; use check.mjs for cross-platform OxRefine comparisons")
  if (process.env.OXREFINE_CODE_BUDGET_MIB) throw new Error("Clear OXREFINE_CODE_BUDGET_MIB for the default-storage people benchmark")
  if (!/^\d+[MG]$/i.test(memory)) throw new Error("OR_MEM must be a Java heap size such as 1400M")
  mkdirSync(DATA, { recursive: true })
  const output = resolve(config.out ?? join(HERE, memory === "1400M" ? "results.json" : `results-${memory}.json`))
  mkdirSync(dirname(output), { recursive: true })
  const binaryHash = sha256(config.ox)
  const referenceHash = sha256(join(OR_HOME, "webapp/WEB-INF/lib/openrefine-main.jar"))
  if (referenceHash !== "b1bee728e85b4aa44967427707ecdb501a6b4aa3d87608ba7b29115c858f0957") throw new Error("OpenRefine main artifact differs from the pinned 3.10.1 reference")
  const report = { when: new Date().toISOString(), openrefine: "3.10.1", orMemory: memory, runs: config.runs, statistic: "median of each phase, median of per-run totals and median peak RSS",
    sizes: config.sizes, complete: false, localMeasurement: true, httpConnection: HTTP_CONNECTION, memoryMetric: "PeakWorkingSet64", memoryUnit: "MiB",
    memoryScope: "process resident high-water mark; excludes parent Node and OS overhead",
    machine: { platform: platform(), release: release(), cpu: cpus()[0]?.model, cpus: cpus().length, totalMemoryMiB: Math.round(totalmem() / 2 ** 20), node: process.version },
    oxBinary: { file: config.ox.split(/[\\/]/).at(-1), sha256: binaryHash }, openrefineMainSha256: referenceHash,
    recipeSha256: sha256(join(HERE, "recipe.json")), generatorSha256: sha256(join(HERE, "gen.mjs")),
    jvm: { initialHeap: "256M", maxHeap: memory }, results: [] }
  let incorrect = false
  for (const n of config.sizes) {
    const csv = dataset(n), row = { rows: n, mb: Math.round(statSync(csv).size / 2 ** 20), inputSha256: sha256(csv), runs: [] }
    for (let i = 0; i < config.runs; i++) {
      const outputs = { ox: join(DATA, `release-ox-${n}-${i}.csv`), or: join(DATA, `release-or-${n}-${i}.csv`) }, run = {}
      for (const engine of i % 2 ? ["or", "ox"] : ["ox", "or"]) {
        console.log(`${n} rows, repetition ${i + 1}/${config.runs}, ${engine}`)
        if (engine === "or") run.or = await runOR(csv, outputs.or, { port: config.orPort, memory, timeout: config.timeout })
        else {
          const r = await runOx(csv, outputs.ox, { bin: config.ox, port: config.oxPort })
          run.ox = { ...r, total: total(r), status: "completed", output: await csvDigest(outputs.ox) }
          if (r.rows !== n || run.ox.output.lines !== n + 1) incorrect = true
        }
      }
      if (run.or.status === "completed") {
        run.diff = diff(outputs.ox, outputs.or)
        if (!run.diff.equal || run.ox.rows !== n || run.or.rows !== n || run.ox.facetChoices !== run.or.facetChoices || run.ox.clusters !== run.or.clusters) incorrect = true
      }
      row.runs.push(run)
    }
    row.ox = aggregate(row.runs.map((r) => r.ox)); row.or = aggregate(row.runs.map((r) => r.or))
    if (row.runs.every((r) => r.diff?.equal)) row.diff = row.runs[0].diff
    report.results.push(row)
    report.verifiedComparisons = report.results.flatMap((r) => r.runs).filter((r) => r.diff?.equal).length
    report.integrityValid = !incorrect && report.verifiedComparisons > 0 && sha256(config.ox) === binaryHash
      && report.results.every((r) => ["completed", "out-of-memory"].includes(r.or.status))
    report.complete = report.results.length === config.sizes.length
    report.valid = report.complete && report.integrityValid
    writeFileSync(output, JSON.stringify(report, null, 2) + "\n")
    if (["error", "timeout", "inconsistent"].includes(row.or.status)) throw new Error(`OpenRefine did not complete reliably; inspect ${output}`)
    if (!report.integrityValid) throw new Error(`Benchmark invalid: output mismatch or binary changed; inspect ${output}`)
  }
  if (!report.valid) throw new Error(`Benchmark invalid: output mismatch or binary changed; inspect ${output}`)
  console.log(output)
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.includes("--help")) console.log(usage)
  else await main(process.argv.slice(2)).catch((error) => { console.error(error.message); process.exitCode = 1 })
}
