// Catch performance regressions: run two OxRefine builds through the benchmark's steps on the same
// machine, alternately, and fail when the new one is clearly slower or uses clearly more memory.
//   node benchmarks/check.mjs --base <ox binary> [--new <ox binary>] [--runs 3] [rows...]
// --new defaults to target/release/ox. Times compare each build's fastest run (a busy runner only ever
// adds time, so one slow disk flush can't fail the check); a step regresses when the new time is over
// 1.3x the base one plus 15 ms (shorter steps are noise). Peak RAM compares medians: over 1.1x plus 8 MB.
import { mkdirSync, writeFileSync, readFileSync } from "node:fs"
import { join, basename } from "node:path"
import { cpus, totalmem, freemem, platform, release } from "node:os"
import { createHash } from "node:crypto"
import assert from "node:assert/strict"
import { DATA, OX, STEPS, dataset, runOx } from "./ox.mjs"
import { MODES, CARDINALITY_STEPS, cardinalityDataset, runCardinality, csvDigest } from "./cardinality.mjs"

const args = process.argv.slice(2)
let invalidOption = false
const opt = (k, d) => {
  const i = args.indexOf(k)
  if (i >= 0 && (!args[i + 1] || args[i + 1].startsWith("--"))) invalidOption = true
  return i < 0 ? d : args.splice(i, 2)[1]
}
const base = opt("--base")
const next = opt("--new", OX)
const runs = Number(opt("--runs", 3))
const profile = opt("--profile", "people")
const history = Number(opt("--history", 40))
const out = opt("--out")
const record = args.includes("--record")
if (record) args.splice(args.indexOf("--record"), 1)
const sizes = args.length ? args.map(Number) : profile === "cardinality" ? [10_000] : [250_000, 1_000_000]
if (invalidOption || !next || (!base && !record) || (base && record) || !["people", "cardinality"].includes(profile) ||
    ![runs, history, ...sizes].every((n) => Number.isSafeInteger(n) && n > 0)) {
  console.error("usage: node benchmarks/check.mjs (--base <ox binary> | --record) [--new <ox binary>] [--runs 3] [--profile people|cardinality] [--history 40] [--out results.json] [rows...]")
  process.exit(2)
}
mkdirSync(DATA, { recursive: true })
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)]
const best = (xs) => Math.min(...xs)
const TIME = { factor: 1.3, slack: 15 }
const RAM = { factor: 1.1, slack: 8 }

let failed = false
const lines = ["| dataset | rows | step | base | new | |", "|---|---|---|---|---|---|"]
const results = []
for (const n of sizes) {
 for (const mode of profile === "cardinality" ? MODES : ["people"]) {
  const data = profile === "cardinality" ? await cardinalityDataset(n, mode) : dataset(n)
  const got = { base: [], new: [] }
  let expectedOutput
  // alternate the builds so drift in the machine's load hits both alike
  for (let i = 0; i < runs; i++) {
    for (const [which, bin, port] of [["base", base, 3041], ["new", next, 3042]]) {
      if (!bin) continue
      const output = join(DATA, `check_${which}_${mode}_${n}.csv`)
      const measurement = profile === "cardinality"
        ? await runCardinality(data, output, { bin, port, history })
        : await runOx(data, output, { bin, port })
      if (profile === "people") {
        assert.equal(measurement.rows, n, "people import row count differs")
        measurement.output = await csvDigest(output)
        assert.equal(measurement.output.lines, n + 1, "people export row count differs")
        if (expectedOutput) assert.deepEqual(measurement.output, expectedOutput, "people exports differ between builds or repetitions")
        else expectedOutput = measurement.output
      }
      got[which].push(measurement)
    }
  }
  for (const step of [...(profile === "cardinality" ? CARDINALITY_STEPS : STEPS), "peak"]) {
    const pick = step === "peak" ? median : best
    const [b, x] = [base ? pick(got.base.map((r) => r[step])) : null, pick(got.new.map((r) => r[step]))]
    const lim = step === "peak" ? RAM : TIME
    const bad = b !== null && x > b * lim.factor + lim.slack
    failed ||= bad
    const unit = step === "peak" ? " MB" : " ms"
    lines.push(`| ${mode} | ${n.toLocaleString("en")} | ${step} | ${b === null ? "—" : `${Math.round(b)}${unit}`} | ${Math.round(x)}${unit} | ${bad ? "**slower**" : b !== null && x < b / lim.factor - lim.slack ? "faster" : ""} |`)
  }
  results.push({ mode, rows: n, ...(profile === "cardinality" ? { distinct: data.distinct, input: data.input } : {}), runs: got })
 }
}
if (out) {
  const binary = (path) => ({ file: basename(path), sha256: createHash("sha256").update(readFileSync(path)).digest("hex") })
  writeFileSync(out, JSON.stringify({ when: new Date().toISOString(), profile, history, runs, localMeasurement: true,
    memoryMetric: process.platform === "darwin" ? "sampled resident MB after phases (cardinality peak=max samples); transient peaks may be missed" : "cumulative process resident high-water mark MB",
    machine: { platform: platform(), release: release(), cpu: cpus()[0]?.model, cpus: cpus().length, totalMemoryMB: Math.round(totalmem() / 2 ** 20), freeMemoryMBAfter: Math.round(freemem() / 2 ** 20) },
    binaries: { ...(base ? { base: binary(base) } : {}), new: binary(next) }, results }, null, 2) + "\n")
}
console.log(lines.join("\n"))
console.log(record ? "\nRecorded local measurements; no base build was compared." : failed ? "\nRegression: a step is clearly slower or uses clearly more memory than the base build." : "\nNo regression.")
process.exit(failed ? 1 : 0)
