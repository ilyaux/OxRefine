import test from "node:test"
import assert from "node:assert/strict"
import { mkdtempSync, writeFileSync, readFileSync, existsSync, rmSync } from "node:fs"
import { execFileSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { options, aggregate, failure, diff } from "./compare.mjs"

test("comparison arguments validate before starting either engine", () => {
  assert.equal(options(["--runs", "3", "--out", "report.json", "100000"]).runs, 3)
  assert.deepEqual(options([]).sizes, [100000, 250000, 500000, 1000000])
  for (const args of [["--runs"], ["--runs", "0"], ["--other"], ["NaN"], ["--or-port", "3030"]]) assert.throws(() => options(args))
})

test("medians preserve per-run totals and never turn failed repetitions into completed results", () => {
  const row = (n) => ({ status: "completed", idle: n, rows: 10, facetChoices: 1, clusters: 2, import: n, facet: n, cluster: n, recipe: n, export: n, total: n * 5, peak: n })
  assert.equal(aggregate([row(1), row(200), row(3)]).total, 15)
  assert.equal(aggregate([row(1), failure(new Error("network problem"))]).status, "inconsistent")
  assert.equal(failure(new Error("network problem")).status, "error")
  assert.equal(failure(new Error("operation timed out")).status, "timeout")
  assert.equal(failure(new Error("HTTP 500"), "java.lang.OutOfMemoryError: Java heap space").status, "out-of-memory")
})

test("output equality includes row and column counts, parsed cells and headers", () => {
  const dir = mkdtempSync(join(tmpdir(), "ox-compare-test-"))
  try {
    const a = join(dir, "a.csv"), b = join(dir, "b.csv")
    writeFileSync(a, 'id,name\n1,"a,b"\n')
    writeFileSync(b, 'id,name\r\n1,"a,b"\r\n')
    assert.equal(diff(a, b).equal, true)
    writeFileSync(b, 'id,name,extra\n1,"a,b",x\n')
    assert.equal(diff(a, b).equal, false)
    writeFileSync(b, 'id,name\n1,"a,b"\n2,x\n')
    assert.equal(diff(a, b).equal, false)
    writeFileSync(a, 'id,name\n1,abc\n')
    writeFileSync(b, 'id,name\n1,abc \n')
    assert.equal(diff(a, b).equal, false, "trailing cell spaces are data")
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test("report rejects partial artifacts and never labels generic failure as OOM", () => {
  const dir = mkdtempSync(join(tmpdir(), "ox-report-test-"))
  try {
    const input = join(dir, "input.json"), output = join(dir, "report.html")
    const command = [fileURLToPath(new URL("report.mjs", import.meta.url)), "--input", input, "--out", output]
    writeFileSync(input, JSON.stringify({ complete: false, valid: false }))
    assert.throws(() => execFileSync(process.execPath, command, { stdio: "pipe" }))
    assert.equal(existsSync(output), false)
    const completed = { status: "completed", total: 9000, peak: 10, import: 1, facet: 1, cluster: 1, recipe: 1, export: 1 }
    writeFileSync(input, JSON.stringify({ when: "2026-11-14T00:00:00Z", runs: 3, orMemory: "1400M", results: [
      { rows: 100, ox: completed, or: completed }, { rows: 200, ox: completed, or: { status: "error", error: "network failure" } },
    ] }))
    execFileSync(process.execPath, command, { stdio: "pipe" })
    const html = readFileSync(output, "utf8")
    assert(html.includes("9,0 с"), "use per-run total median instead of adding phase medians")
    assert(html.includes("ошибка, см. JSON"))
    assert(!html.includes("нет памяти"))
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
