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
