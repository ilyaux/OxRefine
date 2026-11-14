// Linux coordinator stays outside its private cgroup; the producer, verifier and backend join it.
import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { randomUUID } from "node:crypto"
import { mkdirSync, readFileSync, writeFileSync, existsSync, rmdirSync, realpathSync, createWriteStream, statfsSync } from "node:fs"
import { basename, dirname, join, resolve } from "node:path"
import { OX, sleep } from "./ox.mjs"
import { spec, estimatedInputBytes, freeDiskBytes } from "./out-of-core-data.mjs"

async function main() {
  const args = process.argv.slice(2)
  const opt = (key, fallback) => {
    const i = args.indexOf(key)
    if (i < 0) return fallback
    assert(args[i + 1] && !args[i + 1].startsWith("--"), `missing ${key} value`)
    return args.splice(i, 2)[1]
  }
  const directory = opt("--directory")
  const rows = Number(opt("--rows", 100003))
  const cardinality = opt("--cardinality", "low")
  const columns = Number(opt("--columns", 20))
  const valueBytes = Number(opt("--value-bytes", 32))
  const memoryMiB = Number(opt("--memory-mib", 1024))
  const codeBudgetMiB = Number(opt("--code-budget-mib", 64))
  const port = Number(opt("--port", 3056))
  const bin = resolve(opt("--bin", OX))
  const allowLarge = args.includes("--allow-large")
  if (allowLarge) args.splice(args.indexOf("--allow-large"), 1)
  assert(directory && !args.length && Number.isSafeInteger(memoryMiB) && memoryMiB >= 16 && Number.isSafeInteger(codeBudgetMiB) && codeBudgetMiB >= 0 &&
    Number.isInteger(port) && port > 0 && port < 65536,
  "usage: node benchmarks/out-of-core-cgroup.mjs --directory NEW_DIRECTORY --rows N [--bin OX] [--cardinality low|high] [--columns 20] [--value-bytes 32] [--memory-mib 1024] [--code-budget-mib 64] [--port 3056] [--allow-large]")
  const s = spec({ rows, cardinality, columns, valueBytes })
  assert(rows < 1_000_000 || allowLarge, "large execution requires explicit --allow-large")
  assert(cardinality !== "high" || rows < 1_000_000, "large high-cardinality dictionaries remain unsupported by this bounded validation")
  assert.equal(process.platform, "linux", "cgroup validation requires Linux with a writable cgroup v2 memory controller")
  const base = resolve(directory)
  assert(!existsSync(base), `refusing to reuse ${base}`)
  mkdirSync(dirname(base), { recursive: true })
  assert.equal(statfsSync(dirname(base)).type, 0xef53, "choose a Linux ext4 data directory, not /mnt/c or another host-mounted filesystem")
  const reserve = estimatedInputBytes(s) * 3 + rows * columns * 4 * 3 + 64 * 2 ** 20
  assert(freeDiskBytes(dirname(base)) > BigInt(reserve), "insufficient filesystem reserve for this dataset and spill/history files")
  mkdirSync(base)
  const groupRoot = realpathSync("/sys/fs/cgroup")
  const group = join(groupRoot, `oxrefine-ooc-${randomUUID()}`)
  mkdirSync(group)
  const file = join(base, "input.csv")
  const result = join(base, "run")
  const log = createWriteStream(join(base, "events.log"), { flags: "wx" })
  const report = { when: new Date().toISOString(), dataset: s, memoryLimitBytes: memoryMiB * 2 ** 20,
    codeLimitBytes: codeBudgetMiB * 2 ** 20, swapLimitBytes: 0, status: "running",
    scope: "producer, verifier and backend in the same private cgroup; coordinator outside; not an allocator-heap counter" }
  const emit = (v) => { const line = JSON.stringify(v); console.error(line); log.write(`${line}\n`) }
  const assertOwned = () => assert(dirname(group) === groupRoot && basename(group).startsWith("oxrefine-ooc-") && realpathSync(group) === group, "private cgroup identity changed")
  const snapshot = () => Object.fromEntries(["memory.max", "memory.swap.max", "memory.oom.group", "memory.peak", "memory.stat", "memory.events", "cgroup.procs"].map((name) =>
    [name, existsSync(join(group, name)) ? readFileSync(join(group, name), "utf8").trim() : null]))
  let aborted, currentChild
  const abort = (signal) => {
    aborted = signal
    assertOwned()
    currentChild?.kill("SIGKILL")
    // This leaf was created uniquely by this coordinator and contains only its own children.
    if (existsSync(join(group, "cgroup.kill"))) writeFileSync(join(group, "cgroup.kill"), "1")
  }
  process.once("SIGINT", abort)
  process.once("SIGTERM", abort)
  log.on("error", (e) => { report.error = `event log: ${e.message}`; abort("log-error") })
  const run = (label, argv) => new Promise((ok, fail) => {
    emit({ phase: label, state: "started" })
    // Fixed shell source, structured argv: the shell joins before exec, without rewriting arguments.
    const child = spawn("/bin/sh", ["-c", 'printf "%s" "$$" > "$1/cgroup.procs" || exit 125; shift; exec "$@"',
      "oxrefine-ooc", group, process.execPath, ...argv], { cwd: resolve(import.meta.dirname, ".."), stdio: ["ignore", "pipe", "pipe"] })
    currentChild = child
    report.childPids ??= []
    if (child.pid) report.childPids.push({ phase: label, pid: child.pid })
    child.stdout.on("data", (b) => { process.stdout.write(b); log.write(b) })
    child.stderr.on("data", (b) => { process.stderr.write(b); log.write(b) })
    let spawnError
    child.once("error", (e) => { spawnError = e })
    child.once("close", (code, signal) => {
      currentChild = undefined
      report.isolation = snapshot()
      emit({ phase: label, code, signal, isolation: report.isolation })
      if (code === 0 && !aborted && !spawnError) ok()
      else fail(spawnError ?? new Error(`${label} failed: ${aborted ?? signal ?? code}`))
    })
  })
  try {
    assertOwned()
    assert(existsSync(join(group, "memory.max")) && existsSync(join(group, "memory.peak")), "private cgroup has no delegated memory controller / peak counter")
    writeFileSync(join(group, "memory.max"), String(report.memoryLimitBytes))
    writeFileSync(join(group, "memory.swap.max"), "0")
    writeFileSync(join(group, "memory.oom.group"), "1")
    const configured = snapshot()
    assert.equal(configured["memory.max"], String(report.memoryLimitBytes), "memory.max readback differs")
    assert.equal(configured["memory.swap.max"], "0", "swap limit readback differs")
    assert.equal(configured["memory.oom.group"], "1", "OOM group readback differs")
    report.limitReadbackVerified = true
    const large = allowLarge ? ["--allow-large"] : []
    await run("generate", [join(import.meta.dirname, "out-of-core-data.mjs"), "generate", "--file", file,
      "--rows", String(rows), "--cardinality", cardinality, "--columns", String(columns), "--value-bytes", String(valueBytes), ...large])
    await run("validate", [join(import.meta.dirname, "out-of-core.mjs"), "--file", file, "--result-dir", result,
      "--bin", bin, "--port", String(port), "--code-budget-mib", String(codeBudgetMiB), ...large])
    const validation = JSON.parse(readFileSync(join(result, "result.json"), "utf8"))
    assert(validation.phases.every((p) => p.isolation.verified && p.isolation.limitBytes === report.memoryLimitBytes && p.isolation.swapLimitBytes === 0), "backend did not remain in the configured memory cgroup")
    report.backendCgroupVerified = true
    assert(!/^oom(?:_kill|_group_kill)? [1-9]/m.test(snapshot()["memory.events"]), "the cgroup recorded an OOM event")
    report.status = "passed"
  } catch (e) {
    report.status = "failed"
    report.error = e.message
    throw e
  } finally {
    assertOwned()
    report.isolation = snapshot()
    if (readFileSync(join(group, "cgroup.procs"), "utf8").trim() && existsSync(join(group, "cgroup.kill"))) writeFileSync(join(group, "cgroup.kill"), "1")
    for (let i = 0; i < 25; i++) {
      if (!readFileSync(join(group, "cgroup.procs"), "utf8").trim()) {
        try { rmdirSync(group); report.cleanup = "removed empty owned cgroup"; break } catch (e) { if (!["EBUSY", "ENOTEMPTY"].includes(e.code)) throw e }
      }
      await sleep(200)
    }
    report.cleanup ??= "owned cgroup remains; inspect cgroup.procs before cleanup"
    if (report.cleanup !== "removed empty owned cgroup") {
      report.status = "failed"
      report.error = "private cgroup cleanup did not finish"
      process.exitCode = 1
    }
    writeFileSync(join(base, "isolation.json"), JSON.stringify(report, null, 2) + "\n")
    process.removeListener("SIGINT", abort)
    process.removeListener("SIGTERM", abort)
    await new Promise((ok) => log.end(ok))
    console.log(JSON.stringify({ status: report.status, cleanup: report.cleanup, limitReadbackVerified: report.limitReadbackVerified ?? false,
      backendCgroupVerified: report.backendCgroupVerified ?? false,
      peakCgroupBytes: report.isolation["memory.peak"] === null ? null : Number(report.isolation["memory.peak"]),
      events: report.isolation["memory.events"] }, null, 2))
  }
}

main().catch((e) => { console.error(e.message); process.exitCode = 1 })
