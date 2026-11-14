import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

function grid() {
  const hooks = [], requests = [], actions = [], applied = [], listeners = new Map()
  let cursor = 0, dirty = true, effects = [], view
  const scroller = { scrollTop: 0, clientHeight: 600,
    addEventListener: (name, f) => listeners.set(name, f),
    removeEventListener: (name) => listeners.delete(name), querySelector: () => null }
  const react = {
    useRef(initial) {
      const i = cursor++
      return hooks[i] ??= { current: i === 0 ? scroller : i === 1 ? { offsetHeight: 40 } : initial }
    },
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
  react.useLayoutEffect = react.useEffect
  const exports = {}, jsx = (type, props) => ({ type, props })
  const code = ts.transpileModule(readFileSync(new URL("../src/Grid.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
  }).outputText
  vm.runInNewContext(code, {
    exports,
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx }
      if (name === "@/api") return { call: (cmd, args) => new Promise((resolve, reject) => requests.push({ cmd, args, resolve, reject })) }
      if (name === "cn") return { cn: (...s) => s.filter(Boolean).join(" ") }
      return new Proxy({}, { get: (_, key) => String(key) })
    },
    ResizeObserver: class { observe() {} disconnect() {} },
  })
  const props = { columns: ["place"], engine: { facets: [], mode: "row-based" }, version: "project-one:0",
    onTotal: () => {}, onAction: (a) => actions.push(a), onApply: async (op) => { applied.push(op) } }
  function flush() {
    let count = 0
    while (dirty) {
      assert(++count < 20, "grid settles")
      dirty = false; cursor = 0
      view = exports.Grid(props)
      const queued = effects; effects = []
      queued.forEach((f) => f())
    }
  }
  flush()
  return {
    requests, actions, applied,
    get view() { return view },
    click(node) { node.props.onClick(); flush() },
    update(next) { Object.assign(props, next); dirty = true; flush() },
    scroll(top) { scroller.scrollTop = top; listeners.get("scroll")(); flush() },
    async settle(request, data) { request.resolve({ data, ms: 1 }); await Promise.resolve(); await Promise.resolve(); flush() },
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
const marked = (d) => nodes(d.view).filter((n) => n.type === "span" && /decoration-warn/.test(n.props.className ?? ""))
const clusterLink = (d) => nodes(d.view).find((n) => n.type === "button" && /^Cluster /.test(n.props["aria-label"] ?? ""))
const hints = (values = 2) => ({ method: "fingerprint", textOnly: true, rows: 3, columns: [{ column: "place", values, groups: 1, cells: 2 }] })
const row = (index, value, type = "s", marked = true) => [index, [value], false, false, type, null, null, marked ? [0] : []]

test("fingerprint variants link to Cluster and only text cells are marked, without loading again on scroll", async () => {
  const d = grid(), firstRows = d.requests.find((r) => r.cmd === "rows")
  assert.equal(firstRows.args.params.clusterHints, true)
  await d.settle(firstRows, { total: 300, rows: [row(0, "New York"), row(1, "York New"), row(2, "1", "n")] })
  assert.equal(marked(d).length, 0, "cell hints wait for their matching view summary")
  await d.settle(d.requests.find((r) => r.cmd === "cluster_hints"), hints())
  assert.deepEqual(marked(d).map(text), ["New York", "York New"])
  assert.match(text(clusterLink(d)), /2\s+variants.*1\s+group/)
  d.click(clusterLink(d))
  assert.equal(d.actions[0].kind, "cluster")
  assert.equal(d.actions[0].column, "place")
  d.scroll(6500)
  assert.equal(d.requests.filter((r) => r.cmd === "cluster_hints").length, 1)
  assert.ok(d.requests.some((r) => r.cmd === "rows" && r.args.start === 200), "scrolling loads another row page")
  assert.equal(d.applied.length, 0, "suggestions do not change data")
  d.unmount()
})

test("filter, history and project changes hide old hints and discard late replies", async () => {
  const d = grid(), initial = d.requests.find((r) => r.cmd === "cluster_hints")
  await d.settle(initial, hints())
  await d.settle(d.requests.find((r) => r.cmd === "rows"), { total: 2, rows: [row(0, "New York"), row(1, "York New")] })
  assert.equal(marked(d).length, 2)
  d.update({ version: "project-one:0:filtered", engine: { mode: "row-based", facets: [{ type: "text", columnName: "place", query: "New" }] } })
  assert.equal(clusterLink(d), undefined)
  assert.equal(marked(d).length, 0)
  const filtered = d.requests.filter((r) => r.cmd === "cluster_hints").at(-1)
  assert.equal(filtered.args.engine.facets[0].query, "New")
  d.update({ version: "project-one:1:filtered" })
  const transformed = d.requests.filter((r) => r.cmd === "cluster_hints").at(-1)
  d.update({ version: "project-two:1:filtered" })
  const otherProject = d.requests.filter((r) => r.cmd === "cluster_hints").at(-1)
  await d.settle(filtered, hints(99))
  await d.settle(transformed, hints(88))
  assert.equal(clusterLink(d), undefined, "previous project/filter replies cannot restore hints")
  await d.settle(otherProject, hints(0))
  assert.equal(clusterLink(d), undefined)
  d.update({ version: "project-one:0:filtered" })
  await d.settle(d.requests.filter((r) => r.cmd === "cluster_hints").at(-1), hints())
  assert.ok(clusterLink(d), "undo and project return request fresh counts")
  d.unmount()
})
