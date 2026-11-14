import test from "node:test"
import assert from "node:assert/strict"
import { mkdtempSync, readFileSync, writeFileSync, readdirSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { createHash } from "node:crypto"
import { DEFAULT_INPUT, options, validateReport, renderAssets, generate } from "./readme-assets.mjs"

// Deliberately synthetic; no authored test values are written to README assets.
function fixture() {
  const sha = "1".repeat(64), rows = 100_000
  const runs = [0, 1, 2].map((i) => {
    const measured = (factor) => {
      const phases = [1000, 1000, 1000, 10, 10]
      phases[i] = 100
      return { status: "completed", rows, facetChoices: 3, clusters: 2,
        ...Object.fromEntries(["import", "facet", "cluster", "recipe", "export"].map((k, j) => [k, phases[j] * factor])),
        total: phases.reduce((sum, n) => sum + n, 0) * factor, peak: 100 * factor }
    }
    return { ox: measured(1), or: measured(10), diff: { equal: true, cells: rows * 3, same: rows * 3 } }
  })
  const aggregated = (engine) => ({ ...runs[0][engine], import: 1000 * (engine === "ox" ? 1 : 10) })
  return { complete: true, valid: true, integrityValid: true, localMeasurement: true, when: "2026-11-14T00:00:00Z",
    openrefine: "3.10.1", orMemory: "1400M", runs: 3, sizes: [rows], statistic: "median of each phase, median of per-run totals and median peak RSS",
    memoryMetric: "PeakWorkingSet64", memoryUnit: "MiB", memoryScope: "process resident high-water mark; excludes parent Node and OS overhead",
    machine: { platform: "win32" }, oxBinary: { sha256: sha }, openrefineMainSha256: sha, recipeSha256: sha, generatorSha256: sha,
    results: [{ rows, inputSha256: sha, runs, ox: aggregated("ox"), or: aggregated("or") }] }
}

const source = { path: "synthetic-only.json", sha256: "a".repeat(64) }

test("arguments use the fresh release report and reject ambiguous input", () => {
  assert(DEFAULT_INPUT.replaceAll("\\", "/").endsWith("benchmarks/release-0.6.0/openrefine-1400M.json"))
  assert.equal(options([]), DEFAULT_INPUT)
  assert.equal(options(["--input", "report.json"]), resolve("report.json"))
  for (const args of [["report.json"], ["--input"], ["--input", "--help"], ["--unknown"], ["--input", "a", "b"]]) assert.throws(() => options(args))
})

test("totals use the median of per-run totals, not the sum of phase medians", () => {
  const report = fixture()
  assert.equal(validateReport(report), report)
  assert.equal(report.results[0].ox.total, 2120)
  const assets = renderAssets(report, source)
  assert.deepEqual(Object.keys(assets), ["bench-time.svg", "bench-memory.svg", "bench-steps.svg"])
  assert(assets["bench-time.svg"].includes("2.12 s"))
  assert(!assets["bench-time.svg"].includes("3.02 s"))
  assert(assets["bench-memory.svg"].includes("100 MiB"))
  assert(assets["bench-memory.svg"].includes("PeakWorkingSet64"))
  assert(assets["bench-memory.svg"].includes("not a resident-memory limit"))
  assert(assets["bench-steps.svg"].includes("100,000 rows"), "falls back to the largest completed pair when 1M is absent")
  assert(assets["bench-steps.svg"].includes("Each step has its own linear scale"))
  report.results[0].ox.total = 3020
  assert.throws(() => validateReport(report), /aggregate total/)
})

test("partial, unmatched, unmeasured and inconsistent reports cannot become charts", () => {
  const mutations = [
    (r) => { r.complete = false },
    (r) => { r.memoryMetric = "heap limit" },
    (r) => { r.memoryUnit = "MB" },
    (r) => { r.oxBinary.sha256 = "missing" },
    (r) => { r.results[0].runs.pop() },
    (r) => { r.results[0].runs[1].diff.equal = false },
    (r) => { r.results[0].runs[1].or.clusters++ },
    (r) => { r.results[0].ox.peak = NaN },
    (r) => { delete r.results[0].ox.total },
    (r) => { r.results[0].runs[0].ox.total++ },
    (r) => { r.results[0].or = { status: "error", error: "connection lost" } },
    (r) => { r.results[0].or = { status: "timeout" } },
  ]
  for (const mutate of mutations) {
    const report = fixture()
    mutate(report)
    assert.throws(() => renderAssets(report, source))
  }
})

test("only an explicit OOM in every repetition is labelled out of memory", () => {
  const report = fixture(), row = structuredClone(report.results[0])
  row.rows = 1_000_000
  row.ox.rows = row.rows
  row.or = { status: "out-of-memory", error: "Java heap allocation failure" }
  for (const run of row.runs) { run.ox.rows = row.rows; run.or = { ...row.or }; delete run.diff }
  report.results.push(row)
  report.sizes.push(row.rows)
  const assets = renderAssets(report, source)
  assert(assets["bench-time.svg"].includes("OR out of memory"))
  assert(assets["bench-memory.svg"].includes("No memory value is inferred for failed runs"))
  assert(assets["bench-steps.svg"].includes("100,000 rows"))
  row.runs[2].or.status = "timeout"
  assert.throws(() => renderAssets(report, source), /OOM must be explicit/)
})

test("output is self-contained, reduced-motion aware, and leaves qualitative diagrams intact", () => {
  const dir = mkdtempSync(join(tmpdir(), "ox-readme-assets-"))
  try {
    const input = join(dir, "source.json"), output = join(dir, "assets")
    const qualitative = ["hero.svg", "compat.svg", "spill.svg", "database-import.svg"]
    // generate creates output; writing sentinels afterwards also tests a repeat run.
    writeFileSync(input, JSON.stringify(fixture()))
    generate(input, output)
    for (const name of qualitative) writeFileSync(join(output, name), `preserve ${name}`)
    const result = generate(input, output)
    assert.equal(result.source.sha256, createHash("sha256").update(readFileSync(input)).digest("hex"))
    assert.equal(readdirSync(output).length, 7)
    for (const name of qualitative) assert.equal(readFileSync(join(output, name), "utf8"), `preserve ${name}`)
    for (const name of result.files) {
      const svg = readFileSync(join(output, name), "utf8")
      assert(svg.includes(`&quot;sha256&quot;:&quot;${result.source.sha256}&quot;`))
      assert.match(svg, /aria-labelledby="bench-[a-z]+-title bench-[a-z]+-desc"/)
      assert(svg.includes("prefers-reduced-motion:reduce"))
      assert(svg.includes("prefers-color-scheme:dark"))
      assert(svg.includes("@media(max-width:650px)"))
      assert(svg.includes("from{fill-opacity:.45}"), "animation changes paint, preserving measured bar lengths in every frame")
      assert(!svg.includes("scaleX") && !svg.includes("transform:"))
      assert(!/<(?:animate|script|image|foreignObject)\b|repeatCount=|indefinite|NaN|Infinity/.test(svg))
      for (const match of svg.matchAll(/\bwidth="([\d.-]+)"/g)) assert(Number.isFinite(+match[1]) && +match[1] >= 0)
    }
    writeFileSync(input, JSON.stringify({ complete: false }))
    const before = readFileSync(join(output, "bench-time.svg"), "utf8")
    assert.throws(() => generate(input, output))
    assert.equal(readFileSync(join(output, "bench-time.svg"), "utf8"), before, "validate the entire input before overwriting assets")
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
