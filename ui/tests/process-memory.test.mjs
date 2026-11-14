import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

function memoryView(desktop = false) {
  const hooks = [], timers = new Map(), requests = [], exports = {}
  let cursor = 0, dirty = true, effects = [], memory, now = 0, nextTimer = 0
  const react = {
    useState(initial) {
      const i = cursor++
      hooks[i] ??= { value: initial }
      return [hooks[i].value, (value) => { hooks[i].value = value; dirty = true }]
    },
    useEffect(effect) {
      const i = cursor++
      if (!hooks[i]) { hooks[i] = {}; effects.push(() => { hooks[i].cleanup = effect() }) }
    },
  }
  const jsx = (type, props) => ({ type, props })
  const code = ts.transpileModule(readFileSync(new URL("../src/ProcessMemory.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
  }).outputText
  vm.runInNewContext(code, {
    exports,
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx }
      if (name === "@/api") return {
        isTauri: desktop,
        call(command, args, options) {
          assert.equal(command, "memory"); assert.equal(options.quiet, true)
          return new Promise((resolve, reject) => requests.push({ reply: (data) => resolve({ data, ms: 0 }), reject }))
        },
      }
      throw new Error(`Unexpected memory dependency ${name}`)
    },
    setTimeout(callback, delay) { const id = ++nextTimer; timers.set(id, { at: now + delay, callback }); return id },
    clearTimeout(id) { timers.delete(id) },
  })
  function render() {
    if (!dirty) return
    dirty = false; cursor = 0; memory = exports.useProcessMemory()
    const queued = effects; effects = []; queued.forEach((effect) => effect())
  }
  async function flush() { for (let i = 0; i < 4; i++) await Promise.resolve(); render() }
  render()
  return {
    requests, timers,
    get memory() { return memory },
    get label() { return exports.ProcessMemory({ memory }) },
    async flush() { await flush() },
    async advance(ms) {
      const until = now + ms
      for (;;) {
        const next = [...timers].filter(([, timer]) => timer.at <= until).sort((a, b) => a[1].at - b[1].at)[0]
        if (!next) break
        const [id, timer] = next; timers.delete(id); now = timer.at; void timer.callback(); await flush()
      }
      now = until; await flush()
    },
    unmount() { hooks.forEach((hook) => hook.cleanup?.()) },
  }
}
function text(node) {
  if (Array.isArray(node)) return node.map(text).join("")
  if (node && typeof node === "object") return text(node.props?.children)
  return node == null ? "" : String(node)
}
const sample = (current, peak) => ({ pid: 77, currentBytes: current, peakBytes: peak, metric: "rss", peakSource: "os", error: null })

test("one quiet memory request at a time, two-second completion cadence and unmount cleanup", async () => {
  const view = memoryView()
  assert.match(text(view.label), /RAM … · peak …/)
  await view.advance(0); assert.equal(view.requests.length, 1)
  await view.advance(10000); assert.equal(view.requests.length, 1, "slow reads must never overlap")
  view.requests[0].reply(sample(12 * 1024 ** 2, 40 * 1024 ** 2)); await view.flush()
  assert.match(text(view.label), /RAM 12.0 MiB · peak 40.0 MiB/)
  assert.equal(view.label.props["aria-label"], "Server process memory")
  assert.equal(view.label.props["aria-live"], "off", "periodic metrics must not announce inside Jobs' live region")
  assert.match(view.label.props.title, /OS high-water mark/)
  assert.match(view.label.props.title, /all loaded projects/)
  await view.advance(1999); assert.equal(view.requests.length, 1)
  await view.advance(1); assert.equal(view.requests.length, 2)
  const before = view.memory
  view.unmount(); view.requests[1].reply(sample(1000, 2000)); await view.flush()
  assert.equal(view.memory, before, "late responses cannot update an unmounted view")
  await view.advance(10000); assert.equal(view.requests.length, 2); assert.equal(view.timers.size, 0)
})

test("unknown, failed and inconsistent samples never display a made-up zero or observed peak", async () => {
  const view = memoryView(true)
  await view.advance(0); view.requests[0].reject(new Error("offline")); await view.flush()
  assert.match(text(view.label), /RAM — · peak —/)
  assert.doesNotMatch(text(view.label), /0 B/)
  assert.equal(view.label.props["aria-label"], "Backend process memory")
  assert.match(view.label.props.title, /unavailable/)
  await view.advance(2000); view.requests[1].reply(null); await view.flush()
  assert.equal(view.memory.currentBytes, null); assert.equal(view.memory.peakBytes, null)
  await view.advance(2000); view.requests[2].reply(sample(100, 99)); await view.flush()
  assert.equal(view.memory.currentBytes, 100); assert.equal(view.memory.peakBytes, null); assert.equal(view.memory.peakSource, null)
  await view.advance(2000); view.requests[3].reply(sample(Infinity, -1)); await view.flush()
  assert.equal(view.memory.currentBytes, null); assert.equal(view.memory.peakBytes, null)
  await view.advance(2000); view.requests[4].reply({ ...sample(5, 10), metric: "js-heap" }); await view.flush()
  assert.equal(view.memory.currentBytes, null, "unknown metric cannot be presented as process RAM")
  view.unmount()
})

test("cleanup cancels the initial tick before a development effect remount can double-poll", async () => {
  const view = memoryView(); view.unmount(); await view.advance(0)
  assert.equal(view.requests.length, 0); assert.equal(view.timers.size, 0)
})
