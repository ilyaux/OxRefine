// Catch performance regressions: run two OxRefine builds through the benchmark's steps on the same
// machine, alternately, and fail when the new one is clearly slower or uses clearly more memory.
//   node benchmarks/check.mjs --base <ox binary> [--new <ox binary>] [--runs 3] [rows...]
// --new defaults to target/release/ox. Times compare each build's fastest run (a busy runner only ever
// adds time, so one slow disk flush can't fail the check); a step regresses when the new time is over
// 1.3x the base one plus 15 ms (shorter steps are noise). Peak RAM compares medians: over 1.1x plus 8 MB.
import { mkdirSync } from "node:fs"
import { join } from "node:path"
import { DATA, OX, STEPS, dataset, runOx } from "./ox.mjs"

const args = process.argv.slice(2)
const opt = (k, d) => {
  const i = args.indexOf(k)
  return i < 0 ? d : args.splice(i, 2)[1]
}
const base = opt("--base")
const next = opt("--new", OX)
const runs = Number(opt("--runs", 3))
const sizes = args.length ? args.map(Number) : [250_000, 1_000_000]
if (!base) {
  console.error("usage: node benchmarks/check.mjs --base <ox binary> [--new <ox binary>] [--runs 3] [rows...]")
  process.exit(2)
}
mkdirSync(DATA, { recursive: true })
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)]
const best = (xs) => Math.min(...xs)
const TIME = { factor: 1.3, slack: 15 }
const RAM = { factor: 1.1, slack: 8 }

let failed = false
const lines = ["| rows | step | base | new | |", "|---|---|---|---|---|"]
for (const n of sizes) {
  const csv = dataset(n)
  const got = { base: [], new: [] }
  // alternate the builds so drift in the machine's load hits both alike
  for (let i = 0; i < runs; i++) {
    got.base.push(await runOx(csv, join(DATA, `check_base_${n}.csv`), { bin: base, port: 3041 }))
    got.new.push(await runOx(csv, join(DATA, `check_new_${n}.csv`), { bin: next, port: 3042 }))
  }
  for (const step of [...STEPS, "peak"]) {
    const pick = step === "peak" ? median : best
    const [b, x] = [pick(got.base.map((r) => r[step])), pick(got.new.map((r) => r[step]))]
    const lim = step === "peak" ? RAM : TIME
    const bad = x > b * lim.factor + lim.slack
    failed ||= bad
    const unit = step === "peak" ? " MB" : " ms"
    lines.push(`| ${n.toLocaleString("en")} | ${step} | ${Math.round(b)}${unit} | ${Math.round(x)}${unit} | ${bad ? "**slower**" : x < b / lim.factor - lim.slack ? "faster" : ""} |`)
  }
}
console.log(lines.join("\n"))
console.log(failed ? "\nRegression: a step is clearly slower or uses clearly more memory than the base build." : "\nNo regression.")
process.exit(failed ? 1 : 0)
