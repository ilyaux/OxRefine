// Opt-in, 100k real SQL rows per backend. Reads credentials privately; reports never contain them.
import assert from "node:assert/strict"
import { readFileSync, writeFileSync, mkdirSync } from "node:fs"
import { resolve, join, dirname, basename } from "node:path"
import { pathToFileURL } from "node:url"
import { createHash, randomUUID } from "node:crypto"
import { cpus, totalmem, platform, release } from "node:os"
import { withOx, OX, DATA, HTTP_CONNECTION, noFacets } from "./ox.mjs"
import { csvDigest } from "./cardinality.mjs"

export const ROWS = 100_000
export const SQL = "SELECT id,label,quantity FROM db_bench_fixture ORDER BY id"
const usage = "node benchmarks/database.mjs --config <private-connections.json> --sqlite <fixture.sqlite> [--new <ox binary>] [--runs 3] [--out <report.json>]"
const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)]
const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex")

export function expectedRow(i) { return [String(i), `item-${i}`, i % 97 === 0 ? null : String(i % 1000)] }
export function expectedDigest() {
  const hash = createHash("sha256").update("id,label,quantity\n")
  for (let i = 0; i < ROWS; i++) hash.update(`${expectedRow(i).join(",")}\n`)
  return { sha256: hash.digest("hex"), lines: ROWS + 1 }
}
export function publicProfile(connection) {
  const profile = { name: `100k ${connection.driver} benchmark`, id: `benchmark-${connection.driver}`, revision: "" }
  for (const key of ["driver", "host", "port", "database", "username", "tls", "sqlitePath"]) if (connection[key] !== undefined) profile[key] = connection[key]
  return profile
}

async function run(connection, out, options) {
  return withOx(async ({ ox, peak, pid, restart }) => {
    const profile = publicProfile(connection), credentials = { password: connection.password ?? "" }
    const prepare = (sql, maxRows) => ox("database_prepare", { params: { profile, credentials, sql, maxRows, timeoutSeconds: 120, requestId: randomUUID() } })
    const version = await prepare(connection.driver === "sqlite" ? "SELECT sqlite_version() AS version" : "SELECT version() AS version", 1)
    const backendVersion = String(version.rows[0][1][0]).slice(0, 200)
    const result = { driver: connection.driver, backendVersion, rows: ROWS, idleMiB: peak(), httpConnection: HTTP_CONNECTION,
      reopenProtocol: "fresh-backend-process; startup excluded; filesystem cache not flushed", phasePeakMiB: {}, phasePid: {} }
    result.processLifetimes = [{ pid: pid(), role: "importing", idleMiB: result.idleMiB }]
    const time = async (name, work) => {
      const t = performance.now()
      const value = await work()
      result[`${name}Ms`] = performance.now() - t
      result.phasePeakMiB[name] = peak()
      result.phasePid[name] = pid()
      return value
    }
    const prepared = await time("prepare", () => prepare(SQL, ROWS))
    assert.equal(prepared.total, ROWS, "backend fixture row count differs")
    const summary = await time("create", () => ox("database_create", { id: prepared.id, name: "Database benchmark" }))
    assert.equal(summary.rows, ROWS)
    result.importMs = result.prepareMs + result.createMs
    result.importPeakMiB = Math.max(result.phasePeakMiB.prepare, result.phasePeakMiB.create, peak())
    result.phasePeakMiB.import = result.importPeakMiB
    result.phasePid.import = pid()
    result.restart = await restart()
    assert.notEqual(result.restart.pid, result.restart.previousPid, "reopen needs a different backend process")
    result.processLifetimes[0].peakMiB = Math.max(result.idleMiB, result.importPeakMiB, result.restart.previousPeakMB)
    result.processLifetimes.push({ pid: result.restart.pid, role: "reopened", idleMiB: result.restart.idlePeakMB })
    const reopened = await time("reopen", () => ox("open_project", { id: summary.id }))
    assert.equal(reopened.rows, ROWS)
    await time("export", () => ox("export", { path: out, engine: noFacets }))
    const digest = await csvDigest(out)
    assert.deepEqual(digest, expectedDigest(), "database output differs from the independent fixture")
    // Full type/null validation is outside phase timing, not a three-row preview check.
    for (let start = 0; start < ROWS; start += 1000) {
      const page = await ox("rows", { start, limit: 1000 })
      assert.equal(page.rows.length, Math.min(1000, ROWS - start))
      for (let j = 0; j < page.rows.length; j++) {
        const i = start + j, row = page.rows[j]
        assert.deepEqual(row[1], expectedRow(i), `row ${i} values`)
        assert.equal(row[4], i % 97 === 0 ? "nss" : "nsn", `row ${i} types`)
      }
    }
    result.phasePeakMiB.validation = peak()
    result.phasePid.validation = pid()
    result.processLifetimes[1].peakMiB = Math.max(result.restart.idlePeakMB, result.phasePeakMiB.reopen, result.phasePeakMiB.export, result.phasePeakMiB.validation)
    result.workflowPeakMiB = Math.max(...result.processLifetimes.map((p) => p.peakMiB))
    result.output = digest
    result.typedRowsVerified = ROWS
    return result
  }, options)
}

export async function main(args) {
  const config = { new: OX, runs: 3, out: "benchmarks/release-0.6.0/database.json" }
  for (let i = 0; i < args.length; i += 2) {
    if (!["--config", "--sqlite", "--new", "--runs", "--out"].includes(args[i]) || !args[i + 1]) throw new Error(usage)
    config[args[i].slice(2)] = args[i + 1]
  }
  config.runs = Number(config.runs)
  if (!config.config || !config.sqlite || !Number.isSafeInteger(config.runs) || config.runs < 1) throw new Error(usage)
  if (process.env.OXREFINE_CODE_BUDGET_MIB) throw new Error("Clear OXREFINE_CODE_BUDGET_MIB for this default-storage database benchmark")
  const connections = JSON.parse(readFileSync(config.config, "utf8")).connections
  assert(Array.isArray(connections))
  const selected = ["postgres", "mysql", "mariadb"].map((driver) => {
    const connection = connections.find((c) => c.driver === driver)
    assert(connection, `missing ${driver} fixture connection`)
    return connection
  })
  selected.push({ driver: "sqlite", sqlitePath: resolve(config.sqlite), tls: "disable" })
  const binaryHash = sha256(config.new), output = resolve(config.out)
  mkdirSync(DATA, { recursive: true }); mkdirSync(dirname(output), { recursive: true })
  const report = { when: new Date().toISOString(), workload: "database-100k-v2", rows: ROWS, query: SQL, runs: config.runs, httpConnection: HTTP_CONNECTION,
    fixture: "id=0..99999; label=item-id; quantity=NULL when id%97=0, otherwise id%1000", expected: expectedDigest(),
    statistic: "median of raw repetitions; import=SQL prepare plus native project creation/save",
    memory: `${process.platform === "darwin" ? "Sampled backend resident memory (transient peaks may be missed)" : "Per-phase backend process OS high-water mark"} in MiB; workflow peak=max of both backend lifetimes, including full validation; database servers, parent Node and external filesystem cache excluded`,
    cache: "fresh native library per run and a true backend process restart before timed open_project; stop/start/readiness excluded from reopenMs; database services and OS filesystem caches remain warm",
    machine: { platform: platform(), release: release(), cpu: cpus()[0]?.model, cpus: cpus().length, totalMemoryMiB: Math.round(totalmem() / 2 ** 20) },
    binary: { file: basename(config.new), sha256: binaryHash }, complete: false, results: [] }
  for (let repeat = 0; repeat < config.runs; repeat++) {
    for (let k = 0; k < selected.length; k++) {
      const connection = selected[(repeat + k) % selected.length]
      console.log(`${connection.driver}: ${ROWS} rows, repetition ${repeat + 1}/${config.runs}`)
      // Configuration, credentials and connection errors are intentionally not copied to the report.
      const result = await run(connection, join(DATA, `database-${connection.driver}-${repeat}.csv`), { bin: config.new, port: 3058 })
      let entry = report.results.find((r) => r.driver === connection.driver)
      if (!entry) { entry = { driver: connection.driver, runs: [] }; report.results.push(entry) }
      entry.runs.push(result)
      entry.median = Object.fromEntries(["prepareMs", "createMs", "importMs", "reopenMs", "exportMs", "importPeakMiB", "workflowPeakMiB"].map((key) => [key, median(entry.runs.map((r) => r[key]))]))
      const sameBinary = sha256(config.new) === binaryHash
      report.complete = report.results.reduce((n, r) => n + r.runs.length, 0) === config.runs * selected.length
      report.valid = report.complete && sameBinary
      writeFileSync(output, JSON.stringify(report, null, 2) + "\n")
      assert(sameBinary, "benchmark binary changed during measurement")
    }
  }
  console.log(output)
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.includes("--help")) console.log(usage)
  else await main(process.argv.slice(2)).catch(() => { console.error("Database benchmark failed; check the synthetic fixture services and local configuration. No credentials were written to a report."); process.exitCode = 1 })
}
