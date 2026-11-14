import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

const tick = () => new Promise(setImmediate)
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no }); return { promise, resolve, reject } }
const code = (file) => ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX } }).outputText
const nodes = (node) => Array.isArray(node) ? node.flatMap(nodes) : node && typeof node === "object" ? [node, ...nodes(node.props?.children)] : []
const text = (node) => Array.isArray(node) ? node.map(text).join(" ") : node && typeof node === "object" ? text(node.props?.children) : typeof node === "string" || typeof node === "number" ? String(node) : ""
function harness() {
  const commands = [], timers = new Map(), helpers = {}, exports = {}, hooks = [], effects = [], notices = []
  let nextTimer = 0, cursor = 0, dirty = true, view, closed = 0, create = async () => {}, nonce = 0
  const call = (cmd, args, options) => { const request = { cmd, args, options, ...deferred() }; commands.push(request); return request.promise }
  vm.runInNewContext(code("../src/databaseImport.ts"), {
    exports: helpers, require: () => ({ call }), crypto: { randomUUID: () => `request-${++nonce}` },
    localStorage: { setItem() { throw new Error("Credentials must not be stored") } },
    setTimeout: (callback) => { const id = ++nextTimer; timers.set(id, callback); return id }, clearTimeout: (id) => timers.delete(id),
  })
  const react = {
    useState(initial) { const i = cursor++; hooks[i] ??= { value: typeof initial === "function" ? initial() : initial }; return [hooks[i].value, (value) => { hooks[i].value = typeof value === "function" ? value(hooks[i].value) : value; dirty = true }] },
    useRef(initial) { const i = cursor++; return (hooks[i] ??= { current: initial }) },
    useEffect(callback, deps) { const i = cursor++; if (!hooks[i]) { hooks[i] = { deps }; effects.push(() => { hooks[i].cleanup = callback() }) } },
  }
  vm.runInNewContext(code("../src/DatabaseImportDialog.tsx"), {
    exports,
    localStorage: { setItem() { throw new Error("Credentials must not be stored") } },
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx: (type, props) => ({ type, props }), jsxs: (type, props) => ({ type, props }) }
      if (name === "@/api") return { call }
      if (name === "@/databaseImport") return helpers
      if (name === "@/components/ui/toast") return { toast: { add: (notice) => notices.push(notice) } }
      return new Proxy({}, { get: (_, key) => String(key) })
    },
  })
  function render() { let count = 0; while (dirty) { assert(++count < 20); dirty = false; cursor = 0; view = exports.DatabaseImportDialog({ onCreate: (args) => create(args), onClose: () => { closed++ } }); effects.splice(0).forEach((effect) => effect()) } }
  render()
  const find = (predicate) => nodes(view).find(predicate)
  return { commands, helpers, notices, render,
    get view() { return view }, get closed() { return closed },
    set onCreate(callback) { create = callback },
    field(label) { return find((node) => node.props?.["aria-label"] === label) },
    input(label, value) { this.field(label).props.onChange({ target: { value } }); render() },
    click(label) { const result = find((node) => ["CellButton", "StripAction", "button"].includes(node.type) && text(node).trim() === label).props.onClick(); render(); return result },
    disabled(label) { return find((node) => ["CellButton", "StripAction"].includes(node.type) && text(node).trim() === label).props.disabled },
    async reply(command, data) { command.resolve({ data }); await tick(); render() },
    close() { find((node) => node.type === "FullView").props.onClose(); render() },
    unmount() { hooks.forEach((hook) => hook.cleanup?.()) },
    nextPoll() { const [id, callback] = timers.entries().next().value; timers.delete(id); callback() },
  }
}
const last = (h, cmd) => h.commands.filter((request) => request.cmd === cmd).at(-1)
const saved = (extra = {}) => ({ id: "saved", revision: "revision-1", name: "Warehouse", driver: "sqlite", host: "localhost", port: 5432, database: "", username: "", sqlitePath: "/data/warehouse.sqlite", tls: "verify-full", ...extra })
const catalog = (profiles = []) => ({ profiles, drivers: ["sqlite", "postgres", "mysql", "mariadb"].map((id) => ({ id, label: id, available: true })) })
const snapshot = (extra = {}) => ({ id: "snapshot-1", profileRevision: "revision-1", columns: ["Name", "Value"], columnTypes: ["TEXT", "INTEGER"], rows: [[0, ["<script>escaped text</script>", "42"], false, false, "ss"]], total: 1, expiresInSeconds: 1800, warnings: [], ...extra })
async function ready(h, data = catalog()) { await h.reply(last(h, "database_profiles"), data) }
async function connect(h) { await ready(h); h.input("Connection name", "Source"); h.input("SQLite file path", "/data/source.sqlite"); h.input("SQL query", "SELECT * FROM source") }

test("database helpers preserve opaque revisions, exclude secrets and quote schema identifiers by dialect", () => {
  const h = harness()
  const copy = h.helpers.databaseProfile({ ...saved(), password: "do-not-save", credentials: { password: "secret" } })
  assert.equal(copy.revision, "revision-1")
  assert.equal("password" in copy, false)
  assert.equal("credentials" in copy, false)
  assert.equal(h.helpers.databaseTableQuery({ schema: 'odd"schema', name: 'A.B"C' }, "postgres"), 'SELECT * FROM "odd""schema"."A.B""C"')
  assert.equal(h.helpers.databaseTableQuery({ schema: "db`name", name: "a`b" }, "mysql"), "SELECT * FROM `db``name`.`a``b`")
  assert.equal(h.helpers.newDatabaseProfile().revision, "")
  for (const value of ["", "0", "-1", "1.5", "NaN", "1000001"]) assert.ok(h.helpers.databaseQueryOptions(value, "30").error)
  assert.ok(h.helpers.databaseQueryOptions("100", "0").error)
  assert.ok(h.helpers.databaseQueryOptions("100", "121").error)
})

test("profiles create/edit/delete keep passwords out of persistence and reject stale edit errors visibly", async () => {
  const h = harness(); await ready(h, catalog([saved({ driver: "postgres", database: "analytics", username: "reader" })]))
  h.input("Saved connection", "saved")
  h.input("Database password", "private-password")
  h.input("Connection name", "Renamed warehouse")
  const save = h.click("Save profile")
  const request = last(h, "database_save_profile")
  assert.equal(request.args.params.profile.revision, "revision-1")
  assert.equal(JSON.stringify(request.args).includes("private-password"), false)
  request.reject(new Error("Saved connection changed. Reload profiles before saving.")); await save; h.render()
  assert.match(text(h.view), /changed.*Reload/)
  assert.equal(h.field("Database password").props.type, "password")
  const reload = h.click("Reload saved profile")
  last(h, "database_profiles").resolve({ data: catalog([saved({ driver: "postgres", database: "analytics", username: "reader", revision: "revision-peer" })]) }); await reload; h.render()
  assert.equal(h.field("Database password").props.value, "")
  assert.equal(h.field("Connection name").props.value, "Warehouse")
  h.input("Connection name", "Renamed warehouse")
  const retry = h.click("Save profile")
  assert.equal(last(h, "database_save_profile").args.params.profile.revision, "revision-peer")
  const revised = saved({ driver: "postgres", database: "analytics", username: "reader", name: "Renamed warehouse", revision: "revision-2" })
  last(h, "database_save_profile").resolve({ data: { profile: revised } }); await retry; h.render()
  h.click("Delete profile")
  const removal = h.click("Delete saved profile")
  assert.deepEqual(JSON.parse(JSON.stringify(last(h, "database_delete_profile").args)), { id: "saved", revision: "revision-2" })
  last(h, "database_delete_profile").resolve({ data: { deleted: "saved" } }); await removal; h.render()
  assert.equal(h.field("Saved connection").props.value, "")
  assert.equal(last(h, "database_prepare"), undefined)
  h.input("Connection name", "New SQLite")
  h.input("SQLite file path", "/data/new.sqlite")
  const creation = h.click("Save profile")
  assert.equal(last(h, "database_save_profile").args.params.profile.id, "")
  assert.equal(last(h, "database_save_profile").args.params.profile.revision, "")
  last(h, "database_save_profile").resolve({ data: { profile: saved() } }); await creation; h.render()
  assert.equal(h.field("Saved connection").props.value, "saved")
})

test("driver availability is authoritative and connection failures cannot leave a successful test badge", async () => {
  const h = harness()
  const unavailable = catalog(); unavailable.drivers[0] = { id: "sqlite", label: "SQLite", available: false, reason: "Driver not built" }
  await ready(h, unavailable)
  h.input("Connection name", "Source"); h.input("SQLite file path", "/data/file.sqlite"); h.input("SQL query", "SELECT 1")
  assert.match(text(h.view), /Driver not built/)
  assert.equal(h.disabled("Test connection"), true)
  await h.click("Load query result")
  assert.equal(last(h, "database_prepare"), undefined)
  const reload = h.click("Refresh profiles"); last(h, "database_profiles").resolve({ data: catalog() }); await reload; h.render()
  const tested = h.click("Test connection"); last(h, "database_test").resolve({ data: { ok: true } }); await tested; h.render()
  assert.match(text(h.view), /Connection test succeeded/)
  const failure = h.click("Test connection"); last(h, "database_test").reject(new Error("Connection refused")); await failure; h.render()
  assert.doesNotMatch(text(h.view), /Connection test succeeded/)
  assert.match(text(h.view), /Connection refused/)
})

test("schema browsing and query loading are explicit; creation uses only the immutable snapshot", async () => {
  const h = harness(); await connect(h); let created
  assert.deepEqual(h.commands.map((r) => r.cmd), ["database_profiles"], "typing must not execute SQL")
  const browsing = h.click("Browse schema")
  last(h, "database_schema").resolve({ data: { tables: [{ schema: "main", name: 'A"B', columns: [{ name: "value", type: "TEXT", nullable: true }] }] } }); await browsing; h.render()
  assert.match(text(h.view), /TEXT.*nullable/)
  h.click("Use SELECT for this table")
  assert.equal(h.field("SQL query").props.value, 'SELECT * FROM "main"."A""B"')
  assert.equal(last(h, "database_prepare"), undefined)
  const loading = h.click("Load query result")
  assert.equal(last(h, "database_prepare").args.params.maxRows, 100000)
  assert.equal(last(h, "database_prepare").args.params.timeoutSeconds, 30)
  last(h, "database_prepare").resolve({ data: snapshot({ total: 3, rows: [[0, ["<script>escaped text</script>", "42"], false, false], [1, [false, 0], false, false], [2, [null, ""], false, false]] }) }); await loading; h.render()
  assert.match(text(h.view), /<script>escaped text<\/script>/)
  assert.match(text(h.view), /false/)
  assert.match(text(h.view), /\(null\)/)
  assert.equal(nodes(h.view).some((node) => node.props?.dangerouslySetInnerHTML), false)
  h.input("Database project name", "Query snapshot")
  h.onCreate = async (args) => { created = args }
  await h.click("Create project")
  assert.deepEqual(JSON.parse(JSON.stringify(created)), { id: "snapshot-1", name: "Query snapshot" })
  assert.equal(h.commands.filter((r) => r.cmd === "database_prepare").length, 1, "Create must not run SQL again")
})

test("source edits and closing discard late loads/pages, while snapshot save failure preserves the view", async () => {
  const h = harness(); await connect(h)
  const staleLoad = h.click("Load query result")
  h.input("SQL query", "SELECT * FROM different")
  last(h, "database_prepare").resolve({ data: snapshot() }); await staleLoad; h.render()
  assert.equal(h.disabled("Create project"), true)
  assert.doesNotMatch(text(h.view), /escaped text/)
  const load = h.click("Load query result")
  last(h, "database_prepare").resolve({ data: snapshot({ total: 21, rows: Array.from({ length: 20 }, (_, i) => [i, [`row ${i}`, "1"], false, false]) }) }); await load; h.render()
  const page = h.click("Next")
  h.input("Maximum database rows", "100")
  last(h, "database_preview").resolve({ data: snapshot({ rows: [[20, ["STALE PAGE", "2"], false, false]] }) }); await page; h.render()
  assert.doesNotMatch(text(h.view), /STALE PAGE/)
  assert.equal(h.disabled("Create project"), true)
  const latest = h.click("Load query result")
  last(h, "database_prepare").resolve({ data: snapshot() }); await latest; h.render()
  const pending = deferred(); h.onCreate = () => pending.promise
  const saving = h.click("Create project")
  h.close(); assert.equal(h.closed, 0)
  pending.reject(new Error("Disk full")); await saving; h.render()
  assert.match(text(h.view), /Disk full/)
  assert.equal(h.disabled("Create project"), false)
  h.close(); assert.equal(h.closed, 1)
  h.unmount()
})

test("stop before capture targets only the later exact database job, never global cancellation", async () => {
  const h = harness()
  const operation = h.helpers.databaseJob("database_prepare", saved(), "secret", { sql: "SELECT 1" })
  const request = last(h, "database_prepare")
  assert.equal(request.args.params.credentials.password, "secret")
  assert.equal("password" in request.args.params.profile, false)
  operation.stop()
  last(h, "processes").resolve({ data: [{ id: 90, state: "running", stages: [{ label: "Database request another" }] }] }); await tick()
  assert.equal(last(h, "cancel_process"), undefined)
  h.nextPoll()
  last(h, "processes").resolve({ data: [{ id: 23, state: "running", stages: [{ label: `Database request ${request.args.params.requestId}` }] }] }); await tick()
  assert.equal(last(h, "cancel_process").args.id, 23)
  request.resolve({ data: snapshot() }); await operation.promise
  operation.stop()
  assert.equal(h.commands.filter((r) => r.cmd === "cancel_process").length, 1)
})

test("closing during load ignores late results and connection errors redact the current password", async () => {
  const h = harness(); await ready(h, catalog([saved({ driver: "postgres", database: "analytics", username: "reader" })]))
  h.input("Saved connection", "saved"); h.input("Database password", "never-display-me")
  const failed = h.click("Test connection")
  last(h, "database_test").reject(new Error("Driver rejected never-display-me")); await failed; h.render()
  assert.doesNotMatch(text(h.view), /never-display-me/)
  assert.match(text(h.view), /\[password\]/)
  h.input("SQL query", "SELECT 1")
  const loading = h.click("Load query result")
  h.close(); assert.equal(h.closed, 1)
  last(h, "database_prepare").resolve({ data: snapshot() }); await loading; h.render()
  assert.equal(h.disabled("Create project"), true)
  assert.match(h.notices[0].description, /No project was created/)
  h.unmount()
})
