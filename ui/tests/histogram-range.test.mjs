import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

function histogram(overrides = {}) {
  const hooks = [], commits = [], captures = [], released = [], held = new Set(), exports = {}
  let cursor = 0, dirty = true, view, effects = []
  const target = { getBoundingClientRect: () => ({ left: 10, width: 200 }), focus() {},
    setPointerCapture(id) { held.add(id); captures.push(id) }, hasPointerCapture: (id) => held.has(id),
    releasePointerCapture(id) { held.delete(id); released.push(id) } }
  const react = {
    useRef(initial) { const i = cursor++; return hooks[i] ??= { current: initial } },
    useState(initial) {
      const i = cursor++; hooks[i] ??= { value: initial }
      return [hooks[i].value, (next) => { if (!Object.is(next, hooks[i].value)) { hooks[i].value = next; dirty = true } }]
    },
    useEffect(effect, deps) {
      const i = cursor++, old = hooks[i]
      if (!old || deps.some((value, j) => !Object.is(value, old.deps[j]))) {
        hooks[i] = { deps, cleanup: old?.cleanup }
        effects.push(() => { hooks[i].cleanup?.(); hooks[i].cleanup = effect() })
      }
    },
  }
  const jsx = (type, props) => ({ type, props })
  const code = ts.transpileModule(readFileSync(new URL("../src/HistogramRange.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
  }).outputText
  vm.runInNewContext(code, { exports, require(name) {
    if (name === "react") return react
    if (name === "react/jsx-runtime") return { jsx, jsxs: jsx }
    if (name === "@/lib/utils") return { cn: (...s) => s.filter(Boolean).join(" "), maxOf: (values, fallback) => Math.max(fallback, ...values) }
    throw Error(name)
  } })
  const props = { label: "Histogram", min: 0, max: 10, step: 1, from: 0, to: 10, bins: [1, 1], baseBins: [1, 1], ticks: [],
    revision: {}, format: String, onCommit: (from, to) => commits.push([from, to]), ...overrides }
  function flush() {
    let passes = 0
    while (dirty) {
      assert(++passes < 15, "histogram settles")
      dirty = false; cursor = 0; view = exports.HistogramRange(props)
      nodes(view).find((node) => node.type === "svg").props.ref.current = target
      const queued = effects; effects = []; queued.forEach((f) => f())
    }
  }
  const event = (clientX, extra = {}) => ({ clientX, pointerId: 1, button: 0, isPrimary: true, currentTarget: target, preventDefault() {}, ...extra })
  flush()
  return { commits, captures, released, held, target, get view() { return view },
    get svg() { return nodes(view).find((node) => node.type === "svg") },
    get control() { return nodes(view).find((node) => node.props?.onPointerMove) },
    get handles() { return nodes(view).filter((node) => node.props?.role === "slider") },
    down(x, index, extra) { (index === undefined ? this.svg : this.handles[index]).props.onPointerDown(event(x, extra)); flush() },
    move(x, extra) { this.control.props.onPointerMove(event(x, extra)); flush() },
    up(x, extra) { this.control.props.onPointerUp(event(x, extra)); flush() },
    cancel(lost = false) { this.control.props[lost ? "onLostPointerCapture" : "onPointerCancel"](event(0)); flush() },
    key(index, key) { this.handles[index].props.onKeyDown({ key, currentTarget: { dataset: { bound: index === 0 ? "from" : "to" } }, preventDefault() {} }); flush() },
    escape() { this.control.props.onKeyDown({ key: "Escape", preventDefault() {} }); flush() },
    update(next) { Object.assign(props, next); dirty = true; flush() },
    unmount() { hooks.forEach((h) => h.cleanup?.()) },
  }
}
function nodes(node) {
  if (Array.isArray(node)) return node.flatMap(nodes)
  return node && typeof node === "object" ? [node, ...nodes(node.props?.children)] : []
}
const shown = (h) => h.handles.map((handle) => handle.props["aria-valuenow"])

test("forward/reverse histogram brushes snap and commit only once on release, including the padded numeric max", () => {
  for (const [start, end] of [[50, 170], [170, 50]]) {
    const h = histogram()
    h.down(start); h.move(end)
    assert.deepEqual(shown(h), [2, 8]); assert.deepEqual(h.commits, [], "drag previews must not send facet requests")
    h.up(end); assert.deepEqual(h.commits, [[2, 8]])
    assert.deepEqual(h.captures, [1]); assert.deepEqual(h.released, [1]); assert.equal(h.held.size, 0)
    h.up(end); assert.equal(h.commits.length, 1)
  }
  const h = histogram({ max: 10.1, step: 0.1 })
  h.down(-50); h.move(400); h.up(400)
  assert.deepEqual(h.commits, [[0, 10.1]])
  assert.ok(10 >= h.commits[0][0] && 10 < h.commits[0][1], "the last real numeric max stays included by the engine's exclusive upper bound")
})

test("capture cancellation, Escape, stale facet/domain and unmount cannot commit an abandoned draft", () => {
  for (const action of [(h) => h.cancel(), (h) => h.cancel(true), (h) => h.escape(), (h) => h.update({ revision: {} }), (h) => h.update({ max: 20 })]) {
    const h = histogram(); h.down(50); h.move(170); action(h); h.up(170)
    assert.deepEqual(h.commits, []); assert.equal(h.held.size, 0)
  }
  const h = histogram(); h.down(50); h.move(170, { pointerId: 2 }); h.up(170, { pointerId: 2 })
  assert.deepEqual(h.commits, []); assert.equal(h.held.size, 1, "another pointer cannot end the active gesture")
  h.unmount(); assert.equal(h.held.size, 0)
  h.up(170); assert.deepEqual(h.commits, [])
  const click = histogram(); click.down(50); click.up(50); assert.deepEqual(click.commits, [])
})

test("both handles support keyboard ranges and pointer crossing without inverted numeric bounds", () => {
  const h = histogram({ from: 2, to: 8 })
  assert.deepEqual(h.handles.map((handle) => handle.props["aria-label"]), ["Histogram: from", "Histogram: to"])
  h.key(0, "ArrowRight"); assert.deepEqual(h.commits.at(-1), [3, 8])
  h.key(1, "ArrowLeft"); assert.deepEqual(h.commits.at(-1), [2, 7])
  h.key(0, "End"); assert.deepEqual(h.commits.at(-1), [7, 8])
  h.key(1, "Home"); assert.deepEqual(h.commits.at(-1), [2, 3])
  h.key(0, "PageUp"); assert.deepEqual(h.commits.at(-1), [7, 8])
  h.key(1, "PageDown"); assert.deepEqual(h.commits.at(-1), [2, 3])
  h.down(50, 0); h.move(210); h.up(210); assert.deepEqual(h.commits.at(-1), [8, 10])
  assert.ok(h.commits.every(([from, to]) => Number.isFinite(from) && Number.isFinite(to) && to > from))
})

test("timeline brushes preserve millisecond and inclusive upper bounds, including equal endpoints", () => {
  const min = Date.UTC(2026, 0, 1, 0, 0, 0, 123), step = 1000
  const h = histogram({ min, max: min + 4 * step, from: min, to: min + 4 * step, step, inclusive: true, format: (value) => new Date(value).toISOString() })
  h.down(10); h.move(110); h.up(110)
  assert.deepEqual(h.commits, [[min, min + 2 * step]])
  assert.equal(h.handles[0].props["aria-valuetext"], "2026-01-01T00:00:00.123Z")
  assert.ok(min + 2 * step <= h.commits[0][1], "a date at exactly to stays included")
  h.key(0, "End"); assert.deepEqual(h.commits.at(-1), [min + 4 * step, min + 4 * step])
})

test("degenerate, empty and overflowing domains never capture a pointer or expose invalid sliders", () => {
  for (const options of [{ min: 5, max: 5 }, { bins: [], baseBins: [] }, { min: -1e308, max: 1e308 }, { step: 0 }, { step: Number.MIN_VALUE }]) {
    const h = histogram(options)
    assert.equal(h.handles.length, 0); h.down(50); h.move(170); h.up(170)
    assert.deepEqual(h.captures, []); assert.deepEqual(h.commits, [])
    assert.ok(nodes(h.view).filter((node) => node.type === "rect").every((node) => Number.isFinite(node.props.width) && node.props.width > 0))
  }
  const h = histogram(); h.target.getBoundingClientRect = () => ({ left: 10, width: 0 }); h.down(50)
  assert.deepEqual(h.captures, [])
})
