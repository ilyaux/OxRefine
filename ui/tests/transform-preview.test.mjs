import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

function dialog() {
  const hooks = [], requests = [], applied = [], timers = new Map()
  let cursor = 0, dirty = true, effects = [], view, clock = 0, timerId = 0
  const react = {
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
  const code = ts.transpileModule(readFileSync(new URL("../src/Dialogs.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
  }).outputText
  vm.runInNewContext(code, {
    exports,
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx }
      if (name === "@/api") return { call: (cmd, args) => new Promise((resolve, reject) => requests.push({ cmd, args, resolve, reject })) }
      if (name === "@/grel-docs.json") return { functions: {} }
      if (name === "@/ExpressionEditor") return { ExpressionEditor: "editor", recentExpressions: () => [], rememberExpression: () => {} }
      if (name === "@/lib/utils") return { cn: (...s) => s.filter(Boolean).join(" ") }
      return new Proxy({}, { get: (_, key) => String(key) })
    },
    setTimeout(f, delay) { const id = ++timerId; timers.set(id, { at: clock + delay, f }); return id },
    clearTimeout(id) { timers.delete(id) },
  })
  const props = { column: "value", columns: ["value"], mode: "transform", engine: { facets: [], mode: "row-based" }, rows: 4,
    onApply: async (op) => { applied.push(op) }, onClose: () => {} }
  function flush() {
    let count = 0
    while (dirty) {
      assert(++count < 20, "dialog settles")
      dirty = false; cursor = 0
      view = exports.TransformDialog(props)
      const queued = effects; effects = []
      queued.forEach((f) => f())
    }
  }
  flush()
  return {
    requests, applied,
    get view() { return view },
    change(node, value) { node.props.onChange(node.type === "editor" ? value : { target: { value, checked: value } }); flush() },
    click(node) { node.props.onClick(); flush() },
    async settle(i, data) { requests[i].resolve({ data, ms: 1 }); await Promise.resolve(); await Promise.resolve(); flush() },
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
const cell = (s) => ({ text: s, type: "text" })
const result = (changedValues = 1) => ({ rows: 4, changedRows: 2, distinctBefore: 3, distinctAfter: 2, changedValues,
  columnDistinctAfter: 2, multipleResults: false, totalPairs: 3, matchingPairs: 3, start: 0,
  items: [{ before: cell(" a "), after: cell("a"), rows: 2, changed: true }],
  rowPreview: [{ before: cell(" a "), after: cell("a"), row: 0, changed: true }],
})
const applyButton = (d) => nodes(d.view).find((n) => n.type === "CellButton" && /Apply to/.test(text(n)))

test("transform options preview the actual operation and Apply waits for the latest reply", async () => {
  const d = dialog()
  assert.equal(d.requests[0].cmd, "preview_transform")
  assert.equal(d.requests[0].args.op.onError, "keep-original")
  assert.equal(applyButton(d).props.disabled, true)
  await d.settle(0, result())
  assert.equal(applyButton(d).props.disabled, false)
  const policy = nodes(d.view).find((n) => n.type === "select" && n.props.value === "keep-original")
  d.change(policy, "store-error")
  assert.equal(d.requests[1].args.op.onError, "store-error")
  assert.equal(applyButton(d).props.disabled, true)
  const label = nodes(d.view).find((n) => n.type === "label" && /repeat until/.test(text(n)))
  d.change(nodes(label).find((n) => n.type === "input"), true)
  assert.equal(d.requests[2].args.op.repeat, true)
  assert.equal(d.requests[2].args.op.repeatCount, 10)
  await d.settle(1, result(99))
  assert.equal(applyButton(d).props.disabled, true, "late old options must not unlock Apply")
  assert.doesNotMatch(text(d.view), /99\s+of\s+3/)
  await d.settle(2, result(2))
  assert.match(text(d.view), /2\s+of\s+3\s+distinct values change/)
  d.click(applyButton(d))
  assert.equal(d.applied.length, 1)
  assert.equal(d.applied[0].onError, "store-error")
  assert.equal(d.applied[0].repeat, true)
  d.unmount()
})

test("row view remains available and a newly typed expression cannot apply the old preview", async () => {
  const d = dialog()
  await d.settle(0, result())
  const rows = nodes(d.view).find((n) => n.type === "Button" && text(n) === "Rows")
  d.click(rows)
  assert.match(text(d.view), /Showing the first 1 matching rows/)
  assert.equal(d.requests.length, 1, "both views use the same evaluated result")
  d.change(nodes(d.view).find((n) => n.type === "editor"), "value.trim()")
  assert.equal(applyButton(d).props.disabled, true, "debounce must not allow stale application")
  d.click(applyButton(d))
  assert.equal(d.applied.length, 0)
  await d.advance(200)
  assert.equal(d.requests[1].args.op.expression, "grel:value.trim()")
  await d.settle(1, result())
  assert.equal(applyButton(d).props.disabled, false)
  d.unmount()
})
