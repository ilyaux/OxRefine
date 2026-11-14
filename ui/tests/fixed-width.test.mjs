import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

function mount(component, props) {
  const hooks = [], requests = [], timers = new Map(), created = []
  let cursor = 0, dirty = true, effects = [], view, clock = 0, timerId = 0
  const react = {
    useRef(initial) { return hooks[cursor++] ??= { current: initial } },
    useState(initial) {
      const i = cursor++
      hooks[i] ??= { value: typeof initial === "function" ? initial() : initial }
      return [hooks[i].value, (next) => {
        const value = typeof next === "function" ? next(hooks[i].value) : next
        if (!Object.is(value, hooks[i].value)) { hooks[i].value = value; dirty = true }
      }]
    },
    useEffect(effect, deps) {
      const i = cursor++, previous = hooks[i]
      if (!previous || deps.some((v, j) => !Object.is(v, previous.deps[j]))) {
        hooks[i] = { deps, cleanup: previous?.cleanup }
        effects.push(() => { hooks[i].cleanup?.(); hooks[i].cleanup = effect() })
      }
    },
  }
  const exports = {}, jsx = (type, props) => ({ type, props })
  const code = ts.transpileModule(readFileSync(new URL(`../src/${component}.tsx`, import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
  }).outputText
  vm.runInNewContext(code, {
    exports,
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx }
      if (name === "@/api") return { call: (cmd, args) => new Promise((resolve, reject) => requests.push({ cmd, args, resolve, reject })) }
      if (name === "@/lib/utils") return { cn: (...s) => s.filter(Boolean).join(" ") }
      return new Proxy({}, { get: (_, key) => String(key) })
    },
    setTimeout(f, delay) { const id = ++timerId; timers.set(id, { at: clock + delay, f }); return id },
    clearTimeout(id) { timers.delete(id) },
  })
  props.onCreate = async (args) => created.push(args)
  if (component === "FixedWidthRuler") props.onChange = (widths) => { props.widths = widths; dirty = true }
  function flush() {
    let count = 0
    while (dirty) {
      assert(++count < 20, "component settles")
      dirty = false; cursor = 0
      view = exports[component](props)
      for (const n of nodes(view)) if (n.props?.ref) n.props.ref.current = { getBoundingClientRect: () => ({ left: 100, width: 800 }) }
      const queued = effects; effects = []
      queued.forEach((f) => f())
    }
  }
  flush()
  return {
    props, requests, created,
    get view() { return view },
    invoke(node, handler, event = {}) { node.props[handler](event); flush() },
    change(node, value) { node.props.onChange({ target: { value } }); flush() },
    update(next) { Object.assign(props, next); dirty = true; flush() },
    async settle(request, data) { request.resolve({ data, ms: 1 }); await Promise.resolve(); await Promise.resolve(); flush() },
    async advance(ms) {
      clock += ms
      for (const [id, timer] of [...timers]) if (timer.at <= clock) { timers.delete(id); timer.f(); flush() }
      await Promise.resolve(); flush()
    },
    unmount() { hooks.forEach((h) => h.cleanup?.()) },
  }
}
function nodes(n) {
  if (Array.isArray(n)) return n.flatMap(nodes)
  return n && typeof n === "object" ? [n, ...nodes(n.props?.children)] : []
}
function text(n) {
  if (Array.isArray(n)) return n.map(text).join(" ")
  return n && typeof n === "object" ? text(n.props?.children) : typeof n === "string" || typeof n === "number" ? String(n) : ""
}
const lastRequest = (d, cmd) => d.requests.filter((r) => r.cmd === cmd).at(-1)
const marker = (d, i = 0) => nodes(d.view).filter((n) => n.props?.role === "slider")[i]
const widthsInput = (d) => nodes(nodes(d.view).find((n) => n.type === "label" && /Column widths/.test(text(n)))).find((n) => n.type === "Input")
const createButton = (d) => nodes(d.view).find((n) => n.type === "StripAction")
const pointer = (x) => ({ clientX: x, pointerId: 1, preventDefault() {}, stopPropagation() {}, currentTarget: { focus() {}, setPointerCapture() {}, releasePointerCapture() {} } })
const sample = (columnWidths = [3]) => ({ columnWidths, lines: ["aa bb", "cc dd", "ee ff"], sampledLines: 3, truncated: false, characterUnit: "unicode-scalar" })
const preview = (s) => ({ columns: ["value"], rows: [[0, [s], false, false]] })
const info = { format: "fixed-width", options: { encoding: "UTF-8", headerLines: 0, columnWidths: [] } }

test("ruler adds, drags, clamps and removes positive character widths with pointer and keyboard", () => {
  const d = mount("FixedWidthRuler", { lines: ["A😀京é"], widths: [4, 4] })
  const cells = nodes(d.view).filter((n) => /w-\[2ch\]/.test(n.props?.className ?? ""))
  assert.equal(cells.length, 5, "emoji is one scalar slot; combining mark has its own slot")
  assert.equal(marker(d).props["aria-valuenow"], 4)
  d.invoke(marker(d), "onPointerDown", pointer(180))
  d.invoke(marker(d), "onPointerMove", pointer(220))
  assert.deepEqual(Array.from(d.props.widths), [6, 2])
  d.invoke(marker(d), "onPointerMove", pointer(900))
  assert.deepEqual(Array.from(d.props.widths), [7, 1], "a marker cannot cross the next marker")
  d.invoke(marker(d), "onPointerUp", pointer(900))
  d.invoke(marker(d), "onKeyDown", { key: "Home", preventDefault() {} })
  assert.deepEqual(Array.from(d.props.widths), [1, 7])
  d.invoke(marker(d), "onKeyDown", { key: "ArrowLeft", preventDefault() {} })
  assert.deepEqual(Array.from(d.props.widths), [1, 7], "zero widths cannot be produced")
  d.invoke(marker(d), "onKeyDown", { key: "Delete", preventDefault() {} })
  assert.deepEqual(Array.from(d.props.widths), [8])
  const track = nodes(d.view).find((n) => /cursor-crosshair/.test(n.props?.className ?? ""))
  d.invoke(track, "onClick", pointer(200))
  assert.deepEqual(Array.from(d.props.widths), [5, 3])
  d.invoke(marker(d), "onDoubleClick")
  assert.deepEqual(Array.from(d.props.widths), [8])
  d.invoke(nodes(d.view).find((n) => n.type === "Button" && text(n) === "Add split"), "onClick")
  assert.ok(marker(d).props["aria-valuenow"] <= 5, "default split stays inside real source text")
  d.unmount()
})

test("ruler keeps long source overflow bounded and visible markers inside the displayed range", () => {
  const d = mount("FixedWidthRuler", { lines: ["short"], widths: [4, 600] })
  assert.equal(marker(d).props["aria-valuemax"], 40)
  d.invoke(marker(d), "onKeyDown", { key: "End", preventDefault() {} })
  assert.deepEqual(Array.from(d.props.widths), [40, 564])
  d.invoke(marker(d), "onKeyDown", { key: "ArrowRight", preventDefault() {} })
  assert.equal(d.props.widths[0], 40)
  assert.match(text(d.view), /splits past character 512/)
  assert.ok(nodes(d.view).some((n) => /overflow-auto/.test(n.props?.className ?? "")))
  d.unmount()
})

test("import widths reject invalid drafts and explicit ruler or guess replaces them", async () => {
  const d = mount("ImportDialog", { path: "first.txt", onClose() {} })
  await d.settle(lastRequest(d, "inspect"), info)
  await d.settle(lastRequest(d, "fixed_width_sample"), sample())
  await d.advance(300)
  await d.settle(lastRequest(d, "preview_import"), preview("valid"))
  assert.equal(widthsInput(d).props.value, "3")
  assert.equal(createButton(d).props.disabled, false)
  for (const invalid of ["3, x, 10", "3, 0", "3, -1", "1.5", "Infinity", "9007199254740992", "9007199254740991, 1"]) {
    d.change(widthsInput(d), invalid)
    assert.equal(widthsInput(d).props.value, invalid)
    assert.equal(widthsInput(d).props["aria-invalid"], true)
    assert.equal(createButton(d).props.disabled, true)
    assert.deepEqual(Array.from(nodes(d.view).find((n) => n.type === "FixedWidthRuler").props.widths), [3])
  }
  d.invoke(createButton(d), "onClick")
  await Promise.resolve()
  assert.equal(d.created.length, 0, "even a stale click handler cannot import a different sanitized draft")
  const ruler = nodes(d.view).find((n) => n.type === "FixedWidthRuler")
  d.invoke(ruler, "onChange", [2, 3])
  assert.equal(widthsInput(d).props.value, "2, 3")
  assert.equal(widthsInput(d).props["aria-invalid"], false)
  d.change(widthsInput(d), "2, x")
  d.invoke(nodes(d.view).find((n) => n.type === "button" && text(n) === "Guess widths"), "onClick")
  assert.equal(widthsInput(d).props.value, "3")
  assert.equal(widthsInput(d).props["aria-invalid"], false)
  d.change(widthsInput(d), "3, ")
  assert.equal(widthsInput(d).props.value, "3, ", "a comma can be typed without resetting the input")
  assert.equal(widthsInput(d).props["aria-invalid"], false)
  d.change(widthsInput(d), "")
  assert.deepEqual(Array.from(nodes(d.view).find((n) => n.type === "FixedWidthRuler").props.widths), [])
  d.unmount()
})

test("dragging, encoding and file changes discard old previews, inspections and sample replies", async () => {
  const d = mount("ImportDialog", { path: "old.txt", onClose() {} })
  const oldInspect = lastRequest(d, "inspect")
  d.update({ path: "first.txt" })
  await d.settle(oldInspect, { format: "csv", options: {} })
  assert.equal(d.requests.filter((r) => r.cmd === "fixed_width_sample").length, 0)
  await d.settle(lastRequest(d, "inspect"), info)
  await d.settle(lastRequest(d, "fixed_width_sample"), sample())
  await d.advance(300)
  const oldPreview = lastRequest(d, "preview_import")
  d.invoke(nodes(d.view).find((n) => n.type === "FixedWidthRuler"), "onChange", [4])
  assert.equal(createButton(d).props.disabled, true)
  await d.advance(300)
  const dragged = lastRequest(d, "preview_import")
  await d.settle(oldPreview, preview("STALE"))
  assert.doesNotMatch(text(d.view), /STALE/)
  await d.settle(dragged, preview("CURRENT"))
  assert.match(text(d.view), /CURRENT/)
  const encoding = nodes(d.view).find((n) => n.type === "select" && n.props.value === "UTF-8")
  d.change(encoding, "windows-1252")
  const oldSample = lastRequest(d, "fixed_width_sample")
  await d.advance(300)
  const oldEncodingPreview = lastRequest(d, "preview_import")
  d.update({ path: "second.txt" })
  await d.settle(oldSample, sample([99]))
  await d.settle(oldEncodingPreview, preview("OLD ENCODING"))
  assert.doesNotMatch(text(d.view), /OLD ENCODING/)
  assert.equal(createButton(d).props.disabled, true)
  await d.settle(lastRequest(d, "inspect"), info)
  await d.settle(lastRequest(d, "fixed_width_sample"), sample([2]))
  assert.equal(widthsInput(d).props.value, "2")
  await d.advance(300)
  const latest = lastRequest(d, "preview_import")
  assert.equal(latest.args.path, "second.txt")
  assert.equal(latest.args.options.encoding, "UTF-8")
  await d.settle(latest, preview("SECOND FILE"))
  assert.match(text(d.view), /SECOND FILE/)
  assert.equal(createButton(d).props.disabled, false)
  d.unmount()
})

test("an automatic width guess cannot overwrite a draft typed while the sample was loading", async () => {
  const d = mount("ImportDialog", { path: "first.txt", onClose() {} })
  await d.settle(lastRequest(d, "inspect"), info)
  d.change(widthsInput(d), "5, invalid")
  await d.settle(lastRequest(d, "fixed_width_sample"), sample([3]))
  assert.equal(widthsInput(d).props.value, "5, invalid")
  assert.equal(widthsInput(d).props["aria-invalid"], true)
  assert.deepEqual(Array.from(nodes(d.view).find((n) => n.type === "FixedWidthRuler").props.widths), [])
  assert.equal(createButton(d).props.disabled, true)
  d.invoke(nodes(d.view).find((n) => n.type === "button" && text(n) === "Guess widths"), "onClick")
  assert.equal(widthsInput(d).props.value, "3")
  assert.equal(widthsInput(d).props["aria-invalid"], false)
  d.unmount()
})
