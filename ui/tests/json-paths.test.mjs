import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

function mount(component, props) {
  const hooks = [], requests = [], timers = new Map(), created = []
  let cursor = 0, dirty = true, effects = [], view, clock = 0, id = 0
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
    setTimeout(f, delay) { const key = ++id; timers.set(key, { at: clock + delay, f }); return key },
    clearTimeout(key) { timers.delete(key) },
  })
  props.onCreate = async (args) => created.push(args)
  function flush() {
    let count = 0
    while (dirty) {
      assert(++count < 20, "component settles")
      dirty = false; cursor = 0
      view = exports[component](props)
      const queued = effects; effects = []; queued.forEach((f) => f())
    }
  }
  flush()
  return {
    requests, created, get view() { return view },
    invoke(node, handler, event = {}) { node.props[handler](event); flush() },
    change(node, value) { node.props.onChange({ target: { value } }); flush() },
    update(next) { Object.assign(props, next); dirty = true; flush() },
    async settle(request, data) { request.resolve({ data, ms: 1 }); await Promise.resolve(); await Promise.resolve(); flush() },
    async advance(ms) { clock += ms; for (const [key, timer] of [...timers]) if (timer.at <= clock) { timers.delete(key); timer.f(); flush() } await Promise.resolve(); flush() },
    unmount() { hooks.forEach((h) => h.cleanup?.()) },
  }
}
function nodes(n) { return Array.isArray(n) ? n.flatMap(nodes) : n && typeof n === "object" ? [n, ...nodes(n.props?.children)] : [] }
function text(n) { return Array.isArray(n) ? n.map(text).join(" ") : n && typeof n === "object" ? text(n.props?.children) : typeof n === "string" || typeof n === "number" ? String(n) : "" }
const last = (d, cmd) => d.requests.filter((r) => r.cmd === cmd).at(-1)
const pathInput = (d) => nodes(nodes(d.view).find((n) => n.type === "label" && /Path to the records/.test(text(n)))).find((n) => n.type === "Input")
const create = (d) => nodes(d.view).find((n) => n.type === "StripAction")
const picker = (d) => nodes(d.view).find((n) => n.type === "JsonPathPicker")
const selected = ["_", "data", "items", "_"]
const metadata = (path = selected, sampled = false) => ({ file: "nested.json", suggestedPath: path, sampled,
  paths: [{ path: ["_"], count: 1, type: "object", fields: ["data"] }, { path, count: 2, type: "object", fields: ["sku", "price"] }],
  limits: { bytes: 262144, nodes: 2000, depth: 16, paths: 256, records: 100, keyCharacters: 256 } })
const info = () => ({ format: "json", options: { recordPath: selected, encoding: "UTF-8", storeEmptyStrings: true }, json: metadata() })
const preview = (s = "SKU") => ({ columns: ["_ - sku"], rows: [[0, [s], false, false]] })

test("JSON outline is a keyboard native list and explicitly labels sampled counts and limits", () => {
  let chosen
  const d = mount("JsonPathPicker", { data: metadata(selected, true), value: selected, onChange: (p) => { chosen = p } })
  const list = nodes(d.view).find((n) => n.type === "select")
  assert.equal(list.props.id, "json-record-paths")
  assert.match(text(d.view), /Sample counts only/)
  assert.match(text(d.view), /2[,\s]000 nodes/)
  assert.match(text(d.view), /depth\s+16/)
  assert.match(text(d.view), /2\s+object\s+in sample/)
  const punctuation = ["_", "data.with/slash", "rows,here", "_"]
  d.change(list, JSON.stringify(punctuation))
  assert.deepEqual(Array.from(chosen), punctuation)
  d.unmount()
})

test("suggested path is previewed, manual JSON paths preserve punctuation and invalid drafts cannot create", async () => {
  const d = mount("ImportDialog", { path: "nested.json", onClose() {} })
  await d.settle(last(d, "inspect"), info())
  assert.equal(pathInput(d).props.value, JSON.stringify(selected))
  assert.ok(picker(d))
  await d.advance(300)
  assert.deepEqual(Array.from(last(d, "preview_import").args.options.recordPath), selected)
  await d.settle(last(d, "preview_import"), preview())
  assert.equal(create(d).props.disabled, false)
  d.change(pathInput(d), "_, data")
  assert.equal(pathInput(d).props["aria-invalid"], true)
  assert.equal(create(d).props.disabled, true)
  d.invoke(create(d), "onClick")
  await Promise.resolve()
  assert.equal(d.created.length, 0)
  const path = ["_", "data.with/slash", "rows,here", "_"]
  d.change(pathInput(d), JSON.stringify(path))
  assert.equal(pathInput(d).props["aria-invalid"], false)
  await d.advance(300)
  assert.deepEqual(Array.from(last(d, "preview_import").args.options.recordPath), path)
  d.invoke(picker(d), "onChange", selected)
  assert.equal(pathInput(d).props.value, JSON.stringify(selected))
  d.unmount()
})

test("empty or unmatched JSON preview has an explanation and cannot create a blank project", async () => {
  const d = mount("ImportDialog", { path: "empty.json", onClose() {} })
  await d.settle(last(d, "inspect"), info())
  await d.advance(300)
  await d.settle(last(d, "preview_import"), { columns: [], rows: [] })
  assert.match(text(d.view), /No rows or columns at this record path/)
  assert.match(text(d.view), /empty or null/)
  assert.equal(create(d).props.disabled, true)
  d.unmount()
})

test("encoding and file changes discard late outline and preview replies without overwriting manual paths", async () => {
  const d = mount("ImportDialog", { path: "first.json", onClose() {} })
  await d.settle(last(d, "inspect"), info())
  await d.advance(300)
  const oldPreview = last(d, "preview_import")
  const encoding = nodes(d.view).find((n) => n.type === "select" && n.props.value === "UTF-8")
  d.change(encoding, "windows-1252")
  const oldMetadata = last(d, "json_paths")
  const manual = ["_", "literal_", "rows,here"]
  d.change(pathInput(d), JSON.stringify(manual))
  await d.settle(oldMetadata, metadata(["_", "wrong"]))
  assert.equal(pathInput(d).props.value, JSON.stringify(manual), "metadata cannot overwrite a manual choice")
  await d.settle(oldPreview, preview("STALE"))
  assert.doesNotMatch(text(d.view), /STALE/)
  d.change(nodes(d.view).find((n) => n.type === "select" && n.props.value === "windows-1252"), "UTF-16LE")
  const staleMetadata = last(d, "json_paths")
  d.update({ path: "second.json" })
  await d.settle(staleMetadata, metadata(["_", "old-file"]))
  assert.equal(picker(d), undefined)
  await d.settle(last(d, "inspect"), info())
  assert.equal(pathInput(d).props.value, JSON.stringify(selected))
  await d.advance(300)
  const latest = last(d, "preview_import")
  assert.equal(latest.args.path, "second.json")
  await d.settle(latest, preview("SECOND"))
  assert.match(text(d.view), /SECOND/)
  d.unmount()
})

test("switching to JSON Lines seeds canonical roots and preserves explicit underscore field paths", async () => {
  const d = mount("ImportDialog", { path: "records.json", onClose() {} })
  await d.settle(last(d, "inspect"), info())
  const label = nodes(d.view).find((n) => n.type === "label" && /JSON lines/.test(text(n)))
  d.invoke(nodes(label).find((n) => n.type === "input"), "onChange")
  assert.equal(last(d, "json_paths").args.format, "jsonlines")
  assert.equal(pathInput(d).props.value, '["_"]')
  await d.settle(last(d, "json_paths"), metadata(["_"]))
  const literalKey = ["_", "_", "_"]
  d.change(pathInput(d), JSON.stringify(literalKey))
  await d.advance(300)
  const request = last(d, "preview_import")
  assert.equal(request.args.options.jsonLinesRoot, true)
  assert.deepEqual(Array.from(request.args.options.recordPath), literalKey, "no blind underscore skipping")
  d.unmount()
})
