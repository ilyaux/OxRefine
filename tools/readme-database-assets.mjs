// node tools/readme-database-assets.mjs [completed paired database report.json]
import assert from "node:assert/strict"
import { readFileSync, writeFileSync, mkdirSync } from "node:fs"
import { resolve, relative, dirname } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import { createHash } from "node:crypto"
import { STYLE } from "./readme-assets.mjs"
import { expectedDigest } from "../benchmarks/database.mjs"

const ROOT = fileURLToPath(new URL("../", import.meta.url))
const names = { postgres: "PostgreSQL", mysql: "MySQL", mariadb: "MariaDB", sqlite: "SQLite" }
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)]
const hash = (s) => typeof s === "string" && /^[a-f0-9]{64}$/.test(s)
const xml = (s) => String(s).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;")
const num = (n) => n.toLocaleString("en-US", { maximumFractionDigits: 1 })
const text = (x, y, size, s, cls = "", anchor = "start") => `<text x="${x}" y="${y}" font-size="${size}" class="${cls}" text-anchor="${anchor}">${xml(s)}</text>`

export function validateDatabaseReport(r) {
  assert(r?.complete && r.valid && r.integrityValid && r.localMeasurement, "Report must be complete and valid")
  assert.equal(r.workload, "paired-database-100k-v1")
  assert.equal(r.rows, 100_000); assert.equal(r.columns, 3); assert.equal(r.nulls, 1031)
  assert.equal(r.openrefine, "3.10.1")
  assert(Number.isFinite(Date.parse(r.when)) && /^\d+M$/.test(r.orMemory))
  assert.equal(r.runs, 3, "This chart is labelled for three repetitions")
  assert.equal(r.memoryMetric, "PeakWorkingSet64"); assert.equal(r.memoryUnit, "MiB")
  assert.equal(r.machine?.platform, "win32")
  assert.equal(r.httpConnection, "close")
  assert([r.oxBinary?.sha256, r.openrefineMainSha256, r.openrefineDatabaseSha256, r.generatorSha256, r.oxRunnerSha256].every(hash))
  assert(typeof r.persistence === "string" && r.persistence.length > 0)
  assert.deepEqual(r.expected, expectedDigest())
  assert.deepEqual(r.results.map((e) => e.driver).sort(), Object.keys(names).sort(), "Require exactly four distinct backends")
  let pairs = 0
  for (const entry of r.results) {
    assert.equal(entry.runs.length, r.runs)
    assert(entry.runs.every((run, i) => run.repeat === i + 1 && run.order.join(",") === (i % 2 ? "or,ox" : "ox,or")), "Alternate tool order for every backend")
    for (const engine of ["ox", "or"]) {
      const summary = entry[engine], raw = entry.runs.map((x) => x[engine])
      if (summary?.status === "unavailable") {
        assert(engine === "or" && entry.driver === "sqlite", "Only the observed SQLite reference driver failure is allowed")
        assert(raw.every((x) => x?.status === "unavailable" && x.attempted === true && x.reason === summary.reason))
        assert.equal(summary.reason, "Bundled SQLite JDBC registration failed: No suitable driver")
        continue
      }
      assert.equal(summary?.status, "completed")
      for (const run of raw) {
        assert.equal(run.status, "completed")
        assert.equal(run.rows, r.rows); assert.equal(run.typedRowsVerified, r.rows)
        assert.deepEqual(run.output, r.expected, "Every exported row must match the independent fixture")
      }
      for (const key of ["importMs", "importPeakMiB"]) {
        assert(raw.every((x) => Number.isFinite(x[key]) && x[key] > 0), "Missing measurement")
        assert.equal(summary[key], median(raw.map((x) => x[key])), "Summary must be a median of raw measurements")
      }
    }
    if (entry.or.status === "completed") pairs++
  }
  assert(pairs > 0, "No completed reference comparisons")
  return r
}

export function renderDatabase(report, source) {
  validateDatabaseReport(report)
  assert(hash(source.sha256) && typeof source.path === "string")
  const entries = Object.keys(names).map((driver) => report.results.find((e) => e.driver === driver))
  const maxTime = Math.max(...entries.flatMap((e) => [e.ox.importMs, e.or.importMs ?? 0]))
  const maxMemory = Math.max(...entries.flatMap((e) => [e.ox.importPeakMiB, e.or.importPeakMiB ?? 0]))
  const rect = (x, y, value, maximum, cls, width = 300, height = 20) => `<rect x="${x}" y="${y}" width="${(value / maximum * width).toFixed(3)}" height="${height}" class="bar ${cls}"/>`
  let wide = text(48, 65, 36, "Database import: OxRefine vs OpenRefine", "bold")
    + text(48, 104, 20, "100,000 SQL rows · 3 columns · 1,031 NULLs · median of 3 runs", "muted")
    + text(48, 148, 20, "OxRefine 0.6.0", "blue bold") + text(254, 148, 20, "OpenRefine 3.10.1 reference", "gray")
    + text(48, 198, 20, "IMPORT TIME · lower is better", "bold") + text(648, 198, 20, "IMPORT PEAK RAM · lower is better", "bold")
  let compact = text(48, 72, 54, "Database import comparison", "bold")
    + text(48, 132, 35, "100k rows · 3 runs · lower is better", "muted")
    + text(48, 186, 34, "OxRefine 0.6.0", "blue bold") + text(580, 186, 34, "OpenRefine 3.10.1", "gray")
  const descriptions = []
  entries.forEach((e, i) => {
    const y = 248 + i * 160, completed = e.or.status === "completed"
    const faster = completed ? e.or.importMs / e.ox.importMs : null
    const lower = completed ? e.or.importPeakMiB / e.ox.importPeakMiB : null
    const speed = completed ? faster >= 1 ? `${num(faster)}× faster` : `${num(1 / faster)}× slower` : "Reference unavailable"
    const memory = completed ? lower >= 1 ? `${num(lower)}× lower peak` : `${num(1 / lower)}× higher peak` : "No RAM comparison"
    wide += text(48, y, 25, names[e.driver], "bold") + text(558, y, 24, speed, completed ? "blue bold" : "muted", "end")
      + text(648, y, 25, names[e.driver], "bold") + text(1152, y, 24, memory, completed ? "blue bold" : "muted", "end")
    for (const [x, key, maximum, unit] of [[48, "importMs", maxTime, "ms"], [648, "importPeakMiB", maxMemory, "MiB"]]) {
      wide += rect(x, y + 18, e.ox[key], maximum, "blue") + text(x + 510, y + 36, 23, `${num(e.ox[key])} ${unit}`, "blue", "end")
      if (completed) wide += rect(x, y + 55, e.or[key], maximum, "gray") + text(x + 510, y + 73, 23, `${num(e.or[key])} ${unit}`, "gray", "end")
      else wide += text(x, y + 76, 19, "OpenRefine: JDBC driver unavailable", "muted")
    }
    wide += `<path d="M48 ${y + 113}H1152" class="rule"/>`
    compact += text(48, y, 43, names[e.driver], "bold") + text(1152, y, 38, speed, "blue", "end")
      + text(48, y + 53, 35, `Time: Ox ${num(e.ox.importMs)} ms`, "blue")
      + text(648, y + 53, 35, completed ? `OR ${num(e.or.importMs)} ms` : "OR unavailable", "gray")
      + rect(48, y + 62, e.ox.importMs, maxTime, "blue", 450, 10)
      + (completed ? rect(648, y + 62, e.or.importMs, maxTime, "gray", 450, 10) : "")
      + text(48, y + 104, 35, `RAM: Ox ${num(e.ox.importPeakMiB)} MiB`, "blue")
      + text(648, y + 104, 35, completed ? `OR ${num(e.or.importPeakMiB)} MiB` : "No comparison", "gray")
      + rect(48, y + 113, e.ox.importPeakMiB, maxMemory, "blue", 450, 10)
      + (completed ? rect(648, y + 113, e.or.importPeakMiB, maxMemory, "gray", 450, 10) : "")
      + `<path d="M48 ${y + 128}H1152" class="rule"/>`
    descriptions.push(`${names[e.driver]}: OxRefine ${num(e.ox.importMs)} ms, ${num(e.ox.importPeakMiB)} MiB; OpenRefine ${completed ? `${num(e.or.importMs)} ms, ${num(e.or.importPeakMiB)} MiB; ${speed}, ${memory}` : "not measured: bundled SQLite JDBC registration failed"}.`)
  })
  wide += text(48, 925, 19, "Native import paths: Ox saves to disk; OR creates in memory. Startup is excluded from time.", "muted")
    + text(48, 957, 19, "RAM includes runtime overhead; excludes database servers. Each metric shares one linear scale.", "muted")
    + text(48, 989, 18, `${report.when.slice(0, 10)} · local Windows run · OR heap ${report.orMemory} · warm services / OS cache`, "muted")
  compact += text(48, 925, 31, "Ox saves to disk; OR creates in memory. Startup excluded.", "muted")
    + text(48, 968, 31, "Process peak RAM; database servers excluded. Warm caches.", "muted")
    + text(48, 1010, 31, `${report.when.slice(0, 10)} · local Windows · OR heap ${report.orMemory}`, "muted")
  const metadata = { ...source, when: report.when, statistic: report.statistic, oxBinarySha256: report.oxBinary.sha256,
    openrefineMainSha256: report.openrefineMainSha256, openrefineDatabaseSha256: report.openrefineDatabaseSha256, memoryMetric: report.memoryMetric }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="1040" viewBox="0 0 1200 1040" role="img" aria-labelledby="database-title database-desc">
<title id="database-title">Database import time and memory comparison</title>
<desc id="database-desc">${xml(`${descriptions.join(" ")} ${report.persistence} ${report.memoryScope} Source: ${source.path}.`)}</desc>
<metadata>${xml(JSON.stringify(metadata))}</metadata>
${STYLE}<rect width="1200" height="1040" fill="var(--bg)"/><rect x=".5" y=".5" width="1199" height="1039" class="rule"/>
<g class="wide">${wide}</g><g class="compact">${compact}</g>
</svg>
`
}

export function generateDatabase(input, output = resolve(ROOT, "docs/readme/database-comparison.svg")) {
  const bytes = readFileSync(input)
  const source = { path: relative(ROOT, resolve(input)).replaceAll("\\", "/"), sha256: createHash("sha256").update(bytes).digest("hex") }
  const svg = renderDatabase(JSON.parse(bytes), source)
  mkdirSync(dirname(output), { recursive: true }); writeFileSync(output, svg)
  return source
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  console.log(generateDatabase(process.argv[2] ?? resolve(ROOT, "benchmarks/release-0.6.0/database-comparison.json")))
}
