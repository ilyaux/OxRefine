import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

const KEY = "reconciliation.standardServices"
const clone = (value) => JSON.parse(JSON.stringify(value))
function registry(value = undefined, failures = {}) {
  const calls = [],
    preferences = value === undefined ? {} : { [KEY]: clone(value) }
  const exports = {}
  const code = ts.transpileModule(
    readFileSync(
      new URL("../src/reconServiceViews.ts", import.meta.url),
      "utf8"
    ),
    {
      compilerOptions: {
        module: ts.ModuleKind.CommonJS,
        target: ts.ScriptTarget.ES2023,
      },
    }
  ).outputText
  vm.runInNewContext(code, {
    exports,
    require(name) {
      assert.equal(name, "@/api")
      return {
        async call(cmd, args, options) {
          calls.push({ cmd, args: clone(args), quiet: options.quiet })
          if (cmd === "preferences") {
            if (failures.read) {
              failures.read--
              throw new Error("read failed")
            }
            return { data: clone(preferences) }
          }
          assert.equal(cmd, "set_preference")
          assert.equal(args.name, KEY)
          if (failures.write) {
            failures.write--
            throw new Error("write failed")
          }
          preferences[args.name] = clone(args.value)
          return { data: clone(preferences) }
        },
      }
    },
  })
  return { save: exports.saveServiceView, calls, preferences }
}

test("service view merges by URL and preserves other registry data without persisting the full manifest", async () => {
  const r = registry([
    {
      url: "https://other.test/recon",
      view: { url: "https://other.test/{{id}}" },
      untouched: true,
    },
    {
      url: "https://service.test/recon",
      name: "Existing",
      view: { url: "https://old.test/{{id}}", width: 300 },
    },
  ])
  await r.save("https://service.test/recon", {
    name: "Unneeded manifest name",
    token: "not stored",
    view: { url: "https://new.test/item/{{id}}", width: 999 },
  })
  assert.deepEqual(r.preferences[KEY], [
    {
      url: "https://other.test/recon",
      view: { url: "https://other.test/{{id}}" },
      untouched: true,
    },
    {
      url: "https://service.test/recon",
      name: "Existing",
      view: { url: "https://new.test/item/{{id}}", width: 300 },
    },
  ])
  assert.equal(r.calls.filter((c) => c.cmd === "preferences").length, 1)
  assert.equal(r.calls.filter((c) => c.cmd === "set_preference").length, 1)
  assert(r.calls.every((c) => c.quiet))
  await r.save("https://missing-view.test/recon", {})
  await r.save("https://invalid-view.test/recon", { view: { url: 42 } })
  assert.equal(
    r.calls.length,
    2,
    "services without a view have nothing to persist"
  )
})

test("concurrent templates are merged serially, identical in-flight writes deduplicate, and later calls recheck server state", async () => {
  const r = registry()
  const first = r.save("https://a.test/recon", {
    view: { url: "https://a.test/{{id}}" },
  })
  const duplicate = r.save("https://a.test/recon", {
    view: { url: "https://a.test/{{id}}" },
  })
  assert.equal(first, duplicate)
  const second = r.save("https://b.test/recon", {
    view: { url: "https://b.test/{{id}}" },
  })
  await Promise.all([first, second])
  assert.deepEqual(r.preferences[KEY], [
    { url: "https://a.test/recon", view: { url: "https://a.test/{{id}}" } },
    { url: "https://b.test/recon", view: { url: "https://b.test/{{id}}" } },
  ])
  r.preferences[KEY].push({
    url: "https://peer.test/recon",
    view: { url: "https://peer.test/{{id}}" },
  })
  await r.save("https://a.test/recon", {
    view: { url: "https://a.test/{{id}}" },
  })
  assert.equal(r.calls.filter((c) => c.cmd === "preferences").length, 3)
  assert.equal(
    r.calls.filter((c) => c.cmd === "set_preference").length,
    2,
    "unchanged descriptors need no write"
  )
  assert.equal(r.preferences[KEY].length, 3)
  r.preferences[KEY] = []
  await r.save("https://a.test/recon", {
    view: { url: "https://a.test/{{id}}" },
  })
  assert.equal(
    r.preferences[KEY][0].url,
    "https://a.test/recon",
    "a prior successful cache cannot hide a removed server descriptor"
  )
})

test("read/write failures do not poison retry and malformed registries are never overwritten", async () => {
  for (const failures of [{ read: 1 }, { write: 1 }]) {
    const r = registry(undefined, failures)
    const save = () =>
      r.save("https://service.test/recon", {
        view: { url: "https://entity.test/{{id}}" },
      })
    await assert.rejects(save(), /failed/)
    assert.equal(r.preferences[KEY], undefined)
    await save()
    assert.deepEqual(r.preferences[KEY], [
      {
        url: "https://service.test/recon",
        view: { url: "https://entity.test/{{id}}" },
      },
    ])
  }
  for (const invalid of ["{bad JSON", { invalid: "not a list" }]) {
    const r = registry(invalid)
    await assert.rejects(
      r.save("https://service.test/recon", {
        view: { url: "https://entity.test/{{id}}" },
      }),
      /registry/
    )
    assert.deepEqual(r.preferences[KEY], invalid)
    assert.equal(r.calls.filter((c) => c.cmd === "set_preference").length, 0)
  }
  const legacy = registry(
    JSON.stringify([
      {
        service: "https://service.test/recon",
        view: { url: "https://old.test/{{id}}" },
      },
    ])
  )
  await legacy.save("https://service.test/recon", {
    view: { url: "https://entity.test/{{id}}" },
  })
  assert.equal(legacy.preferences[KEY][0].service, "https://service.test/recon")
  assert.equal(
    legacy.preferences[KEY][0].view.url,
    "https://entity.test/{{id}}"
  )
  const alreadyMatching = registry(
    JSON.stringify([
      {
        url: "https://service.test/recon",
        view: { url: "https://entity.test/{{id}}" },
      },
    ])
  )
  await alreadyMatching.save("https://service.test/recon", {
    view: { url: "https://entity.test/{{id}}" },
  })
  assert(
    Array.isArray(alreadyMatching.preferences[KEY]),
    "a string-encoded registry must become the native array consumed by export, even if its view is unchanged"
  )
})
