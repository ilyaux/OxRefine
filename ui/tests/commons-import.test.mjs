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
  let nextTimer = 0, cursor = 0, dirty = true, view, closed = 0, create = async () => {}
  const call = (cmd, args, options) => { const request = { cmd, args, options, ...deferred() }; commands.push(request); return request.promise }
  vm.runInNewContext(code("../src/commonsImport.ts"), {
    exports: helpers, require: () => ({ call }),
    setTimeout: (callback) => { const id = ++nextTimer; timers.set(id, callback); return id }, clearTimeout: (id) => timers.delete(id),
  })
  const react = {
    useState(initial) {
      const i = cursor++; hooks[i] ??= { value: typeof initial === "function" ? initial() : initial }
      return [hooks[i].value, (value) => { hooks[i].value = typeof value === "function" ? value(hooks[i].value) : value; dirty = true }]
    },
    useRef(initial) { const i = cursor++; return (hooks[i] ??= { current: initial }) },
    useEffect(callback, deps) { const i = cursor++; if (!hooks[i]) { hooks[i] = { deps }; effects.push(() => { hooks[i].cleanup = callback() }) } },
  }
  vm.runInNewContext(code("../src/CommonsImportDialog.tsx"), {
    exports, crypto: { randomUUID: () => "request-ui" },
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx: (type, props) => ({ type, props }), jsxs: (type, props) => ({ type, props }) }
      if (name === "@/api") return { call }
      if (name === "@/commonsImport") return helpers
      if (name === "@/components/ui/toast") return { toast: { add: (notice) => notices.push(notice) } }
      return new Proxy({}, { get: (_, key) => String(key) })
    },
  })
  function render() {
    while (dirty) { dirty = false; cursor = 0; view = exports.CommonsImportDialog({ onCreate: (args) => create(args), onClose: () => { closed++ } }); effects.splice(0).forEach((effect) => effect()) }
  }
  render()
  const find = (predicate) => nodes(view).find(predicate)
  return { commands, timers, helpers, notices, render,
    get view() { return view }, get closed() { return closed },
    set onCreate(callback) { create = callback },
    input(id, value) { find((node) => node.props?.id === id || node.props?.["aria-label"] === id).props.onChange({ target: { value } }); render() },
    click(label) { const result = find((node) => ["CellButton", "StripAction"].includes(node.type) && text(node).trim() === label).props.onClick(); render(); return result },
    disabled(label) { return find((node) => ["CellButton", "StripAction"].includes(node.type) && text(node).trim() === label).props.disabled },
    close() { find((node) => node.type === "FullView").props.onClose(); render() },
    unmount() { hooks.forEach((hook) => hook.cleanup?.()) },
    nextPoll() { const [id, callback] = timers.entries().next().value; timers.delete(id); callback() },
  }
}

const snapshot = (overrides = {}) => ({ id: "snapshot-1", requestId: "request-ui", total: 1, columns: ["File", "Wikitext"], rows: [[0, ["File:<Bird>.jpg", "{{Description}} <script>text</script>"], false, false, "ss", null, { 0: { j: "matched", m: { id: "M42" } } }]], categories: [{ name: "Category:Birds", depth: 0 }], requests: 2, warnings: [], expiresInSeconds: 1800, ...overrides })

test("Commons titles preserve commas/underscores and validate only supported category/depth bounds", () => {
  const h = harness()
  const parsed = h.helpers.commonsSource(" Category:A,B\nC_D ", "1")
  assert.deepEqual(JSON.parse(JSON.stringify(parsed.categories)), [{ name: "Category:A,B", depth: 1 }, { name: "C_D", depth: 1 }])
  for (const depth of ["", "-1", "1.5", "11", "NaN"]) assert.match(h.helpers.commonsSource("Birds", depth).error, /0 to 10/)
  assert.match(h.helpers.commonsSource("https://commons.wikimedia.org/wiki/Category:Birds", "0").error, /titles rather than web/)
  assert.match(h.helpers.commonsSource(Array(21).fill("Birds").join("\n"), "0").error, /20 categories/)
  assert.equal(h.commands.length, 0, "typing or rendering must not fetch Commons")
})

test("browse renders filename/M-id as text, name edits keep snapshot, source edits invalidate create", async () => {
  const h = harness(); let created
  h.onCreate = async (args) => { created = args }
  h.input("commons-categories", "Category:Birds")
  assert.equal(h.commands.length, 0)
  const browsing = h.click("Browse and preview")
  const request = h.commands.find((command) => command.cmd === "commons_prepare")
  assert.equal(request.args.params.requestId, "request-ui")
  assert.equal(request.args.params.includeWikitext, false)
  h.commands.find((command) => command.cmd === "processes").resolve({ data: [] })
  request.resolve({ data: snapshot() }); await browsing; h.render()
  assert.match(text(h.view), /File:<Bird>\.jpg/)
  assert.match(text(h.view), /M42/)
  assert.equal(nodes(h.view).find((node) => node.type === "a").props.href, "https://commons.wikimedia.org/entity/M42")
  assert.equal(nodes(h.view).some((node) => node.props?.dangerouslySetInnerHTML), false)
  assert.equal(created, undefined)
  h.input("Commons project name", "My Commons data")
  assert.equal(h.disabled("Create project"), false)
  h.input("commons-depth", "1")
  assert.equal(h.disabled("Create project"), true)
  assert.equal(h.commands.filter((command) => command.cmd === "commons_prepare").length, 1)
  h.input("commons-depth", "0")
  await h.click("Create project")
  assert.deepEqual(JSON.parse(JSON.stringify(created)), { id: "snapshot-1", name: "My Commons data" })
})

test("stop before job capture cancels only its later exact marker, never another job or id0", async () => {
  const h = harness()
  const work = h.helpers.prepareCommons([{ name: "Birds", depth: 0 }], false, "exact-request")
  work.stop()
  h.commands.find((command) => command.cmd === "processes").resolve({ data: [{ id: 77, state: "running", stages: [{ label: "Commons request another-request" }] }] })
  await tick()
  assert.equal(h.commands.filter((command) => command.cmd === "cancel_process").length, 0)
  h.nextPoll()
  h.commands.filter((command) => command.cmd === "processes").at(-1).resolve({ data: [{ id: 23, state: "running", stages: [{ label: "Commons request exact-request" }] }] })
  await tick()
  const cancel = h.commands.find((command) => command.cmd === "cancel_process")
  assert.equal(cancel.args.id, 23)
  assert.equal(cancel.options.quiet, true)
  cancel.resolve({ data: null })
  h.commands.find((command) => command.cmd === "commons_prepare").resolve({ data: snapshot() }); await work.promise
  work.stop()
  assert.equal(h.commands.filter((command) => command.cmd === "cancel_process").length, 1)
})

test("close discards a late prepare reply; create holds the view until success or failure", async () => {
  const h = harness()
  h.input("commons-categories", "Birds")
  const pending = h.click("Browse and preview")
  h.close(); assert.equal(h.closed, 1)
  h.commands.find((command) => command.cmd === "commons_prepare").resolve({ data: snapshot() }); await pending; h.render()
  assert.equal(h.disabled("Create project"), true)
  assert.match(h.notices[0].description, /No project was created/)
  h.unmount()

  const creating = harness(); const wait = deferred()
  creating.input("commons-categories", "Birds")
  const browse = creating.click("Browse and preview")
  creating.commands.find((command) => command.cmd === "commons_prepare").resolve({ data: snapshot() }); await browse; creating.render()
  creating.onCreate = () => wait.promise
  const save = creating.click("Create project")
  creating.close(); assert.equal(creating.closed, 0)
  wait.reject(new Error("Could not save project")); await save; creating.render()
  assert.match(text(creating.view), /Could not save project/)
  creating.close(); assert.equal(creating.closed, 1)
})

test("prepare errors and late preview pages cannot create partial/stale data", async () => {
  const h = harness()
  h.input("commons-categories", "Birds")
  const failed = h.click("Browse and preview")
  h.commands.find((command) => command.cmd === "commons_prepare").reject(new Error("Commons HTTP429; try again later"))
  await failed; h.render()
  assert.match(text(h.view), /HTTP429/)
  assert.equal(h.disabled("Create project"), true)
  const browse = h.click("Browse and preview")
  h.commands.filter((command) => command.cmd === "commons_prepare").at(-1).resolve({ data: snapshot({ total: 21, rows: Array.from({ length: 20 }, (_, i) => [i, [`File:${i}.jpg`], false, false, "s"]) }) })
  await browse; h.render()
  const page = h.click("Next")
  const request = h.commands.find((command) => command.cmd === "commons_preview")
  assert.deepEqual(JSON.parse(JSON.stringify(request.args)), { id: "snapshot-1", start: 20, limit: 20 })
  h.input("commons-categories", "New category")
  request.resolve({ data: snapshot({ total: 21, rows: [[20, ["File:last.jpg"], false, false, "s"]] }) }); await page; h.render()
  assert.equal(h.disabled("Create project"), true)
  assert.doesNotMatch(text(h.view), /File:last/)
  h.unmount()
})
