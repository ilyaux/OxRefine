import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

const compile = (file) => ts.transpileModule(readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8"), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
}).outputText
const api = {}
vm.runInNewContext(compile("api.ts"), { exports: api, require: () => ({}), window: {}, location: { search: "" },
  URLSearchParams, sessionStorage: { getItem: () => null }, crypto: { randomUUID: () => "test" } })
const { listFacet, rangeFacet, timeFacet } = api
const textFacet = (column) => ({ type: "text", name: column, columnName: column, query: "", mode: "text", caseSensitive: false, invert: false })
const plain = (value) => JSON.parse(JSON.stringify(value))
const listData = { choices: [["Zulu", 10, "Zulu"], ["Alpha", 5, "Alpha"]], distinct: 2, blank: 1, error: 1 }
const rangeData = { min: 0, max: 10, step: 5, bins: [2, 1], baseBins: [3, 2], numericCount: 3, nonNumericCount: 2, blankCount: 1, errorCount: 1 }
const minDate = Date.UTC(2026, 0, 1), maxDate = Date.UTC(2026, 0, 3)
const timeData = { min: minDate, max: maxDate, step: 864e5, bins: [2, 1], baseBins: [3, 2], timeCount: 3, nonTimeCount: 2, blankCount: 1, errorCount: 1, baseTimeCount: 5 }

// Render the real nested components with keyed hook lifetimes; replace only React and the browser.
function rail(facets, mode = "record-based") {
  const instances = new Map(), timers = new Map(), overrides = new Map(), changes = [], edited = [], limits = [], requests = []
  let active, cursor, dirty = true, view, effects = [], id = 0, timerId = 0
  let engine = { mode, facets }
  const history = ["Existing step"]
  const react = {
    useState(initial) {
      const hooks = active, i = cursor++
      hooks[i] ??= { value: typeof initial === "function" ? initial() : initial }
      return [hooks[i].value, (next) => {
        const value = typeof next === "function" ? next(hooks[i].value) : next
        if (!Object.is(value, hooks[i].value)) { hooks[i].value = value; dirty = true }
      }]
    },
    useId() {
      const hooks = active, i = cursor++
      hooks[i] ??= { value: `facet-${++id}` }
      return hooks[i].value
    },
    useEffect(effect, deps) {
      const hooks = active, i = cursor++, previous = hooks[i]
      if (!previous || deps.some((value, j) => !Object.is(value, previous.deps[j]))) {
        hooks[i] = { deps, cleanup: previous?.cleanup }
        effects.push(() => { hooks[i].cleanup?.(); hooks[i].cleanup = effect() })
      }
    },
  }
  const exports = {}, jsx = (type, props, key) => ({ type, props, key })
  vm.runInNewContext(compile("Sidebar.tsx"), {
    exports,
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx, Fragment: "Fragment" }
      if (name === "@/api") return { ...api, call: (...args) => { requests.push(args); throw Error("unexpected API call") } }
      if (name === "@/lib/utils") return { cn: (...s) => s.filter(Boolean).join(" "), maxOf: (values, fallback) => Math.max(fallback, ...values) }
      return new Proxy({}, { get: (_, key) => String(key) })
    },
    setTimeout(f) { const key = ++timerId; timers.set(key, f); return key }, clearTimeout(key) { timers.delete(key) },
  })
  const data = (facet) => overrides.has(facet.name) ? overrides.get(facet.name) : facet.type === "list" ? listData : facet.type === "range" ? rangeData : facet.type === "timerange" ? timeData : null
  const props = { columns: [], onChange(next) { changes.push(next); engine = { ...engine, facets: next }; dirty = true },
    onEditChoice(facet, choice) { edited.push({ facet, choice }) }, onLimit(n) { limits.push(n) }, onAdd() {} }
  function render(node, path, seen) {
    if (Array.isArray(node)) return node.map((child, i) => render(child, `${path}/${child?.key ?? i}`, seen))
    if (!node || typeof node !== "object") return node
    if (typeof node.type === "function") {
      const key = `${path}/${node.type.name}`
      seen.add(key)
      active = instances.get(key) ?? []
      instances.set(key, active)
      cursor = 0
      return render(node.type(node.props), key, seen)
    }
    return { ...node, props: { ...node.props, children: render(node.props?.children, `${path}/${node.type}`, seen) } }
  }
  function flush() {
    let passes = 0
    while (dirty) {
      assert(++passes < 15, "facet rail settles")
      dirty = false
      props.facets = engine.facets; props.data = engine.facets.map(data)
      const seen = new Set()
      view = render(jsx(exports.Facets, props), "root", seen)
      for (const [key, hooks] of instances) if (!seen.has(key)) { hooks.forEach((h) => h.cleanup?.()); instances.delete(key) }
      const queued = effects; effects = []; queued.forEach((f) => f())
    }
  }
  flush()
  return { changes, edited, limits, requests, history, get engine() { return engine }, get view() { return view },
    click(node) { assert.ok(node, "control exists"); node.props.onClick(); flush() },
    change(node, value) { node.props.onChange({ target: { value } }); flush() },
    setData(name, value) { overrides.set(name, value); dirty = true; flush() },
    advance() { const queued = [...timers.values()]; timers.clear(); queued.forEach((f) => f()); flush() },
  }
}
function nodes(node, visible = false) {
  if (Array.isArray(node)) return node.flatMap((child) => nodes(child, visible))
  if (!node || typeof node !== "object" || visible && node.props?.hidden) return []
  return [node, ...nodes(node.props?.children, visible)]
}
function text(node) {
  if (Array.isArray(node)) return node.map(text).join("")
  return node && typeof node === "object" ? text(node.props?.children) : typeof node === "string" || typeof node === "number" ? String(node) : ""
}
const section = (rail, name) => nodes(rail.view).find((n) => n.type === "section" && n.props["aria-label"] === `Facet ${name}`)
const button = (view, label) => nodes(view).find((n) => (n.type === "Button" || n.type === "button") && (n.props["aria-label"] === label || text(n) === label))
const bound = (view, label) => nodes(view).find((n) => n.type === "input" && n.props["aria-label"] === label)
const toggle = (view) => nodes(view).find((n) => n.props?.["aria-controls"])

test("each facet collapses accessibly without changing filters, header controls or mounted inputs", () => {
  const facets = [listFacet("choice"), textFacet("query"), rangeFacet("number"), timeFacet("date")]
  const r = rail(facets)
  for (const facet of facets) {
    let view = section(r, facet.name)
    const initial = toggle(view), bodyId = initial.props["aria-controls"]
    const controls = nodes(view).filter((n) => n.type === "Button").map((n) => n.props["aria-label"])
    assert.equal(initial.props["aria-expanded"], true)
    assert.ok(nodes(view).find((n) => n.props?.id === bodyId && n.props.hidden === false))
    r.click(initial)
    view = section(r, facet.name)
    assert.equal(toggle(view).props["aria-expanded"], false)
    assert.equal(toggle(view).props["aria-controls"], bodyId)
    assert.ok(nodes(view).some((n) => n.props?.id === bodyId && n.props.hidden === true))
    assert.deepEqual(nodes(view).filter((n) => n.type === "Button").map((n) => n.props["aria-label"]), controls)
    assert.equal(nodes(view, true).filter((n) => n.type === "input" || n.type === "Input" || n.props?.role === "checkbox").length, 0)
    r.click(toggle(view))
  }
  assert.equal(r.changes.length, 0)
  assert.equal(r.engine.facets, facets)
  assert.equal(r.engine.mode, "record-based")
  assert.deepEqual(r.requests, [])
  assert.deepEqual(r.history, ["Existing step"])
})

test("duplicate sibling removal preserves collapse and list sort instead of moving the first sibling's state", () => {
  const r = rail([{ ...listFacet("choice"), name: "First" }, { ...listFacet("choice"), name: "Second" }])
  r.click(button(section(r, "Second"), "Name"))
  r.click(toggle(section(r, "Second")))
  const id = toggle(section(r, "Second")).props["aria-controls"]
  r.click(button(section(r, "First"), "Remove the First facet"))
  assert.equal(toggle(section(r, "Second")).props["aria-expanded"], false)
  assert.equal(toggle(section(r, "Second")).props["aria-controls"], id)
  r.click(toggle(section(r, "Second")))
  assert.equal(button(section(r, "Second"), "Name").props["aria-pressed"], true)
  const choices = nodes(section(r, "Second")).filter((n) => n.props?.role === "checkbox")
  assert.match(text(choices[0]), /^Alpha/)
  r.click(choices[0])
  assert.equal(r.engine.facets[0].selection[0].v.v, "Alpha")
  r.click(button(section(r, "Second"), "Edit Alpha"))
  assert.equal(r.edited[0].choice, "Alpha")
})

test("collapse retains text query, numeric/date drafts, selections and too-many-choice actions", () => {
  const r = rail([textFacet("query"), rangeFacet("number"), timeFacet("date"), listFacet("choice")])
  r.change(nodes(section(r, "query")).find((n) => n.type === "Input"), "pending")
  r.click(toggle(section(r, "query")))
  r.advance()
  assert.equal(r.engine.facets[0].query, "pending", "the mounted query debounce still commits through the usual callback")
  r.click(toggle(section(r, "query")))
  assert.equal(nodes(section(r, "query")).find((n) => n.type === "Input").props.value, "pending")
  for (const name of ["number", "date"]) {
    r.change(bound(section(r, name), `${name}: from`), "uncommitted draft")
    r.click(toggle(section(r, name))); r.click(toggle(section(r, name)))
    assert.equal(bound(section(r, name), `${name}: from`).props.value, "uncommitted draft")
  }
  const choice = nodes(section(r, "choice")).find((n) => n.props?.role === "checkbox")
  r.click(choice); r.click(toggle(section(r, "choice"))); r.click(toggle(section(r, "choice")))
  assert.equal(nodes(section(r, "choice")).find((n) => n.props?.role === "checkbox").props["aria-checked"], true)
  r.setData("choice", { distinct: 5000, tooMany: true, limit: 2000, blank: 0, error: 0 })
  r.click(toggle(section(r, "choice"))); r.click(toggle(section(r, "choice")))
  r.click(button(section(r, "choice"), "Show all"))
  assert.deepEqual(r.limits, [5000])
  assert.equal(r.engine.mode, "record-based")
  assert.deepEqual(r.history, ["Existing step"])
})

test("range and timeline reset reuse factory defaults, retain custom names/expressions and clear stale drafts", () => {
  for (const [factory, name, column, expression, data] of [[rangeFacet, "Custom number", "number", "value.toNumber()", rangeData], [timeFacet, "Custom date", "date", "value.toDate()", timeData]]) {
    const fresh = { ...factory(column, expression), name }
    const narrowed = { ...fresh, from: data.min + 1, to: data.max - 1, selectBlank: false, selectError: false,
      ...(fresh.type === "range" ? { selectNumeric: false, selectNonNumeric: false } : { selectTime: false, selectNonTime: false }) }
    const r = rail([narrowed])
    r.change(bound(section(r, name), `${column}: from`), "bad draft")
    r.click(toggle(section(r, name)))
    r.click(button(section(r, name), `Reset the ${name} facet`))
    assert.deepEqual(plain(r.engine.facets[0]), plain(fresh))
    assert.equal(toggle(section(r, name)).props["aria-expanded"], false, "reset does not reopen a collapsed facet")
    r.click(toggle(section(r, name)))
    assert.notEqual(bound(section(r, name), `${column}: from`).props.value, "bad draft")
    assert.ok(nodes(section(r, name)).filter((n) => n.props?.role === "checkbox").every((n) => n.props["aria-checked"] === true))
    r.setData(name, { ...data, min: data.min - 100, max: data.max + 100 })
    assert.deepEqual(plain(r.engine.facets[0]), plain(fresh), "new facet data must not narrow a reset filter to old histogram bounds")
    r.change(bound(section(r, name), `${column}: from`), "another draft")
    r.click(button(section(r, name), `Reset the ${name} facet`))
    assert.notEqual(bound(section(r, name), `${column}: from`).props.value, "another draft", "reset also clears drafts when filter was already at defaults")
    r.setData(name, null)
    r.click(button(section(r, name), `Reset the ${name} facet`))
    assert.deepEqual(plain(r.engine.facets[0]), plain(fresh), "reset is available while facet data is missing")
    assert.equal(r.engine.mode, "record-based")
    assert.deepEqual(r.history, ["Existing step"])
    assert.deepEqual(r.requests, [])
  }
})

test("dense numeric and timeline histograms keep every SVG bar positive and inside the plot", () => {
  const bins = Array.from({ length: 452 }, (_, i) => i % 3 + 1)
  for (const [facet, data] of [[rangeFacet("number"), rangeData], [timeFacet("date"), timeData]]) {
    const r = rail([facet])
    r.setData(facet.name, { ...data, bins, baseBins: bins, step: (data.max - data.min) / bins.length })
    const svg = nodes(section(r, facet.name)).find((node) => node.type === "svg")
    assert.ok(svg, `the ${facet.type} panel renders its shared Histogram`)
    const bars = nodes(svg).filter((node) => node.type === "rect")
    assert.equal(bars.length, bins.length * 2, "both baseline and matching-count bars must remain visible")
    for (const { props: { x, width } } of bars) {
      assert.ok(Number.isFinite(width) && width > 0, "each dense bin needs a real positive width")
      assert.ok(Number.isFinite(x) && x >= 0 && x + width <= 100, "bar geometry stays inside the 0..100 plot domain")
    }
  }
})
