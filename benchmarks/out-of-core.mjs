// Explicit, private HTTP probe for large-dataset validation. Generation is a separate command.
import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { randomUUID, createHash } from "node:crypto"
import { createServer } from "node:net"
import { request } from "node:http"
import { existsSync, mkdirSync, readFileSync, writeFileSync, createReadStream } from "node:fs"
import { join, resolve, basename } from "node:path"
import { pathToFileURL } from "node:url"
import { OX, kill, sleep } from "./ox.mjs"
import { spec, counts, header, verifyCsv, freeDiskBytes } from "./out-of-core-data.mjs"
import { processMemory, diskUsage, hostFacts, cgroupMemory, anonymousSpillDisk } from "./out-of-core-measure.mjs"

const transform = (columnName, expression) => ({ op: "core/text-transform", columnName, expression: `grel:${expression}`, onError: "store-error" })
async function rawFileHash(path) {
  const h = createHash("sha256")
  for await (const chunk of createReadStream(path)) h.update(chunk)
  return h.digest("hex")
}
export const OPS = [transform("value", "value.toUppercase()"), transform("qty", "value.toNumber()"),
  { op: "core/mass-edit", columnName: "group", expression: "value", edits: [
    { from: ["g0", "g1", "g2", "g3"], to: "A" }, { from: ["g4", "g5", "g6", "g7"], to: "B" },
  ] }]

async function verifyErrorTypes(probe, expected) {
  const data = await probe.call("facets", { engine: { mode: "row-based", facets: [{ type: "range", name: "qty", columnName: "qty", expression: "value",
    from: -Number.MAX_VALUE, to: Number.MAX_VALUE, selectNumeric: true, selectNonNumeric: true, selectBlank: true, selectError: true }] } })
  assert.equal(data[0].errorCount, expected.qtyErrors, "backend Error cell count differs")
  assert.equal(data[0].numericCount, expected.rows - expected.qtyErrors, "backend numeric cell count differs")
  return { errors: data[0].errorCount, numeric: data[0].numericCount }
}

/** This checks the real CodeBudget counters, never claims a bound on the whole allocator heap. */
export function checkCodeBudget(storage, manifest, limitBytes) {
  assert(storage && storage.codes, "backend row-code budget diagnostics are missing")
  assert.equal(storage.codeHeapLimitBytes, limitBytes, "backend did not apply the requested row-code budget")
  const c = storage.codes
  for (const key of ["heap_bytes", "heap_peak_bytes", "mapped_bytes", "mapped_peak_bytes", "buffer_bytes", "buffer_peak_bytes", "spill_files"]) {
    assert(Number.isSafeInteger(c[key]) && c[key] >= 0, `invalid row-code counter ${key}`)
  }
  assert(c.heap_bytes <= c.heap_peak_bytes && c.heap_peak_bytes <= limitBytes, "tracked row-code builder allocations exceeded budget")
  if (manifest.minimumCodeBytes > limitBytes) {
    assert(c.mapped_bytes > 0 && c.spill_files > 0, "dataset exceeds code budget but no live spilled arrays were reported")
  }
  return { verified: true, scope: "tracked CodeBuilder arrays only; From<Vec>, dictionaries and scratch excluded",
    limitBytes, heapPeakBytes: c.heap_peak_bytes, bufferPeakBytes: c.buffer_peak_bytes,
    logicalMappedPeakBytes: c.mapped_peak_bytes,
    codeVolumeExceedsBudget: manifest.minimumCodeBytes > limitBytes,
    inputFileExceedsBudget: manifest.input.bytes > limitBytes,
    totalHeapBounded: false }
}

/** JSON replies stay small. An accidental all-rows JSON endpoint cannot consume the dataset RAM. */
export function jsonRequest(port, token, cmd, args = {}) {
  return new Promise((ok, fail) => {
    const body = JSON.stringify(args)
    const req = request({ hostname: "127.0.0.1", port, path: `/api/${cmd}`, method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json", "Content-Length": Buffer.byteLength(body) } }, (res) => {
      let bytes = 0
      const chunks = []
      res.on("data", (b) => {
        bytes += b.length
        if (bytes > 8 * 2 ** 20) { res.destroy(new Error("benchmark JSON response exceeds 8 MiB; use a streaming export")); return }
        chunks.push(b)
      })
      res.on("error", fail)
      res.on("end", () => {
        try {
          const json = JSON.parse(Buffer.concat(chunks).toString("utf8"))
          if (res.statusCode !== 200) {
            const e = new Error(typeof json === "string" ? json : json.message ?? JSON.stringify(json))
            e.status = res.statusCode
            fail(e)
          } else ok(json.data)
        } catch (e) { fail(e) }
      })
    })
    req.on("error", fail)
    req.end(body)
  })
}

export async function startProbe({ bin, port, data, env = {} }) {
  await new Promise((ok, fail) => {
    const p = createServer()
    p.once("error", fail)
    p.listen({ host: "127.0.0.1", port, exclusive: true }, () => p.close(ok))
  })
  const token = randomUUID()
  const child = spawn(bin, ["serve", "--port", String(port), "--data", data, "--token", token],
    { env: { ...process.env, ...env }, stdio: ["ignore", "ignore", "pipe"] })
  let spawnError, stderr = ""
  child.on("error", (e) => { spawnError = e })
  child.stderr.on("data", (b) => { stderr = (stderr + b.toString()).slice(-8192) })
  const check = () => {
    if (spawnError) throw spawnError
    if (child.exitCode !== null) throw new Error(`benchmark server exited ${child.exitCode}: ${stderr.replaceAll(token, "<token>")}`)
  }
  const call = (cmd, args) => jsonRequest(port, token, cmd, args)
  const stop = async () => { kill(child); await sleep(200) }
  try {
    const until = performance.now() + 120_000
    for (;;) {
      check()
      try {
        const m = await call("memory")
        assert.equal(m.pid, child.pid, "port answered from a different process")
        break
      } catch (e) {
        check()
        if (performance.now() >= until) throw e
        await sleep(100)
      }
    }
    return { child, call, stop, check, memory: () => processMemory(child.pid) }
  } catch (e) { await stop(); throw e }
}

export async function cancelImport(probe, file, rows) {
  const attempts = []
  for (let attempt = 0; attempt < 3; attempt++) {
    const existing = new Set((await probe.call("projects")).map((p) => p.id))
    const before = new Set((await probe.call("processes")).map((j) => j.id))
    let settled = false
    const work = probe.call("open", { path: file, name: "cancelled-import" }).then(
      (value) => { settled = true; return { value } }, (error) => { settled = true; return { error } })
    const until = performance.now() + 30_000
    let cancelled
    while (!settled && performance.now() < until) {
      const running = (await probe.call("processes")).find((j) => !before.has(j.id) && j.state === "running" && j.done > 0 && (j.total === 0 || j.done < j.total))
      if (running) {
        await probe.call("cancel_process", { id: running.id })
        cancelled = { id: running.id, stage: running.label, progress: running.done, total: running.total }
        break
      }
      await sleep(1)
    }
    if (!cancelled && !settled) await probe.stop()
    const result = await work
    if (cancelled && result.error && /Cancelled/.test(result.error.message)) return { ...cancelled, error: result.error.message, priorRaces: attempts }
    if (result.error) throw new Error(`cancelled import returned an unexpected error: ${result.error.message}`)
    // A small import can finish before the cancel arrives. Remove only that known new test project,
    // record the race, and retry; successful completion never counts as cancellation coverage.
    assert(result.value?.id && !existing.has(result.value.id), "completed cancelled-import has no new owned project id")
    await probe.call("delete_project", { id: result.value.id })
    attempts.push({ outcome: "finished-before-cancel", cancelSent: !!cancelled })
  }
  assert.fail(`could not observe and cancel an in-flight ${rows}-row import after three attempts; do not report cancellation as tested`)
}

/** Uses the storage_stats contract from storage::snapshot; resident memory is collected separately. */
export async function runValidation({ file, resultDir, bin = OX, port = 3056, env = {}, codeBudgetMiB = 64, allowLarge = false }) {
  const manifest = JSON.parse(readFileSync(`${file}.json`, "utf8"))
  const s = spec(manifest)
  assert(s.rows < 1_000_000 || allowLarge, "large execution requires explicit --allow-large")
  assert(s.cardinality !== "high" || s.rows < 1_000_000, "large high-cardinality runs are disabled while dictionaries remain unbounded; code-vector spill is insufficient")
  assert(!existsSync(resultDir), `refusing to reuse result directory ${resultDir}`)
  assert(Number.isSafeInteger(codeBudgetMiB) && codeBudgetMiB >= 0, "codeBudgetMiB must be nonnegative")
  assert(manifest.input.rows === s.rows && manifest.minimumCodeBytes === s.rows * s.columns * 4, "invalid input manifest")
  assert(freeDiskBytes(resolve(file, "..")) > BigInt(manifest.input.bytes * 3 + manifest.minimumCodeBytes * 3), "insufficient disk reserve for input-sized exports and code snapshots")
  mkdirSync(resultDir, { recursive: true })
  const data = join(resultDir, "library")
  const output = join(resultDir, "output.csv")
  const codeLimitBytes = codeBudgetMiB * 2 ** 20
  const policyEnv = { ...env, OXREFINE_CODE_BUDGET_MIB: String(codeBudgetMiB), OXREFINE_SPILL_DIR: join(resultDir, "spill") }
  const report = { when: new Date().toISOString(), machine: hostFacts(), dataset: s, expectedCounts: counts(s), input: manifest.input,
    minimumCodeBytes: manifest.minimumCodeBytes, binary: { file: basename(bin), sha256: createHash("sha256").update(readFileSync(bin)).digest("hex") }, phases: [], status: "running" }
  report.policy = { codeLimitBytes, scope: "CodeBuilder allocations only; not a process or whole-heap limit" }
  const save = () => writeFileSync(join(resultDir, "result.json"), JSON.stringify(report, null, 2) + "\n")
  let probe
  try {
    console.error(JSON.stringify({ phase: "validate-input", state: "start", rows: s.rows }))
    report.inputValidation = await verifyCsv(file, s)
    console.error(JSON.stringify({ phase: "validate-input", state: "passed", rows: report.inputValidation.rows }))
    assert.equal(report.inputValidation.sha256, manifest.input.sha256, "source differs from generation manifest")
    probe = await startProbe({ bin, port, data, env: policyEnv })
    const phase = async (name, work) => {
      console.error(JSON.stringify({ phase: name, state: "start" }))
      const t = performance.now()
      const value = await work()
      const elapsedMs = performance.now() - t
      const diagnostics = await probe.call("memory")
      const storage = await probe.call("storage_stats")
      const sample = { name, elapsedMs, os: probe.memory(), isolation: cgroupMemory(probe.child.pid), backend: diagnostics, storage,
        anonymousSpillDisk: anonymousSpillDisk(probe.child.pid, policyEnv.OXREFINE_SPILL_DIR), nativeDisk: diskUsage(data),
        totalRunDisk: diskUsage(resultDir), budget: checkCodeBudget(storage, manifest, codeLimitBytes) }
      report.phases.push(sample)
      save()
      console.error(JSON.stringify({ phase: name, state: "passed", elapsedMs, residentPeakBytes: sample.os.peakResidentBytes,
        codeHeapPeakBytes: storage.codes.heap_peak_bytes, logicalMappedBytes: storage.codes.mapped_bytes }))
      return value
    }
    const opened = await phase("import", () => probe.call("open", { path: file, name: "out-of-core-validation", options: { guessCellValueTypes: false, trimStrings: false } }))
    assert.equal(opened.rows, s.rows)
    assert.equal(opened.columns.join(","), header(s))
    const id = opened.id
    assert(id)
    const nativeOriginal = join(data, "projects", id, "data.oxt")
    report.originalStoreHash = await rawFileHash(nativeOriginal)
    for (let i = 0; i < OPS.length; i++) {
      const changed = await phase(["uppercase", "stored-errors", "merge"][i], () => probe.call("apply", { op: OPS[i] }))
      assert.equal(changed.cursor, i + 1)
    }
    report.errorTypes = await phase("typed-error-count", () => verifyErrorTypes(probe, report.expectedCounts))
    await phase("export", () => probe.call("export", { path: output, format: "csv" }))
    console.error(JSON.stringify({ phase: "verify-export", state: "start" }))
    report.finalValidation = await verifyCsv(output, s, "merge")
    await phase("undo", () => probe.call("goto", { index: 0 }))
    await phase("export-undo", () => probe.call("export", { path: output, format: "csv" }))
    console.error(JSON.stringify({ phase: "verify-undo", state: "start" }))
    report.undoValidation = await verifyCsv(output, s)
    assert.equal(report.undoValidation.sha256, manifest.input.sha256)
    await phase("redo", () => probe.call("goto", { index: OPS.length }))
    report.cancel = await phase("cancel-import", () => cancelImport(probe, file, s.rows))
    const projects = await probe.call("projects")
    assert.equal(projects.length, 1, "cancelled import persisted a partial project")
    assert.equal(projects[0].id, id)
    await probe.call("open_project", { id })
    await assert.rejects(probe.call("apply", { op: transform("missing-column", "value") }), /no column/)
    assert.equal((await probe.call("summary")).cursor, OPS.length, "failed operation changed history")
    await probe.stop()
    probe = await startProbe({ bin, port, data, env: policyEnv })
    const reopened = await phase("restart-reopen", () => probe.call("open_project", { id }))
    assert.equal(reopened.rows, s.rows)
    assert.equal(reopened.cursor, OPS.length)
    assert.equal(reopened.history.length, OPS.length)
    report.reopenedErrorTypes = await phase("reopened-typed-error-count", () => verifyErrorTypes(probe, report.expectedCounts))
    await phase("export-reopened", () => probe.call("export", { path: output, format: "csv" }))
    console.error(JSON.stringify({ phase: "verify-reopened-and-original-hashes", state: "start" }))
    report.reopenedValidation = await verifyCsv(output, s, "merge")
    assert.equal(report.reopenedValidation.sha256, report.finalValidation.sha256)
    report.originalStoreHashAfter = await rawFileHash(nativeOriginal)
    assert.equal(report.originalStoreHashAfter, report.originalStoreHash, "native original table was changed by editing or reopen")
    report.inputHashAfter = await rawFileHash(file)
    assert.equal(report.inputHashAfter, manifest.input.sha256, "source CSV was modified")
    await probe.stop()
    probe = undefined
    report.afterStopDisk = diskUsage(resultDir)
    report.afterStopSpillDirectory = diskUsage(policyEnv.OXREFINE_SPILL_DIR)
    assert.equal(report.afterStopSpillDirectory.files, 0, "private spill files leaked after backend exit")
    report.status = "functional-passed"
    report.trackedCodeBudgetVerified = report.phases.every((p) => p.budget.verified)
    report.totalHeapBudgetVerified = false
    report.totalHeapBudgetExplanation = "The code-vector builder budget excludes resident dictionaries, other code constructors and scratch. OS resident memory is not heap; this run does not itself install an OS memory constraint."
    report.smallerThanDatasetCgroup = report.phases.every((p) => p.isolation.verified && p.isolation.limitBytes < manifest.minimumCodeBytes &&
      p.isolation.limitBytes < manifest.input.bytes && p.isolation.swapLimitBytes === 0)
    report.observedResidentBelowCgroupCap = report.smallerThanDatasetCgroup && report.phases.every((p) => p.os.peakResidentBytes <= p.isolation.limitBytes)
    report.observedPeakResidentBytes = Math.max(...report.phases.map((p) => p.os.peakResidentBytes))
    save()
    return report
  } catch (e) {
    report.status = "failed"
    report.error = e.message
    save()
    throw e
  } finally { if (probe) await probe.stop() }
}

async function main() {
  const args = process.argv.slice(2)
  const opt = (key, fallback) => {
    const i = args.indexOf(key)
    if (i < 0) return fallback
    assert(args[i + 1] && !args[i + 1].startsWith("--"), `missing ${key} value`)
    return args.splice(i, 2)[1]
  }
  const file = opt("--file")
  const resultDir = opt("--result-dir")
  const bin = opt("--bin", OX)
  const port = Number(opt("--port", 3056))
  const codeBudgetMiB = Number(opt("--code-budget-mib", 64))
  const allowLarge = args.includes("--allow-large")
  if (allowLarge) args.splice(args.indexOf("--allow-large"), 1)
  assert(!args.length && file && resultDir && Number.isInteger(port) && port > 0 && port < 65536,
    "usage: node benchmarks/out-of-core.mjs --file INPUT.csv --result-dir NEW_DIRECTORY [--bin OX] [--port 3056] [--code-budget-mib 64] [--allow-large]")
  const report = await runValidation({ file: resolve(file), resultDir: resolve(resultDir), bin: resolve(bin), port, codeBudgetMiB, allowLarge })
  console.log(JSON.stringify({ status: report.status, rows: report.dataset.rows, trackedCodeBudgetVerified: report.trackedCodeBudgetVerified,
    totalHeapBudgetVerified: report.totalHeapBudgetVerified, smallerThanDatasetCgroup: report.smallerThanDatasetCgroup }, null, 2))
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  main().catch((e) => { console.error(e.message); process.exitCode = 1 })
}
