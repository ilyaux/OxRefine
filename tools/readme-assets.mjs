// Only measured benchmark charts are generated here; qualitative diagrams are hand-authored.
// node tools/readme-assets.mjs --input benchmarks/release-0.6.0/openrefine-1400M.json
import { readFileSync, writeFileSync, mkdirSync } from "node:fs"
import { createHash } from "node:crypto"
import { resolve, relative, basename } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"

const ROOT = fileURLToPath(new URL("../", import.meta.url))
export const DEFAULT_INPUT = resolve(ROOT, "benchmarks/release-0.6.0/openrefine-1400M.json")
const STEPS = ["import", "facet", "cluster", "recipe", "export"]
const NAMES = ["Import", "Text facet", "Clustering", "Recipe", "Export"]
const usage = "node tools/readme-assets.mjs [--input <completed release report.json>]"
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)]
const close = (a, b) => Math.abs(a - b) <= Math.max(1, Math.abs(a), Math.abs(b)) * 1e-10
const metric = (n) => Number.isFinite(n) && n >= 0
const hash = (s) => typeof s === "string" && /^[a-f0-9]{64}$/.test(s)
const xml = (s) => String(s).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;")
const number = (n) => n.toLocaleString("en-US", { maximumFractionDigits: 2 })
const time = (ms) => ms >= 1000 ? `${number(ms / 1000)} s` : `${number(ms)} ms`
const rows = (n) => `${number(n)} rows`
const coord = (n) => Number(n.toFixed(3))
const require = (condition, message) => { if (!condition) throw new Error(`Invalid benchmark report: ${message}`) }

export function options(args) {
  if (!args.length) return DEFAULT_INPUT
  if (args.length === 2 && args[0] === "--input" && args[1] && !args[1].startsWith("--")) return resolve(args[1])
  throw new Error(usage)
}

export function validateReport(report) {
  require(report?.complete === true && report.valid === true && report.integrityValid === true, "report must be complete and valid")
  require(report.localMeasurement === true && Number.isFinite(Date.parse(report.when)), "measurement provenance is missing")
  require(report.openrefine === "3.10.1" && /^\d+[MG]$/i.test(report.orMemory), "reference version or heap configuration is missing")
  require(report.memoryMetric === "PeakWorkingSet64" && report.memoryUnit === "MiB" && typeof report.memoryScope === "string" && report.memoryScope.length > 0, "expected Windows process PeakWorkingSet64 in MiB")
  require(report.machine?.platform === "win32", "PeakWorkingSet64 requires a Windows measurement")
  require([report.oxBinary?.sha256, report.openrefineMainSha256, report.recipeSha256, report.generatorSha256].every(hash), "artifact fingerprints are missing")
  require(Number.isSafeInteger(report.runs) && report.runs > 0, "invalid repetition count")
  require(Array.isArray(report.results) && report.results.length > 0, "no measured results")
  require(Array.isArray(report.sizes) && report.sizes.length === report.results.length, "size coverage is incomplete")
  const seen = new Set()
  for (const row of report.results) {
    require(Number.isSafeInteger(row.rows) && row.rows > 0 && !seen.has(row.rows) && report.sizes.includes(row.rows), "invalid or duplicate row count")
    seen.add(row.rows)
    require(hash(row.inputSha256), `input fingerprint missing at ${row.rows} rows`)
    require(Array.isArray(row.runs) && row.runs.length === report.runs, `missing repetitions at ${row.rows} rows`)
    for (const engine of ["ox", "or"]) {
      const value = row[engine], raw = row.runs.map((run) => run[engine])
      if (engine === "or" && value?.status === "out-of-memory") {
        require(raw.every((r) => r?.status === "out-of-memory"), "OOM must be explicit in every repetition")
        continue
      }
      require(value?.status === "completed" && raw.every((r) => r?.status === "completed"), `${engine} did not complete consistently at ${row.rows} rows`)
      for (const r of [value, ...raw]) {
        require([...STEPS, "total", "peak"].every((k) => metric(r[k])) && r.total > 0 && r.peak > 0, "invalid or missing measured metric")
        require(r.rows === row.rows, "imported row count differs from the input")
      }
      for (const r of raw) require(close(r.total, STEPS.reduce((sum, k) => sum + r[k], 0)), "raw total differs from its measured phases")
      // Phase medians need not add up to the median of the per-run totals.
      for (const k of [...STEPS, "total", "peak"]) require(close(value[k], median(raw.map((r) => r[k]))), `aggregate ${k} is not the median of repetitions`)
    }
    if (row.or.status === "completed") {
      require(row.runs.every((r) => r.diff?.equal === true && r.diff.cells > 0 && r.diff.same === r.diff.cells), "completed outputs were not equal in every repetition")
      require(row.runs.every((r) => r.ox.facetChoices === r.or.facetChoices && r.ox.clusters === r.or.clusters), "facet or cluster results differ")
    }
  }
  require(report.results.some((r) => r.or.status === "completed"), "no completed reference comparison")
  return report
}

const STYLE = `<style>
svg{--bg:#fff;--ink:#111113;--mute:#5f5f66;--rule:#e4e4e0;--ox:#1d4ed8;--or:#686870;--warn:#99500a}
@media(prefers-color-scheme:dark){svg{--bg:#0d1117;--ink:#e6edf3;--mute:#9198a1;--rule:#30363d;--ox:#74a7ff;--or:#a4acb8;--warn:#efb44c}}
text{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif;fill:var(--ink);font-variant-numeric:tabular-nums}
.muted{fill:var(--mute)}.bold{font-weight:600}.blue{fill:var(--ox)}.gray{fill:var(--or)}.warn{fill:var(--warn)}.rule{fill:none;stroke:var(--rule)}.compact{display:none}
.bar{animation:measure 2.4s cubic-bezier(.16,1,.3,1) 1}
@keyframes measure{from{fill-opacity:.45}to{fill-opacity:1}}
@media(max-width:650px){.wide{display:none}.compact{display:inline}}
@media(prefers-reduced-motion:reduce){.bar{animation:none}}
</style>`

const text = (x, y, size, content, cls = "", anchor = "start") => `<text x="${x}" y="${y}" font-size="${size}" class="${cls}" text-anchor="${anchor}">${xml(content)}</text>`
const bar = (x, y, width, cls, height) => `<rect x="${x}" y="${y}" width="${coord(width)}" height="${height}" class="bar ${cls}"/>`

function chart(report, source, { id, title, subtitle, entries, format, description, footer, perRowScale = false }) {
  const height = 280 + entries.length * 118
  const maximum = Math.max(...entries.flatMap((e) => [e.ox, e.or ?? 0])) || 1
  let wide = text(48, 66, 34, title, "bold") + text(48, 104, 20, subtitle, "muted")
  wide += text(48, 145, 20, "OxRefine", "blue bold") + text(210, 145, 20, `OpenRefine ${report.openrefine}`, "gray")
  let compact = text(48, 88, 58, title, "bold") + text(48, 148, 37, subtitle, "muted")
  entries.forEach((entry, i) => {
    const y = 208 + i * 118, scale = perRowScale ? Math.max(entry.ox, entry.or ?? 0) || 1 : maximum
    wide += text(48, y + 17, 24, entry.label, "bold")
    wide += bar(330, y - 13, entry.ox / scale * 630, "blue", 22)
    wide += text(1152, y + 5, 22, format(entry.ox), "blue", "end")
    if (entry.or != null) {
      wide += bar(330, y + 23, entry.or / scale * 630, "gray", 22) + text(1152, y + 41, 22, format(entry.or), "gray", "end")
    } else wide += text(330, y + 42, 21, "Out of memory", "warn")
    wide += `<path d="M48 ${y + 78}H1152" class="rule"/>`
    compact += text(48, y + 7, 41, entry.label, "bold")
    compact += text(48, y + 53, 36, `Ox ${format(entry.ox)}`, "blue")
    compact += text(636, y + 53, 36, entry.or == null ? "OR out of memory" : `OR ${format(entry.or)}`, entry.or == null ? "warn" : "gray")
    compact += bar(48, y + 68, entry.ox / scale * 510, "blue", 17)
    if (entry.or != null) compact += bar(636, y + 68, entry.or / scale * 510, "gray", 17)
  })
  wide += text(48, height - 62, 19, footer, "muted") + text(48, height - 30, 17, `${report.runs} repetitions · ${report.when.slice(0, 10)} · OR Java heap ${report.orMemory}`, "muted")
  compact += text(48, height - 62, 32, `${report.runs} repetitions · ${report.when.slice(0, 10)} · local Windows run`, "muted")
  compact += text(48, height - 20, 30, perRowScale ? "Each step has its own linear scale." : "All bars share one linear scale.", "muted")
  const details = entries.map((e) => `${e.label}: OxRefine ${format(e.ox)}; OpenRefine ${e.or == null ? "out of memory" : format(e.or)}.`).join(" ")
  const metadata = { source: source.path, sha256: source.sha256, when: report.when, repetitions: report.runs,
    statistic: report.statistic, memoryMetric: report.memoryMetric, memoryUnit: report.memoryUnit, memoryScope: report.memoryScope,
    oxBinarySha256: report.oxBinary.sha256, openrefineMainSha256: report.openrefineMainSha256, recipeSha256: report.recipeSha256,
    openrefine: report.openrefine, javaHeap: report.orMemory }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="${height}" viewBox="0 0 1200 ${height}" role="img" aria-labelledby="${id}-title ${id}-desc">
<title id="${id}-title">${xml(title)}</title>
<desc id="${id}-desc">${xml(`${description} ${details} ${report.runs} repetitions, measured ${report.when}. Java heap ${report.orMemory} is configuration, not a resident-memory limit. Source: ${source.path}.`)}</desc>
<metadata>${xml(JSON.stringify(metadata))}</metadata>
${STYLE}
<rect width="1200" height="${height}" fill="var(--bg)"/><rect x=".5" y=".5" width="1199" height="${height - 1}" class="rule"/>
<g class="wide">${wide}</g><g class="compact">${compact}</g>
</svg>
`
}

export function renderAssets(report, source) {
  validateReport(report)
  require(hash(source.sha256) && typeof source.path === "string", "source fingerprint is missing")
  const sorted = [...report.results].sort((a, b) => a.rows - b.rows)
  const entries = (key) => sorted.map((r) => ({ label: rows(r.rows), ox: r.ox[key], or: r.or.status === "completed" ? r.or[key] : null }))
  const pair = sorted.find((r) => r.rows === 1_000_000 && r.or.status === "completed") ?? sorted.filter((r) => r.or.status === "completed").at(-1)
  return {
    "bench-time.svg": chart(report, source, {
      id: "bench-time", title: "Measured workflow time", subtitle: "Import + facet + clustering + recipe + export",
      entries: entries("total"), format: time, description: "Median of per-run workflow totals; excludes process startup, data generation and output validation. Completed pairs have identical exported values.",
      footer: "Median of per-run totals; includes all five phases. Startup is excluded.",
    }),
    "bench-memory.svg": chart(report, source, {
      id: "bench-memory", title: "Peak resident memory", subtitle: "Median process high-water mark · MiB",
      entries: entries("peak"), format: (n) => `${number(n)} MiB`, description: `Measured with ${report.memoryMetric}. ${report.memoryScope}. No memory value is inferred for failed runs.`,
      footer: "Process peak includes runtime overhead; failed runs have no inferred peak.",
    }),
    "bench-steps.svg": chart(report, source, {
      id: "bench-steps", title: "Time spent in each step", subtitle: `${rows(pair.rows)} · median phase time`,
      entries: STEPS.map((k, i) => ({ label: NAMES[i], ox: pair.ox[k], or: pair.or[k] })), format: time, perRowScale: true,
      description: `Each phase is measured independently at ${rows(pair.rows)}. Each step has its own linear scale; phase medians need not sum to the median workflow total.`,
      footer: "Each step has its own linear scale. Phase medians do not define the total.",
    }),
  }
}

export function generate(input, output = resolve(ROOT, "docs/readme")) {
  const bytes = readFileSync(input)
  const source = { path: relative(ROOT, resolve(input)).replaceAll("\\", "/"), sha256: createHash("sha256").update(bytes).digest("hex") }
  const assets = renderAssets(JSON.parse(bytes.toString("utf8")), source)
  mkdirSync(output, { recursive: true })
  for (const [name, body] of Object.entries(assets)) writeFileSync(resolve(output, name), body)
  return { source, files: Object.keys(assets) }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.includes("--help")) console.log(usage)
  else {
    try {
      const result = generate(options(process.argv.slice(2)))
      console.log(`Source: ${basename(result.source.path)} · SHA-256 ${result.source.sha256}`)
      for (const file of result.files) console.log(`docs/readme/${file}`)
    } catch (error) { console.error(error.message); process.exitCode = 1 }
  }
}
