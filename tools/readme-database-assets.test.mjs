import test from "node:test"
import assert from "node:assert/strict"
import { readFileSync, mkdtempSync, writeFileSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { renderDatabase, validateDatabaseReport, generateDatabase } from "./readme-database-assets.mjs"

// Exercise the public measurement-to-chart boundary with the retained raw report.
const report = () => JSON.parse(readFileSync(new URL("../benchmarks/release-0.6.0/database-comparison.json", import.meta.url)))
const source = { path: "measured-report.json", sha256: "a".repeat(64) }

test("database chart retains measured values, ratios, provenance and missing baseline", () => {
  const r = report(), svg = renderDatabase(r, source)
  assert.equal(validateDatabaseReport(r), r)
  assert(svg.includes("Native import paths: Ox saves to disk; OR creates in memory"))
  assert(svg.includes("JDBC driver unavailable") && svg.includes("No RAM comparison"))
  const pg = r.results.find((e) => e.driver === "postgres")
  const ratio = (pg.or.importMs / pg.ox.importMs).toLocaleString("en-US", { maximumFractionDigits: 1 })
  assert(svg.includes(`${ratio}× faster`))
  assert(svg.includes(`&quot;sha256&quot;:&quot;${source.sha256}&quot;`))
  assert.equal([...svg.matchAll(/class="bar (?:blue|gray)"/g)].length, 28)
  assert(svg.includes("prefers-reduced-motion:reduce") && svg.includes("prefers-color-scheme:dark"))
  assert(!/NaN|Infinity|<script|<animate|scaleX|transform:/.test(svg))
  const bars = [...svg.matchAll(/<rect x="(?:48|648)" y="[\d.]+" width="([\d.]+)" height="(?:10|20)" class="bar (?:blue|gray)"/g)].map((m) => +m[1])
  assert(bars.every((n) => n > 0 && n <= 450))
})

test("incomplete, mismatched and unmeasured inputs cannot produce comparison claims", () => {
  for (const mutate of [
    (r) => { r.complete = false },
    (r) => { r.results[0].ox.importMs++ },
    (r) => { r.results[0].runs[0].or.output.sha256 = "b".repeat(64) },
    (r) => { r.results[0].runs[0].or.typedRowsVerified-- },
    (r) => { r.results[0].runs[1].order = ["ox", "or"] },
    (r) => { r.results[0].runs.pop() },
    (r) => { r.results[0].runs[0].or.importPeakMiB = NaN },
    (r) => { r.memoryMetric = "heap limit" },
    (r) => { r.results.find((e) => e.driver === "sqlite").or.reason = "timeout" },
    (r) => { r.results.find((e) => e.driver === "sqlite").runs[0].or.attempted = false },
  ]) {
    const r = report(); mutate(r)
    assert.throws(() => renderDatabase(r, source))
  }
})

test("invalid source leaves existing artwork untouched", () => {
  const dir = mkdtempSync(join(tmpdir(), "ox-db-svg-test-"))
  try {
    const input = join(dir, "report.json"), output = join(dir, "chart.svg")
    writeFileSync(output, "preserve"); writeFileSync(input, '{"complete":false}')
    assert.throws(() => generateDatabase(input, output))
    assert.equal(readFileSync(output, "utf8"), "preserve")
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
