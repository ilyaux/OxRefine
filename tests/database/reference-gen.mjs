// Generate references through the real OpenRefine 3.10.1 database extension, not a JDBC imitation.
// Credentials are read only from the explicitly supplied, ignored local test configuration.
import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { readFileSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { createServer } from "node:net"
import { OR_HOME, startOpenRefine } from "../../tools/openrefine.mjs"

const artifacts = {
  "webapp/WEB-INF/lib/openrefine-main.jar": "b1bee728e85b4aa44967427707ecdb501a6b4aa3d87608ba7b29115c858f0957",
  "webapp/extensions/database/module/MOD-INF/lib/openrefine-database.jar": "ff9b36b43e14d5fb842003866d042286298b621a9fc7a01444e1ae5997a80c67",
  "webapp/extensions/database/module/MOD-INF/lib/postgresql-42.7.8.jar": "2a32a9dcbc42d67a50ad3a0de5efd102c8d2be46720045f2cbd6689f160ab7c7",
  "webapp/extensions/database/module/MOD-INF/lib/mariadb-java-client-3.5.6.jar": "a129703efd7b0f334564d46753de999f09b3a361489a2eb647e6020390981cc9",
  "webapp/extensions/database/module/MOD-INF/lib/mysql-connector-j-8.2.0.jar": "06f14fbd664d0e382347489e66495ca27ab7e6c2e1d9969a496931736197465f",
}
for (const [path, expected] of Object.entries(artifacts)) {
  assert.equal(createHash("sha256").update(readFileSync(join(OR_HOME, path))).digest("hex"), expected, `Pinned reference artifact mismatch: ${path}`)
}

const path = process.env.OXREFINE_DATABASE_TEST_CONFIG ?? resolve("target/database-live/connections.json")
const cfg = JSON.parse(readFileSync(path, "utf8"))
const connections = cfg.connections ?? cfg
const engines = { postgres: "PostgreSQL", mysql: "MySQL", mariadb: "MariaDB" }
const configured = connections.filter((c) => c.driver in engines)
assert.ok(configured.length, "No synthetic database fixture connections configured")
const port = 3340
await new Promise((ok, no) => {
  const check = createServer()
  check.once("error", no)
  check.listen(port, "127.0.0.1", () => check.close(ok))
})

let or
try {
  or = await startOpenRefine({
    port, mem: "512M", workspace: resolve("target/database-live/openrefine-reference-workspace"),
    jvm: ["-Duser.timezone=UTC"],
  })
  const csrf = (await or.get("/command/core/get-csrf-token")).token
  const reportedVersion = await or.get("/command/core/get-version")
  async function post(command, fields) {
    const form = new URLSearchParams()
    for (const [k, v] of Object.entries(fields)) form.set(k, typeof v === "string" ? v : JSON.stringify(v))
    const r = await fetch(`${or.url}/command/${command}${command.includes("?") ? "&" : "?"}csrf_token=${encodeURIComponent(csrf)}`, { method: "POST", body: form })
    if (!r.ok) throw new Error(`Reference command HTTP${r.status}; sensitive response omitted`)
    return r.json()
  }
  async function queryTable(connection, query, blankNulls = true, duplicateError = false) {
    const info = {
      connectionName: "synthetic fixture", databaseType: connection.driver === "postgres" ? "postgresql" : connection.driver,
      databaseServer: connection.host, databasePort: String(connection.port), databaseUser: connection.username,
      databasePassword: connection.password, initialDatabase: connection.database, initialSchema: "public", query,
    }
    const tested = await post("database/test-query", info)
    assert.equal(tested.code, "ok", `${connection.driver} reference SELECT failed; response omitted`)
    const { jobID } = await post("core/create-importing-job", {})
    const ctl = (sub) => `core/importing-controller?controller=database%2Fdatabase-import-controller&jobID=${jobID}&subCommand=${sub}`
    const init = await post(ctl("initialize-parser-ui"), info)
    assert.equal(init.status, "ok", "Reference parser initialization failed")
    const options = {
      ...init.options, projectName: "synthetic database types", skipDataLines: 0, limit: -1,
      storeBlankRows: true, storeBlankCellsAsNulls: blankNulls, disableAutoPreview: false,
    }
    const created = await post(ctl("create-project"), { ...info, options: JSON.stringify(options) })
    assert.notEqual(created.status, "error", "Reference project creation failed")
    let config
    for (let i = 0; i < 300; i++) {
      config = (await post(`core/get-importing-job-status?jobID=${jobID}`, {})).job?.config ?? {}
      if (config.projectID) break
      if (config.state === "error") {
        if (duplicateError && config.errors?.some((e) => e.message?.includes("Duplicate column name 'label'")))
          return { error: "Duplicate column name 'label'" }
        writeFileSync(resolve("target/database-live/reference-error-private.json"), JSON.stringify(config))
        throw new Error("Reference import failed; sensitive response retained only in ignored local diagnostics")
      }
      await new Promise((r) => setTimeout(r, 100))
    }
    if (!config.projectID) throw new Error("Reference import timed out")
    try {
      const columns = (await or.get(`/command/core/get-models?project=${config.projectID}`)).columnModel.columns
      const raw = await (await fetch(`${or.url}/command/core/get-rows?project=${config.projectID}&start=0&limit=1000`)).text()
      // Preserve int64 JSON literals rather than rounding them through JavaScript Number.
      const decoded = JSON.parse(raw, (_key, value, context) =>
        typeof value === "number" && Number.isInteger(value) && !Number.isSafeInteger(value) && /^-?\d+$/.test(context?.source ?? "")
          ? JSON.rawJSON(context.source) : value)
      return {
        columns: columns.map((c) => c.name),
        rows: decoded.rows.map((r) => ({ starred: r.starred, flagged: r.flagged, cells: columns.map((c) => r.cells[c.cellIndex] ?? null) })),
      }
    } finally {
      await or.command("delete-project", { project: config.projectID })
    }
  }
  for (const connection of configured) {
    const version = await queryTable(connection, "SELECT version() AS engine_version")
    const query = "SELECT * FROM typed_fixture ORDER BY id"
    const table = await queryTable(connection, query)
    assert.equal(JSON.stringify(table.rows[0].cells[2]), '{"v":9223372036854775807}')
    assert.equal(JSON.stringify(table.rows[1].cells[2]), '{"v":-9223372036854775808}')
    const out = {
      reference: "OpenRefine 3.10.1 bundled database extension", engine: engines[connection.driver],
      serverVersion: version.rows[0].cells[0].v, timeZone: "UTC",
      provenance: { release: "3.10.1", reportedVersion, artifacts, generator: "tests/database/reference-gen.mjs", bootstrap: "Unchanged bundled database extension and its dynamic JDBC drivers; UTC JVM" },
      source: "Locally generated synthetic values; no personal data", query,
      options: { storeBlankRows: true, storeBlankCellsAsNulls: true }, ...table,
      duplicateHeaders: await queryTable(connection, "SELECT id AS label, intv AS label FROM typed_fixture ORDER BY id", true, true),
      emptyStrings: await queryTable(connection, "SELECT emptyv, nullable FROM typed_fixture ORDER BY id", false),
    }
    writeFileSync(resolve(`tests/database/reference-${connection.driver}.json`), JSON.stringify(out, null, 2) + "\n")
    console.log(`${connection.driver}: actual OpenRefine reference written (${table.columns.length} columns, ${table.rows.length} rows)`)
  }
} finally {
  or?.stop()
}
