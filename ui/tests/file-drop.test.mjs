import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

function source(file) {
  return ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
  }).outputText
}
const helpers = {}
vm.runInNewContext(source("../src/fileDrop.ts"), { exports: helpers })
const tick = () => new Promise(setImmediate)
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no }); return { promise, resolve, reject } }
const file = (name = "data.csv") => ({ name, webkitRelativePath: "" })
const transfer = (files, items = files.map(() => ({ kind: "file" }))) => ({ types: ["Files"], files, items, dropEffect: "none" })

function listener(options = {}) {
  const events = new Map(), errors = [], received = [], dragging = [], busy = []
  let enabled = true
  const surface = {
    addEventListener: (name, callback) => events.set(name, callback),
    removeEventListener: (name) => events.delete(name),
  }
  const controller = helpers.listenForFileDrops({ surface, canDrop: () => enabled,
    onDrop: async (value) => received.push(value), onError: (e) => errors.push(e.message), onDragging: (value) => dragging.push(value), onBusy: (value) => busy.push(value), ...options,
  })
  return { ...controller, events, errors, received, dragging, busy,
    disable() { enabled = false },
    emit(name, data) {
      let prevented = false
      events.get(name)?.({ dataTransfer: data, preventDefault() { prevented = true }, relatedTarget: null })
      return prevented
    },
  }
}

test("browser file drags are recognized before files are readable; text and URLs are untouched", async () => {
  const drop = listener()
  const over = transfer([])
  assert.equal(drop.emit("dragenter", over), true)
  assert.equal(drop.dragging.at(-1), true)
  assert.equal(drop.emit("dragover", over), true)
  assert.equal(over.dropEffect, "copy")
  assert.equal(drop.emit("drop", transfer([file()])), true)
  await tick()
  assert.equal(drop.received[0].file.name, "data.csv")
  assert.deepEqual(drop.busy, [true, false])
  for (const type of ["text/plain", "text/uri-list"]) {
    assert.equal(drop.emit("drop", { types: [type], items: [{ kind: "string" }], files: [] }), false)
  }
  drop.dispose(); assert.equal(drop.events.size, 0)
})

test("blocked projects/dialogs, multiple files and browser folders never import or navigate", async () => {
  const drop = listener()
  drop.emit("drop", transfer([file(), file("second.csv")]))
  drop.emit("drop", transfer([file("folder")], [{ kind: "file", webkitGetAsEntry: () => ({ isDirectory: true }) }]))
  drop.disable()
  assert.equal(drop.emit("drop", transfer([file()])), true, "blocked file drops still cannot navigate away")
  await tick()
  assert.equal(drop.received.length, 0)
  assert.match(drop.errors[0], /one file at a time/)
  assert.match(drop.errors[1], /Folders cannot be uploaded/)
  assert.match(drop.errors[2], /Go to Projects/)
  drop.dispose()
})

test("cancelled upload/inspection cannot stage a stale wizard or archive confirmation", async () => {
  for (const boundary of ["upload", "inspect"]) {
    const wait = deferred(), stages = [], confirmations = [], calls = []
    let pending
    const drop = listener({ onDrop: (value, current) => (pending = helpers.prepareDroppedImport(value, current, {
      upload: async () => { calls.push("upload"); return boundary === "upload" ? wait.promise : { path: "/staged/data.csv", name: "data.csv" } },
      inspect: async () => { calls.push("inspect"); return wait.promise },
      stage: (value) => stages.push(value), confirmArchive: (value) => confirmations.push(value),
    })) })
    drop.emit("drop", transfer([file()])); await tick()
    if (boundary === "upload") drop.cancel(); else drop.dispose()
    wait.resolve(boundary === "upload" ? { path: "/staged/data.csv", name: "data.csv" } : { format: "openrefine-project" })
    await pending; await tick()
    assert.equal(stages.length, 0); assert.equal(confirmations.length, 0)
    if (boundary === "upload") assert.deepEqual(calls, ["upload"])
    if (boundary === "upload") assert.equal(drop.busy.at(-1), false)
    else assert.deepEqual(drop.busy, [true], "an unmounted listener must not update component state")
    drop.dispose()
  }
})

test("busy drop is rejected without cancelling the first preview; archives require confirmation", async () => {
  const waiting = deferred(), accepted = []
  const drop = listener({ onDrop: async (value, current) => { await waiting.promise; if (current()) accepted.push(value) } })
  drop.emit("drop", transfer([file("first.csv")]))
  drop.emit("drop", transfer([file("second.csv")]))
  waiting.resolve(); await tick()
  assert.equal(accepted.length, 1); assert.equal(accepted[0].file.name, "first.csv")
  assert.match(drop.errors[0], /already being prepared/)
  const staged = [], confirmed = []
  await helpers.prepareDroppedImport({ path: "C:\\data\\project.openrefine.tar.gz" }, () => true, {
    upload: () => assert.fail("native paths are not browser uploads"), inspect: async () => ({ format: "openrefine-project" }),
    stage: (value) => staged.push(value), confirmArchive: (value) => confirmed.push(value),
  })
  assert.equal(staged.length, 0); assert.equal(confirmed[0].name, "project.openrefine.tar.gz")
  drop.dispose()
})

test("desktop native events import only drop paths and release late async listeners", async () => {
  const registration = deferred(); let native, unlistened = 0
  const drop = listener({ listenNative: (handler) => { native = handler; return registration.promise } })
  native({ type: "enter", paths: ["C:\\data\\data.csv"] }); native({ type: "over" }); native({ type: "leave" })
  assert.equal(drop.received.length, 0)
  native({ type: "drop", paths: ["C:\\data\\data.csv"] }); await tick()
  assert.equal(drop.received[0].path, "C:\\data\\data.csv")
  native({ type: "drop", paths: ["a.csv", "b.csv"] }); await tick()
  assert.match(drop.errors[0], /one file or folder/)
  drop.dispose()
  registration.resolve(async () => { unlistened++; throw new Error("already closed") }); await tick()
  assert.equal(unlistened, 1)
  native({ type: "drop", paths: ["late.csv"] }); await tick()
  assert.equal(drop.received.length, 1)
})

test("drop hook ignores toast dialog roles while Home About/Delete popup slots still block drops", () => {
  const exports = {}, effects = []
  let connected, popup = null
  vm.runInNewContext(source("../src/useFileDrop.ts"), {
    exports, window: {},
    document: { querySelector(selector) {
      assert.equal(selector, '[data-slot="dialog-content"]:not([hidden]), [data-slot="alert-dialog-content"]:not([hidden])', "toast role=dialog/alertdialog must not match the blocking selector")
      return popup
    } },
    require(name) {
      if (name === "react") return {
        useState: () => [false, () => {}], useRef: (value) => ({ current: value }), useCallback: (callback) => callback,
        useEffect: (callback) => effects.push(callback), useLayoutEffect: (callback) => callback(),
      }
      if (name === "@/api") return { isTauri: false }
      if (name === "@/fileDrop") return { listenForFileDrops: (options) => { connected = options; return { cancel() {}, dispose() {} } } }
      if (name === "@tauri-apps/api/webview") return {}
      throw new Error(`Unexpected import ${name}`)
    },
  })
  exports.useFileDrop({ enabled: true, onDrop: async () => {}, onError() {} })
  effects.forEach((effect) => effect())
  assert.equal(connected.canDrop(), true, "a notification toast must not prevent retrying a valid file")
  popup = { dataset: { slot: "dialog-content" } }
  assert.equal(connected.canDrop(), false, "Home About dialog still blocks drop")
  popup = { dataset: { slot: "alert-dialog-content" } }
  assert.equal(connected.canDrop(), false, "Home Delete confirmation still blocks drop")
  popup = null
  assert.equal(connected.canDrop(), true)
})

// Render the real App with deferred commands; the mock drop hook exposes its eligibility and cancel wiring.
function app() {
  const hooks = [], effects = [], commands = [], uploads = []
  let cursor = 0, dirty = true, view, dropOptions, ticket = 0
  const exports = {}, cancel = () => { ticket++ }
  const react = {
    useState(initial) {
      const i = cursor++; hooks[i] ??= { value: typeof initial === "function" ? initial() : initial }
      return [hooks[i].value, (value) => { hooks[i].value = typeof value === "function" ? value(hooks[i].value) : value; dirty = true }]
    },
    useEffect(callback, deps) {
      const i = cursor++, before = hooks[i]
      if (!before || deps?.some((value, k) => !Object.is(value, before.deps?.[k]))) {
        hooks[i] = { deps }; effects.push(callback)
      }
    },
    useMemo: (callback) => callback(), useCallback: (callback) => callback,
    useRef(initial) { const i = cursor++; return (hooks[i] ??= { current: initial }) },
  }
  vm.runInNewContext(`${source("../src/App.tsx")}\nexports.__chip = chip;`, { exports, window: { addEventListener() {}, removeEventListener() {} },
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx: (type, props) => ({ type, props }), jsxs: (type, props) => ({ type, props }) }
      if (name === "@/api") return {
        call: (cmd, args) => { const wait = deferred(); commands.push({ cmd, args, ...wait }); return wait.promise },
        upload: (value) => { const wait = deferred(); uploads.push({ value, ...wait }); return wait.promise },
        onProjectConflict: () => () => {},
      }
      if (name === "@/useFileDrop") return { useFileDrop: (options) => { dropOptions = options; return { dragging: false, busy: false, cancel } } }
      if (name === "@/fileDrop") return helpers
      if (name === "@/ProcessMemory") return { useProcessMemory: () => null }
      if (name === "@/updates") return { checkForUpdates() {} }
      if (name === "@/components/ui/toast") return { Toaster: "Toaster", toast: { add() {} } }
      if (name === "@/lib/utils") return { cn: (...values) => values.join(" ") }
      if (name === "../package.json") return { default: { version: "test" } }
      return new Proxy({}, { get: (_, key) => key === "ProjectName" ? { Provider: "Provider" } : String(key) })
    },
  })
  const nodes = (node) => Array.isArray(node) ? node.flatMap(nodes) : node && typeof node === "object" ? [node, ...nodes(node.props?.children)] : []
  function render() {
    let count = 0
    while (dirty) { assert(++count < 15); dirty = false; cursor = 0; view = exports.App(); effects.splice(0).forEach((effect) => effect()) }
  }
  render()
  return { commands, uploads, render,
    chip: exports.__chip,
    get enabled() { return dropOptions.enabled },
    get staged() { return nodes(view).find((node) => node.type === "ImportDialog") },
    startDrop(value) { const current = ticket; return dropOptions.onDrop(value, () => current === ticket && dropOptions.enabled) },
    open(id) { nodes(view).find((node) => node.type === "Home").props.onOpen(id); render() },
  }
}

test("App disables drops during session restore and invalidates a drop as project opening starts", async () => {
  const a = app()
  assert.equal(a.enabled, false, "restored project selection must settle before Home accepts files")
  a.commands[0].reject(new Error("no project open")); await tick(); a.render()
  assert.equal(a.enabled, true)
  const dropped = a.startDrop({ file: file() })
  a.open("saved-project")
  assert.equal(a.enabled, false, "opening project is still pending; rendered summary is not enough")
  a.uploads[0].resolve({ path: "/staged/data.csv", name: "data.csv" }); await dropped; a.render()
  assert.equal(a.commands.some((command) => command.cmd === "inspect"), false)
  assert.equal(a.staged, undefined)
})

test("App does not queue competing project selections while native/browser opening is pending", async () => {
  const a = app()
  a.open("too-early")
  assert.equal(a.commands.length, 1, "session restore must finish before a project can be selected")
  a.commands[0].reject(new Error("no project open")); await tick(); a.render()
  a.open("first")
  a.open("second")
  await tick()
  assert.equal(a.commands.filter((command) => command.cmd === "open_project").length, 1)
  a.commands[1].reject(new Error("could not open")); await tick(); a.render()
  a.open("after-error")
  assert.equal(a.commands.filter((command) => command.cmd === "open_project").length, 2, "a failed operation releases the selection guard")
})

test("numeric facet chips show the exclusive upper bound while timeline chips stay inclusive", () => {
  const a = app()
  assert.equal(a.chip({ type: "range", columnName: "n", from: 20, to: 80, selectNumeric: true, selectNonNumeric: true, selectBlank: true, selectError: true }), "n ∈ [20, 80)")
  assert.equal(a.chip({ type: "timerange", columnName: "when", from: 0, to: 86400000, selectTime: true, selectNonTime: true, selectBlank: true, selectError: true }), "when ∈ [1970-01-01, 1970-01-02]")
})
