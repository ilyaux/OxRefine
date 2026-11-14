import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

// Run the real transport module with deferred HTTP calls; no DOM or Tauri runtime is needed.
function api() {
  const requests = []
  const exports = {}
  const source = readFileSync(new URL("../src/api.ts", import.meta.url), "utf8")
  const code = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2023,
    },
  }).outputText
  vm.runInNewContext(code, {
    exports,
    require: () => ({}),
    window: {},
    location: { search: "" },
    sessionStorage: { getItem: () => null },
    URLSearchParams,
    fetch: () =>
      new Promise((resolve, reject) =>
        requests.push({
          resolve: () =>
            resolve({ ok: true, json: async () => ({ data: null, ms: 0 }) }),
          reject,
        })
      ),
  })
  return { ...exports, requests }
}

test("pending is counted without listeners and reported on subscribe/remount", async () => {
  const a = api()
  const request = a.call("summary")
  const first = []
  a.onPending((n) => first.push(n))()
  assert.deepEqual(first, [1])
  a.requests[0].resolve()
  await request
  const remounted = []
  const off = a.onPending((n) => remounted.push(n))
  assert.deepEqual(remounted, [0])
  off()
})

test("multiple subscribers and overlapping requests count commands once, including failure", async () => {
  const a = api()
  const first = [],
    second = []
  const off = a.onPending((n) => first.push(n))
  a.onPending((n) => second.push(n))
  const one = a.call("open")
  const two = a.call("apply")
  assert.deepEqual(first, [0, 1, 2])
  assert.deepEqual(second, [0, 1, 2])
  a.requests[0].resolve()
  await one
  off()
  a.requests[1].reject(new Error("connection lost"))
  await assert.rejects(two, /connection lost/)
  assert.deepEqual(first, [0, 1, 2, 1])
  assert.deepEqual(second, [0, 1, 2, 1, 0])
})

test("quiet progress requests never keep the jobs poller alive", async () => {
  const a = api()
  const seen = []
  a.onPending((n) => seen.push(n))
  const quiet = a.call("processes", {}, { quiet: true })
  a.requests[0].resolve()
  await quiet
  assert.deepEqual(seen, [0])
})
