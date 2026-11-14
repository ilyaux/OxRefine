import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

let nextPage = 0
function api(storage = new Map()) {
  const requests = [], exports = {}
  const code = ts.transpileModule(readFileSync(new URL("../src/api.ts", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023 },
  }).outputText
  vm.runInNewContext(code, {
    exports, require: () => ({}), window: {}, location: { search: "" }, URLSearchParams,
    crypto: { randomUUID: () => `page-${++nextPage}` },
    sessionStorage: { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) },
    fetch: (url, options) => new Promise((resolve) => requests.push({ url, options,
      reply: (data, status = 200) => resolve({ ok: status === 200, status, json: async () => data }),
    })),
  })
  return { ...exports, requests, storage }
}
const context = (project, revision) => ({ project, revision })
const response = (project, revision, data = {}) => ({ data, ms: 0, context: context(project, revision) })
async function select(a, project, revision = "epoch-0") {
  const request = a.call("open_project", { id: project })
  a.requests.at(-1).reply(response(project, revision))
  await request
}

test("duplicated storage still gets independent page ownership and explicit project selection", async () => {
  const first = api()
  await select(first, "123")
  const second = api(new Map(first.storage))
  await select(second, "123")
  assert.notEqual(first.requests[0].options.headers["X-Ox-Session"], second.requests[0].options.headers["X-Ox-Session"])
  await select(first, "456")
  const read = second.call("rows", { limit: 10 })
  assert.equal(second.requests.at(-1).options.headers["X-Ox-Project"], "123")
  assert.equal(second.requests.at(-1).options.headers["X-Ox-Revision"], "epoch-0")
  second.requests.at(-1).reply(response("123", "epoch-0", [])); await read
})

test("background library reads cannot bless a stale editor and conflicts never retry a mutation", async () => {
  const a = api(); await select(a, "123")
  const library = a.call("projects")
  a.requests.at(-1).reply(response("123", "epoch-1", [{ id: "123", revision: "epoch-1" }, { id: "456", revision: "epoch-7" }]))
  await library
  const messages = []; a.onProjectConflict((message) => messages.push(message))
  const write = a.call("apply", { op: {} })
  const count = a.requests.length
  assert.equal(a.requests.at(-1).options.headers["X-Ox-Revision"], "epoch-0")
  a.requests.at(-1).reply({ code: "stale_project", message: "Refresh before editing", context: context("123", "epoch-1") }, 409)
  await assert.rejects(write, { name: "ProjectConflict" })
  assert.deepEqual(messages, ["Refresh before editing"])
  assert.equal(a.requests.length, count, "a write is never replayed automatically")
  const refresh = a.call("summary")
  a.requests.at(-1).reply(response("123", "epoch-1")); await refresh
  const edit = a.call("apply", { op: {} })
  assert.equal(a.requests.at(-1).options.headers["X-Ox-Revision"], "epoch-1")
  a.requests.at(-1).reply(response("123", "epoch-2")); await edit
  // Metadata on another library entry uses that entry's version, not the editor's.
  const rename = a.call("rename_project", { id: "456", name: "renamed" })
  assert.equal(a.requests.at(-1).options.headers["X-Ox-Project"], "456")
  assert.equal(a.requests.at(-1).options.headers["X-Ox-Revision"], "epoch-7")
  a.requests.at(-1).reply(response("456", "epoch-8")); await rename
  const read = a.call("rows")
  assert.equal(a.requests.at(-1).options.headers["X-Ox-Project"], "123")
  assert.equal(a.requests.at(-1).options.headers["X-Ox-Revision"], "epoch-2")
  a.requests.at(-1).reply(response("123", "epoch-2")); await read
})

test("late selection and old-view responses cannot replace a newer selected project", async () => {
  const a = api()
  const old = a.call("open_project", { id: "123" })
  const latest = a.call("open_project", { id: "456" })
  a.requests[1].reply(response("456", "epoch-5")); await latest
  const rejected = assert.rejects(old, { name: "ProjectContextChanged" })
  a.requests[0].reply(response("123", "epoch-0")); await rejected
  assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, "456")
  const read = a.call("rows")
  const readRequest = a.requests.at(-1)
  const close = a.call("close")
  a.requests.at(-1).reply(response(null, null, null)); await close
  const staleRead = assert.rejects(read, { name: "ProjectContextChanged" })
  readRequest.reply(response("456", "epoch-5", ["old rows"])); await staleRead
  assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, null)
})

test("partial errors require a fresh summary and free job polls do not change the project revision", async () => {
  const a = api(); await select(a, "123")
  const failed = a.call("apply_recipe", { ops: [] })
  a.requests.at(-1).reply("step 2: missing column; 1 of 2 steps remain applied", 400)
  await assert.rejects(failed, /1 of 2/)
  const jobs = a.call("processes", {}, { quiet: true })
  a.requests.at(-1).reply({ data: [], ms: 0 }); await jobs
  const fresh = a.call("summary")
  assert.equal(a.requests.at(-1).options.headers["X-Ox-Revision"], "epoch-0")
  a.requests.at(-1).reply(response("123", "epoch-1")); await fresh
  const next = a.call("apply", { op: {} })
  assert.equal(a.requests.at(-1).options.headers["X-Ox-Revision"], "epoch-1")
  a.requests.at(-1).reply(response("123", "epoch-2")); await next
})

test("quiet process memory stays free of project contexts during a running command and selection change", async () => {
  const a = api(); await select(a, "123")
  const counts = []; a.onPending((n) => counts.push(n))
  const running = a.call("apply", { op: {} }); const operation = a.requests.at(-1)
  const memory = a.call("memory", {}, { quiet: true }); const read = a.requests.at(-1)
  await select(a, "456")
  read.reply({ data: { pid: 9, currentBytes: 100, peakBytes: 200, metric: "rss", peakSource: "os", error: null }, ms: 0 })
  assert.equal((await memory).data.currentBytes, 100, "global read needs no project context and survives project switching")
  assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, "456")
  assert.deepEqual(counts, [0, 1, 2, 1], "memory polling must not count as an active command")
  const stale = assert.rejects(running, { name: "ProjectContextChanged" }); operation.reply(response("123", "epoch-1")); await stale
})

test("import metadata requires no project context while another project is opened", async () => {
  for (const command of ["fixed_width_sample", "wikitext_tables"]) {
    const a = api()
    const sample = a.call(command, { path: "input.txt" }, { quiet: true }); const request = a.requests.at(-1)
    await select(a, "123")
    const data = command === "fixed_width_sample" ? { lines: ["ABC  123"] } : { tables: [{ index: 0, rows: 2, columns: 2 }] }
    request.reply({ data, ms: 0 })
    assert.deepEqual((await sample).data, data)
    assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, "123")
  }
})

test("Commons snapshots stay independent of project changes; only successful creation selects a project", async () => {
  const a = api(); await select(a, "123")
  const preparation = a.call("commons_prepare", { params: { requestId: "test-request" } })
  const pending = a.requests.at(-1)
  await select(a, "456")
  pending.reply({ data: { id: "snapshot", total: 2 }, ms: 10 })
  assert.equal((await preparation).data.id, "snapshot")
  const preview = a.call("commons_preview", { id: "snapshot" })
  a.requests.at(-1).reply({ data: { rows: [], total: 2 }, ms: 0 }); await preview
  assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, "456")

  const oldRows = a.call("rows"); const read = a.requests.at(-1)
  const created = a.call("commons_create", { id: "snapshot", name: "Commons files" })
  a.requests.at(-1).reply(response("789", "epoch-0", { id: "789" })); await created
  assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, "789")
  const stale = assert.rejects(oldRows, { name: "ProjectContextChanged" })
  read.reply(response("456", "epoch-0", [])); await stale

  const expired = a.call("commons_create", { id: "expired" })
  a.requests.at(-1).reply("Commons preview expired", 400)
  await assert.rejects(expired, /expired/)
  assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, "789")
})

test("database profile/query commands stay free of project revisions, CAS errors do not refresh the editor", async () => {
  for (const command of ["database_profiles", "database_save_profile", "database_delete_profile", "database_test", "database_schema", "database_prepare", "database_preview", "storage_stats"]) {
    const a = api(); await select(a, "old")
    const operation = a.call(command, {}); const request = a.requests.at(-1)
    await select(a, "current")
    request.reply({ data: { id: "snapshot" }, ms: 0 }); await operation
    assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, "current")
  }
  const a = api(); await select(a, "old")
  let conflicts = 0; a.onProjectConflict(() => conflicts++)
  const save = a.call("database_save_profile", { params: { profile: {} } })
  a.requests.at(-1).reply({ code: "stale_profile", message: "Connection profile changed; reload profiles" }, 409)
  await assert.rejects(save, /reload profiles/)
  assert.equal(conflicts, 0)
  assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, "old")
  const failed = a.call("database_create", { id: "expired", name: "Data" })
  a.requests.at(-1).reply("Database snapshot expired", 400); await assert.rejects(failed, /expired/)
  assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, "old")
  const create = a.call("database_create", { id: "snapshot", name: "Data" })
  a.requests.at(-1).reply(response("new", "epoch-0", { id: "new" })); await create
  assert.equal(JSON.parse(a.storage.get("ox-project-context")).project, "new")
})
