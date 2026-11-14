import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

function home() {
  const hooks = [], requests = [], effects = [], exports = {}
  let cursor = 0, view
  const react = {
    useState(initial) {
      const i = cursor++
      hooks[i] ??= { value: initial }
      return [hooks[i].value, (value) => { hooks[i].value = value }]
    },
    useEffect(effect) {
      const i = cursor++
      if (!hooks[i]) { hooks[i] = {}; effects.push(effect) }
    },
  }
  const jsx = (type, props) => ({ type, props })
  const code = ts.transpileModule(readFileSync(new URL("../src/Home.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
  }).outputText
  vm.runInNewContext(code, {
    exports,
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx }
      if (name === "@/api") return { call: (cmd) => new Promise((resolve) => requests.push({ cmd, resolve })) }
      if (name === "@/lib/utils") return { cn: (...s) => s.filter(Boolean).join(" ") }
      return new Proxy({}, { get: (_, key) => String(key) })
    },
  })
  function render() {
    cursor = 0
    view = exports.Home({ onError(e) { throw e } })
    effects.splice(0).forEach((f) => f())
    return view
  }
  render()
  return { requests, get view() { return view },
    async load(data) { requests[0].resolve({ data }); await Promise.resolve(); render() },
    filter(value) { nodes(view).find((n) => n.type === "Input").props.onChange({ target: { value } }); render() },
  }
}
function nodes(n) {
  if (Array.isArray(n)) return n.flatMap(nodes)
  return n && typeof n === "object" ? [n, ...nodes(n.props?.children)] : []
}
function text(n) {
  if (Array.isArray(n)) return n.map(text).join("")
  return n && typeof n === "object" ? text(n.props?.children) : typeof n === "string" || typeof n === "number" ? String(n) : ""
}
const project = (id, stats) => ({ id, name: `Project ${id}`, created: "2026-11-01T10:00:00Z", modified: "2026-11-01T10:00:00Z", rows: 4, columns: 2, stats })
const rows = (view) => nodes(view).filter((n) => n.type === "tr" && nodes(n).some((c) => c.type === "td"))
const cells = (row) => nodes(row).filter((n) => n.type === "td")

test("Home shows full saved history and readable project file sizes, and filters locally", async () => {
  const h = home()
  assert.equal(h.requests[0].cmd, "projects")
  await h.load([project("1", { historySteps: 12, sizeBytes: 1536 }), project("2", { historySteps: 0, sizeBytes: 0 })])
  const headers = nodes(h.view).filter((n) => n.type === "th")
  assert.deepEqual(headers.map(text).slice(3, 5), ["Steps", "On disk"])
  assert.match(headers[3].props.title, /redo/)
  assert.match(headers[4].props.title, /original archives/)
  const first = cells(rows(h.view)[0])
  assert.equal(text(first[3]), "12")
  assert.equal(text(first[4]), `${(1.5).toLocaleString(undefined, { maximumFractionDigits: 1 })} KiB`)
  assert.equal(first[4].props.title, `${(1536).toLocaleString()} bytes in project files`)
  assert.equal(text(cells(rows(h.view)[1])[3]), "0", "known zero is distinct from missing")
  assert.equal(text(cells(rows(h.view)[1])[4]), "0 B")
  h.filter("Project 2")
  assert.equal(rows(h.view).length, 1)
  assert.equal(h.requests.length, 1, "filtering must not repeat file scans or history reads")
})

test("missing or unavailable statistics never display a misleading zero", async () => {
  const h = home()
  await h.load([project("old"), project("failed", { historySteps: null, sizeBytes: null })])
  for (const row of rows(h.view)) {
    const rowCells = cells(row)
    assert.equal(text(rowCells[3]), "Unavailable")
    assert.equal(text(rowCells[4]), "Unavailable")
    assert.match(rowCells[3].props.title, /could not be counted/)
    assert.match(rowCells[4].props.title, /could not be measured/)
  }
})
