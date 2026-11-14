import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

// Run the real wizard with controlled requests and time, so late replies and Create
// use the same state transitions as the browser rather than a copy of the logic.
function mount(path) {
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
  const code = ts.transpileModule(readFileSync(new URL("../src/ImportDialog.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
  }).outputText
  vm.runInNewContext(code, {
    exports, URL,
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
  const props = { path, onClose() {}, onCreate: async (args) => created.push(args) }
  function flush() {
    let count = 0
    while (dirty) {
      assert(++count < 20, "wizard settles")
      dirty = false; cursor = 0
      view = exports.ImportDialog(props)
      const queued = effects; effects = []; queued.forEach((f) => f())
    }
  }
  flush()
  return {
    requests, created, get view() { return view },
    invoke(node, handler, event = {}) { node.props[handler](event); flush() },
    change(node, value) { node.props.onChange({ target: { value } }); flush() },
    check(node, checked) { node.props.onChange({ target: { checked } }); flush() },
    update(next) { Object.assign(props, next); dirty = true; flush() },
    async settle(request, data) { request.resolve({ data, ms: 1 }); await Promise.resolve(); await Promise.resolve(); flush() },
    async fail(request, message) { request.reject(new Error(message)); await Promise.resolve(); await Promise.resolve(); flush() },
    async advance(ms = 300) { clock += ms; for (const [id, timer] of [...timers]) if (timer.at <= clock) { timers.delete(id); timer.f(); flush() } await Promise.resolve(); flush() },
    unmount() { hooks.forEach((h) => h.cleanup?.()) },
  }
}
function nodes(n) { return Array.isArray(n) ? n.flatMap(nodes) : n && typeof n === "object" ? [n, ...nodes(n.props?.children)] : [] }
function text(n) { return Array.isArray(n) ? n.map(text).join(" ") : n && typeof n === "object" ? text(n.props?.children) : typeof n === "string" || typeof n === "number" ? String(n) : "" }
const last = (d, cmd) => d.requests.filter((r) => r.cmd === cmd).at(-1)
const create = (d) => nodes(d.view).find((n) => n.type === "StripAction")
const field = (d, label) => nodes(nodes(d.view).find((n) => n.type === "label" && text(n).trim().startsWith(label))).find((n) => ["input", "Input", "select"].includes(n.type))
const preview = (s = "CURRENT") => ({ columns: ["value"], rows: [[0, [s], false, false]] })
const tables = [{ index: 0, caption: "First", rows: 3, columns: 2 }, { index: 1, caption: "Second", rows: 4, columns: 3 }]
const wikiInfo = () => ({ format: "wikitext", tables, wikiBaseUrl: "https://en.wikipedia.org/wiki/", options: { encoding: "UTF-8", headerLines: 1, ignoreLines: -1, skipDataLines: 0, limit: -1, blankSpanningCells: true, includeRawTemplates: false, parseReferences: true, storeBlankRows: true, storeBlankColumns: true, wikiUrl: null, reconService: null, tableIndexes: [] } })

test("MARC uses XML record paths and creates only from the current path preview", async () => {
  const d = mount("library.mrc")
  const path = ["collection", "record"]
  await d.settle(last(d, "inspect"), { format: "marc", elements: [{ path, count: 3 }, { path: [...path, "datafield"], count: 8 }], options: { recordPath: path, limit: -1, trimStrings: false, guessCellValueTypes: false, storeEmptyStrings: true } })
  assert.match(text(d.view), /MARC/)
  assert.ok(field(d, "Load at most records"))
  assert.ok(nodes(d.view).some((n) => n.props?.title === "One record can expand into several rows."))
  assert.doesNotMatch(text(d.view), /Character encoding|Header lines|Skip data lines|Store blank rows/)
  await d.advance()
  const stale = last(d, "preview_import")
  assert.equal(stale.args.format, "marc")
  d.change(field(d, "Each record is an element"), JSON.stringify([...path, "datafield"]))
  await d.settle(stale, preview("STALE"))
  assert.doesNotMatch(text(d.view), /STALE/)
  assert.equal(create(d).props.disabled, true)
  await d.advance()
  await d.settle(last(d, "preview_import"), preview())
  d.invoke(create(d), "onClick")
  await Promise.resolve()
  assert.equal(d.created[0].format, "marc")
  assert.deepEqual(Array.from(d.created[0].options.recordPath), [...path, "datafield"])
  d.unmount()
})

test("Wikitext table and parser settings reach preview and Create, with link resolution opt in", async () => {
  const d = mount("tables.wiki")
  await d.settle(last(d, "inspect"), wikiInfo())
  assert.equal(field(d, "Resolve wiki links").props.checked, false)
  assert.equal(field(d, "Blank cells covered").props.checked, true)
  assert.match(text(d.view), /not expanded|OxRefine option/)
  assert.doesNotMatch(text(d.view), /Parse numbers and dates|Blank cells as empty|Trim whitespace/)
  assert.equal(d.requests.filter((r) => r.cmd === "wikitext_tables").length, 0, "initial inspect metadata is reused")
  await d.advance()
  const stale = last(d, "preview_import")
  d.change(field(d, "Tables to import"), "1")
  d.check(field(d, "Keep raw template syntax"), true)
  await d.settle(stale, preview("OLD TABLE"))
  assert.equal(create(d).props.disabled, true)
  await d.advance()
  const current = last(d, "preview_import")
  assert.deepEqual(Array.from(current.args.options.tableIndexes), [1])
  assert.equal(current.args.options.includeRawTemplates, true)
  assert.equal(current.args.options.wikiUrl, null)
  await d.settle(current, preview())
  d.invoke(create(d), "onClick")
  await Promise.resolve()
  assert.deepEqual(Array.from(d.created[0].options.tableIndexes), [1])
  assert.equal(d.created[0].format, "wikitext")
  d.unmount()
})

test("manual Wikitext selection refreshes table metadata and discards older encoding and source replies", async () => {
  const d = mount("clipboard.txt")
  await d.settle(last(d, "inspect"), { format: "lines", options: { encoding: "UTF-8", headerLines: 0, limit: -1 }, files: [{ name: "one.txt", selected: true, size: 20 }, { name: "two.txt", selected: false, size: 30 }] })
  const wikiRadio = nodes(nodes(d.view).find((n) => n.type === "label" && /Wikitext tables/.test(text(n)))).find((n) => n.type === "input")
  d.invoke(wikiRadio, "onChange")
  const oldMetadata = last(d, "wikitext_tables")
  assert.equal(oldMetadata.args.format, "wikitext")
  d.change(field(d, "Character encoding"), "windows-1252")
  const currentMetadata = last(d, "wikitext_tables")
  await d.settle(oldMetadata, { tables: [{ ...tables[0], caption: "STALE METADATA" }] })
  assert.doesNotMatch(text(d.view), /STALE METADATA/)
  await d.settle(currentMetadata, { tables })
  d.change(field(d, "Tables to import"), "1")
  d.check(field(d, "two.txt"), true)
  const nextMetadata = last(d, "wikitext_tables")
  assert.deepEqual(Array.from(nextMetadata.args.options.files), ["one.txt", "two.txt"])
  await d.settle(nextMetadata, { tables: [tables[0]] })
  assert.match(text(d.view), /selected table is not in this source/)
  await d.advance()
  await d.settle(last(d, "preview_import"), preview())
  assert.equal(create(d).props.disabled, true, "changing the source cannot silently import all tables")
  d.change(field(d, "Tables to import"), "all")
  await d.advance()
  await d.settle(last(d, "preview_import"), preview())
  assert.equal(create(d).props.disabled, false)
  d.update({ path: "another.wiki" })
  assert.equal(create(d).props.disabled, true)
  d.unmount()
})

test("empty Wikitext has a recovery message and cannot create an empty project", async () => {
  const d = mount("empty.wikitext")
  await d.settle(last(d, "inspect"), { ...wikiInfo(), tables: [] })
  await d.advance()
  await d.settle(last(d, "preview_import"), { columns: [], rows: [] })
  assert.match(text(d.view), /No Wikitext tables found/)
  assert.equal(create(d).props.disabled, true)
  d.invoke(create(d), "onClick")
  assert.equal(d.created.length, 0)
  d.unmount()
})

test("a successful preview cannot hide a failed Wikitext table inspection", async () => {
  const d = mount("table.txt")
  await d.settle(last(d, "inspect"), { format: "lines", options: { encoding: "UTF-8", headerLines: 0 } })
  const radio = nodes(nodes(d.view).find((n) => n.type === "label" && /Wikitext tables/.test(text(n)))).find((n) => n.type === "input")
  d.invoke(radio, "onChange")
  await d.fail(last(d, "wikitext_tables"), "Cannot inspect tables in this encoding")
  await d.advance()
  await d.settle(last(d, "preview_import"), preview())
  assert.match(text(d.view), /Cannot inspect tables in this encoding/)
  assert.equal(create(d).props.disabled, true)
  d.unmount()
})

test("wiki link resolution validates addresses before preview and can be disabled", async () => {
  const d = mount("links.wiki")
  await d.settle(last(d, "inspect"), wikiInfo())
  d.check(field(d, "Resolve wiki links"), true)
  assert.equal(field(d, "Wiki page prefix").props.value, "https://en.wikipedia.org/wiki/")
  assert.equal(field(d, "Reconciliation service").props.value, "https://wikidata.reconci.link/en/api")
  d.change(field(d, "Reconciliation service"), "https://user:password@example.org/api")
  await d.advance()
  assert.equal(last(d, "preview_import"), undefined)
  assert.match(text(d.view), /without embedded credentials/)
  assert.equal(create(d).props.disabled, true)
  d.change(field(d, "Reconciliation service"), "https://example.org/api")
  await d.advance()
  assert.equal(last(d, "preview_import").args.options.wikiUrl, "https://en.wikipedia.org/wiki/")
  assert.equal(last(d, "preview_import").args.options.reconService, "https://example.org/api")
  d.check(field(d, "Resolve wiki links"), false)
  await d.advance()
  assert.equal(last(d, "preview_import").args.options.wikiUrl, null)
  d.unmount()
})

test("PC-Axis exposes only its effective row, encoding and archive source options", async () => {
  const d = mount("statistics.zip")
  await d.settle(last(d, "inspect"), { format: "pcaxis", options: { encoding: "UTF-8", limit: -1, skipDataLines: 0, files: ["one.px"], includeFileSources: true, includeArchiveFileName: true }, files: [{ name: "one.px", selected: true, size: 20 }], archive: "statistics.zip" })
  assert.match(text(d.view), /PC-Axis|Skip data rows|Character encoding|Add a File column|Add an Archive column/)
  assert.doesNotMatch(text(d.view), /Header lines|Skip first lines|Trim whitespace|Parse numbers and dates|Store blank|Blank cells as empty/)
  d.change(field(d, "Skip data rows"), "3")
  d.change(field(d, "Load at most rows"), "0")
  d.change(field(d, "Character encoding"), "windows-1252")
  await d.advance()
  const request = last(d, "preview_import")
  assert.equal(request.args.format, "pcaxis")
  assert.equal(request.args.options.skipDataLines, 3)
  assert.equal(request.args.options.limit, 0, "PC-Axis treats zero as unlimited")
  assert.equal(request.args.options.encoding, "windows-1252")
  await d.settle(request, preview())
  d.invoke(create(d), "onClick")
  await Promise.resolve()
  assert.equal(d.created[0].options.includeArchiveFileName, true)
  d.unmount()
})
