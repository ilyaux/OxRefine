// Real bundled JDBC extension vs OxRefine, using the same synthetic SQL fixture.
// Credentials stay in the opt-in private configuration, never in the report.
import assert from "node:assert/strict"
import { readFileSync, writeFileSync, mkdtempSync, rmSync, mkdirSync, openSync, closeSync } from "node:fs"
import { join, resolve, dirname, basename } from "node:path"
import { tmpdir, cpus, totalmem, release } from "node:os"
import { createHash } from "node:crypto"
import { pathToFileURL } from "node:url"
import { startOpenRefine, OR_HOME } from "../tools/openrefine.mjs"
import { peakMB, HTTP_CONNECTION, sleep, noFacets } from "./ox.mjs"
import { run as runOx, ROWS, SQL, expectedRow, expectedDigest } from "./database.mjs"
import { csvDigest } from "./cardinality.mjs"

const sha = (path) => createHash("sha256").update(readFileSync(path)).digest("hex")
const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)]
const headers = { Connection: HTTP_CONNECTION }

export async function runReference(connection, workspace, out) {
  // Do not allow accidentally measuring an existing user's server.
  const { createServer } = await import("node:net")
  await new Promise((ok, fail) => {
    const probe = createServer().once("error", fail)
    probe.listen(3348, "127.0.0.1", () => probe.close(ok))
  })
  mkdirSync(workspace, { recursive: true })
  const logPath = join(workspace, "reference-private.log"), log = openSync(logPath, "w")
  const or = await startOpenRefine({ port: 3348, mem: "1400M", workspace, jvm: ["-Duser.timezone=UTC"], stdio: ["ignore", log, log] })
  try {
    const get = async (path) => (await fetch(`${or.url}/command/core/${path}`, { headers })).json()
    const csrf = (await get("get-csrf-token")).token
    const version = await get("get-version")
    // This pinned 3.10.1 reference build reports the unreplaced Maven placeholder.
    assert(["3.10.1", "3.10-SNAPSHOT"].includes(version.version))
    const post = async (path, fields = {}, json = true) => {
      const body = new URLSearchParams()
      for (const [k, v] of Object.entries(fields)) body.set(k, typeof v === "string" ? v : JSON.stringify(v))
      const res = await fetch(`${or.url}/command/${path}${path.includes("?") ? "&" : "?"}csrf_token=${encodeURIComponent(csrf)}`, { method: "POST", body, headers })
      if (!res.ok && connection.driver === "sqlite" && readFileSync(logPath, "utf8").includes("No suitable driver found for jdbc:sqlite:")) {
        const error = new Error("Bundled SQLite JDBC registration failed: No suitable driver")
        error.code = "SQLITE_REFERENCE_DRIVER_UNAVAILABLE"
        throw error
      }
      assert(res.ok, `OpenRefine HTTP ${res.status}; sensitive response omitted`)
      return json ? res.json() : res.text()
    }
    const info = {
      connectionName: "synthetic benchmark", databaseType: connection.driver === "postgres" ? "postgresql" : connection.driver,
      databaseServer: connection.host ?? "na", databasePort: String(connection.port ?? 0), databaseUser: connection.username ?? "na",
      databasePassword: connection.password ?? "", initialDatabase: connection.sqlitePath?.replaceAll("\\", "/") ?? connection.database, initialSchema: "public", query: SQL,
    }
    // Warm driver loading / connection establishment using one row, as in the Ox runner.
    const warm = await post("database/test-query", { ...info, query: connection.driver === "sqlite" ? "SELECT sqlite_version() AS version" : "SELECT version() AS version" })
    assert.equal(warm.code, "ok", "Reference database warm-up failed; sensitive response omitted")
    const result = { status: "completed", rows: ROWS, reportedVersion: version, idleMiB: peakMB(or.pid), pid: or.pid }
    const t = performance.now()
    const { jobID } = await post("core/create-importing-job")
    const ctl = (sub) => `core/importing-controller?controller=database%2Fdatabase-import-controller&jobID=${jobID}&subCommand=${sub}`
    const init = await post(ctl("initialize-parser-ui"), info)
    assert.equal(init.status, "ok")
    const options = { ...init.options, projectName: "Database benchmark", skipDataLines: 0, limit: -1, storeBlankRows: true, storeBlankCellsAsNulls: true }
    const created = await post(ctl("create-project"), { ...info, options })
    assert.notEqual(created.status, "error")
    let project
    while (performance.now() - t < 120_000) {
      const cfg = (await post(`core/get-importing-job-status?jobID=${jobID}`)).job?.config
      assert(cfg?.state !== "error", "Reference import failed; sensitive response omitted")
      if (cfg?.projectID) { project = cfg.projectID; break }
      await sleep(5)
    }
    assert(project, "Reference import did not finish")
    result.importMs = performance.now() - t
    result.importPeakMiB = peakMB(or.pid)
    // Check every value and its JSON type, including all 1,031 SQL NULLs.
    const columns = (await get(`get-models?project=${project}`)).columnModel.columns
    assert.deepEqual(columns.map((c) => c.name), ["id", "label", "quantity"])
    for (let start = 0; start < ROWS; start += 1000) {
      const page = await get(`get-rows?project=${project}&start=${start}&limit=1000`)
      assert.equal(page.total, ROWS)
      assert.equal(page.rows.length, 1000)
      for (let j = 0; j < page.rows.length; j++) {
        const i = start + j, cells = columns.map((c) => page.rows[j].cells[c.cellIndex] ?? null)
        const values = cells.map((c) => c == null ? null : String(c.v))
        assert.deepEqual(values, expectedRow(i), `reference row ${i} values`)
        assert.equal(typeof cells[0]?.v, "number")
        assert.equal(typeof cells[1]?.v, "string")
        if (i % 97 === 0) assert.equal(cells[2], null)
        else assert.equal(typeof cells[2]?.v, "number")
      }
    }
    const csv = await post("core/export-rows/database.csv", { project: String(project), format: "csv", engine: noFacets }, false)
    writeFileSync(out, csv)
    result.output = await csvDigest(out)
    assert.deepEqual(result.output, expectedDigest())
    result.typedRowsVerified = ROWS
    result.workflowPeakMiB = peakMB(or.pid)
    return result
  } finally {
    or.stop()
    if (or.proc.exitCode === null) await new Promise((ok) => or.proc.once("close", ok))
    closeSync(log)
  }
}

export async function main(args) {
  const config = { runs: 3, out: "benchmarks/release-0.6.0/database-comparison.json" }
  for (let i = 0; i < args.length; i += 2) {
    assert(["--config", "--sqlite", "--new", "--runs", "--out"].includes(args[i]) && args[i + 1], "Invalid benchmark arguments")
    config[args[i].slice(2)] = args[i + 1]
  }
  assert(process.platform === "win32", "This report uses Windows PeakWorkingSet64")
  assert(config.config && config.sqlite && config.new)
  config.runs = Number(config.runs)
  assert(Number.isSafeInteger(config.runs) && config.runs >= 3 && config.runs % 2 === 1)
  assert(!process.env.OXREFINE_CODE_BUDGET_MIB)
  const connections = JSON.parse(readFileSync(config.config, "utf8")).connections
  const selected = ["postgres", "mysql", "mariadb"].map((d) => connections.find((c) => c.driver === d))
  assert(selected.every(Boolean))
  selected.push({ driver: "sqlite", sqlitePath: resolve(config.sqlite), tls: "disable" })
  const jdbcNames = ["postgresql-42.7.8.jar", "mysql-connector-j-8.2.0.jar", "mariadb-java-client-3.5.6.jar", "sqlite-jdbc-3.51.1.0.jar"]
  const artifacts = [config.new, join(OR_HOME, "webapp/WEB-INF/lib/openrefine-main.jar"), join(OR_HOME, "webapp/extensions/database/module/MOD-INF/lib/openrefine-database.jar"),
    ...jdbcNames.map((name) => join(OR_HOME, "webapp/extensions/database/module/MOD-INF/lib", name))]
  const hashes = artifacts.map(sha)
  assert.equal(hashes[1], "b1bee728e85b4aa44967427707ecdb501a6b4aa3d87608ba7b29115c858f0957", "OpenRefine differs from the pinned reference")
  assert.equal(hashes[2], "ff9b36b43e14d5fb842003866d042286298b621a9fc7a01444e1ae5997a80c67", "Database extension differs from the pinned reference")
  const report = {
    when: new Date().toISOString(), workload: "paired-database-100k-v1", localMeasurement: true,
    openrefine: "3.10.1", oxrefine: "0.6.0", orMemory: "1400M", rows: ROWS, columns: 3, nulls: 1031, query: SQL, runs: config.runs,
    expected: expectedDigest(), statistic: "median of raw repetitions; ratios are ratios of medians", httpConnection: HTTP_CONNECTION,
    protocol: "Fresh process and private project library per run; alternate OxRefine/OpenRefine order; rotate backend order. Startup, warm one-row version query, export and full validation excluded from import. No optional OpenRefine preview or full-query test. Poll project readiness every 5 ms.",
    persistence: "OxRefine import includes SQL fetch, typed snapshot and project save. OpenRefine import ends when its native database controller registers the project in memory; no autosave, export or archive creation is charged to import. These are native import paths, not identical durability guarantees.",
    memoryMetric: "PeakWorkingSet64", memoryUnit: "MiB", memoryScope: "Backend process OS high-water mark at import completion, including startup/runtime overhead; excludes database servers, browser, parent Node and external filesystem cache. JVM -Xms256M -Xmx1400M; heap configuration is not a process RAM limit.",
    cache: "Database services and OS filesystem cache remain warm; no cache flush; no controlled isolated-host claim.",
    machine: { platform: process.platform, release: release(), cpu: cpus()[0].model.trim(), cpus: cpus().length, totalMemoryMiB: Math.round(totalmem() / 2 ** 20) },
    oxBinary: { file: basename(config.new), sha256: hashes[0] }, openrefineMainSha256: hashes[1], openrefineDatabaseSha256: hashes[2],
    jdbcSha256: Object.fromEntries(jdbcNames.map((name, i) => [name, hashes[i + 3]])),
    referenceBuild: "Same pinned 3.10.1 reference artifacts as the conformance fixtures and existing release comparisons; this build reports 3.10-SNAPSHOT [TRUNK]. Exact API version and JVM details are retained per completed run.",
    generatorSha256: sha(new URL(import.meta.url)), oxRunnerSha256: sha(new URL("./database.mjs", import.meta.url)),
    complete: false, valid: false, integrityValid: false, results: [],
  }
  const lab = mkdtempSync(join(tmpdir(), "ox-db-paired-"))
  mkdirSync(dirname(resolve(config.out)), { recursive: true })
  try {
    for (let repeat = 0; repeat < config.runs; repeat++) for (let k = 0; k < selected.length; k++) {
      const connection = selected[(repeat + k) % selected.length]
      const order = repeat % 2 ? ["or", "ox"] : ["ox", "or"]
      const pair = { repeat: repeat + 1, order }
      for (const engine of order) {
        console.log(`${connection.driver} ${repeat + 1}/${config.runs}: ${engine}`)
        const out = join(lab, `${connection.driver}-${repeat}-${engine}.csv`)
        if (engine === "ox") pair.ox = { status: "completed", ...await runOx(connection, out, { bin: config.new, port: 3058 }) }
        else try { pair.or = await runReference(connection, join(lab, `or-${connection.driver}-${repeat}`), out) }
        catch (e) {
          if (e.code !== "SQLITE_REFERENCE_DRIVER_UNAVAILABLE") throw e
          pair.or = { status: "unavailable", reason: e.message, attempted: true }
        }
      }
      if (pair.or.status === "completed") assert.deepEqual(pair.ox.output, pair.or.output)
      let entry = report.results.find((r) => r.driver === connection.driver)
      if (!entry) { entry = { driver: connection.driver, backendVersion: pair.ox.backendVersion, runs: [] }; report.results.push(entry) }
      entry.runs.push(pair)
      for (const engine of ["ox", "or"]) {
        if (entry.runs.every((r) => r[engine].status === "unavailable")) entry[engine] = { status: "unavailable", reason: pair[engine].reason }
        else {
          assert(entry.runs.every((r) => r[engine].status === "completed"), "Inconsistent reference completion")
          entry[engine] = { status: "completed", ...Object.fromEntries(["importMs", "importPeakMiB"].map((key) => [key, median(entry.runs.map((r) => r[engine][key]))])) }
        }
      }
      report.integrityValid = artifacts.every((p, i) => sha(p) === hashes[i])
      assert(report.integrityValid, "Measured artifact changed")
      report.complete = report.results.length === 4 && report.results.every((r) => r.runs.length === config.runs)
      report.valid = report.complete && report.integrityValid
      writeFileSync(config.out, JSON.stringify(report, null, 2) + "\n")
    }
  } finally { rmSync(lab, { recursive: true, force: true }) }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  await main(process.argv.slice(2)).catch((e) => { console.error(`Database comparison failed: ${e instanceof assert.AssertionError ? e.message : e.name}; sensitive details omitted.`); process.exitCode = 1 })
}
