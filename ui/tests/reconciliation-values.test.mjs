import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

// Execute the real component; only React lifecycle and its transport are replaced.
function panel() {
  const hooks = [],
    requests = [],
    applied = []
  let cursor = 0,
    dirty = true,
    effects = [],
    view
  const react = {
    useState(initial) {
      const i = cursor++
      hooks[i] ??= {
        value: typeof initial === "function" ? initial() : initial,
      }
      return [
        hooks[i].value,
        (next) => {
          const value = typeof next === "function" ? next(hooks[i].value) : next
          if (!Object.is(value, hooks[i].value)) {
            hooks[i].value = value
            dirty = true
          }
        },
      ]
    },
    useEffect(effect, deps) {
      const i = cursor++,
        previous = hooks[i]
      if (!previous || deps.some((v, j) => !Object.is(v, previous.deps[j]))) {
        hooks[i] = { deps, cleanup: previous?.cleanup }
        effects.push(() => {
          hooks[i].cleanup?.()
          hooks[i].cleanup = effect()
        })
      }
    },
  }
  const exports = {},
    jsx = (type, props) => ({ type, props })
  const code = ts.transpileModule(
    readFileSync(
      new URL("../src/ReconciliationValues.tsx", import.meta.url),
      "utf8"
    ),
    {
      compilerOptions: {
        module: ts.ModuleKind.CommonJS,
        target: ts.ScriptTarget.ES2023,
        jsx: ts.JsxEmit.ReactJSX,
      },
    }
  ).outputText
  vm.runInNewContext(code, {
    exports,
    require(name) {
      if (name === "react") return react
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx }
      if (name === "@/api")
        return {
          call: (cmd, args) =>
            new Promise((resolve, reject) =>
              requests.push({ cmd, args, resolve, reject })
            ),
        }
      if (name === "@/lib/utils")
        return { cn: (...s) => s.filter(Boolean).join(" ") }
      return new Proxy({}, { get: (_, key) => String(key) })
    },
  })
  const props = {
    column: "name",
    engine: { facets: [], mode: "record-based" },
    version: "one",
    onApply: async (op) => {
      applied.push(op)
    },
    onClose() {},
    onReconcile() {},
  }
  function flush() {
    let count = 0
    while (dirty) {
      assert(++count < 20, "component settles")
      dirty = false
      cursor = 0
      view = exports.ReconciliationValues(props)
      const queued = effects
      effects = []
      queued.forEach((f) => f())
    }
  }
  flush()
  return {
    requests,
    applied,
    props,
    get view() {
      return view
    },
    update(next) {
      Object.assign(props, next)
      dirty = true
      flush()
    },
    async click(node) {
      await node.props.onClick()
      flush()
    },
    change(node, value) {
      node.props.onChange({ target: { value } })
      flush()
    },
    async settle(i, data) {
      requests[i].resolve({ data, ms: 1 })
      await Promise.resolve()
      await Promise.resolve()
      flush()
    },
    unmount() {
      hooks.forEach((h) => h.cleanup?.())
    },
  }
}
function nodes(n) {
  if (Array.isArray(n)) return n.flatMap(nodes)
  return n && typeof n === "object" ? [n, ...nodes(n.props?.children)] : []
}
function text(n) {
  if (Array.isArray(n)) return n.map(text).join(" ")
  return n && typeof n === "object"
    ? text(n.props?.children)
    : typeof n === "string" || typeof n === "number"
      ? String(n)
      : ""
}
const candidate = { id: "Q10", name: "Paris", types: ["Q515"], score: 95 }
const recon = {
  j: "none",
  service: "http://example.test/recon",
  identifierSpace: "http://example.test/entity/",
  schemaSpace: null,
  c: [candidate],
}
const group = (row, value) => ({
  row,
  value,
  count: 2,
  eligible: 2,
  missing: 0,
  typeCounts: [["text", 2]],
  variantTotal: 2,
  bulkSafe: true,
  bulkReason: null,
})
function result(row = 0, chosen = row) {
  const g = group(row, row === 0 ? "Rome" : "Paris")
  return {
    total: 2,
    rows: 4,
    start: 0,
    limit: 20,
    values: [group(0, "Rome"), group(10, "Paris")],
    detail: {
      ...g,
      variantStart: 0,
      variants: [
        { row, count: 1, recon },
        { row: row + 2, count: 1, recon },
      ],
      selected: {
        row: chosen,
        count: 1,
        recon,
        candidateTotal: 1,
        candidateStart: 0,
      },
    },
  }
}
const match = (p, scope) =>
  nodes(p.view).find((n) =>
    n.props?.["aria-label"]?.startsWith(`Match Paris to ${scope}`)
  )

test("judging refetches while preserving value/state selection and using explicit cell versus same-text scope", async () => {
  const p = panel()
  assert.equal(p.requests[0].cmd, "recon_values")
  await p.settle(0, result())
  await p.click(
    nodes(p.view).find(
      (n) =>
        n.type === "button" &&
        n.props["aria-pressed"] !== undefined &&
        /^Paris/.test(text(n))
    )
  )
  await p.settle(1, result(10))
  p.change(
    nodes(p.view).find((n) => n.props?.["aria-label"] === "Saved result state"),
    "12"
  )
  await p.settle(2, result(10, 12))
  p.update({ version: "two" })
  const request = p.requests.at(-1)
  assert.equal(request.args.params.row, 10)
  assert.equal(request.args.params.variantRow, 12)
  assert.equal(
    match(p, "cell"),
    undefined,
    "old candidates cannot be judged during the new revision's request"
  )
  await p.settle(p.requests.length - 1, result(10, 12))
  await p.click(match(p, "cell"))
  assert.equal(p.applied[0].op, "ox/recon-judge-one-cell")
  assert.equal(p.applied[0].row, 12)
  await p.settle(p.requests.length - 1, result(10, 12))
  await p.click(match(p, "all"))
  assert.equal(p.applied[1].op, "core/recon-judge-similar-cells")
  assert.equal(p.applied[1].similarValue, "Paris")
  assert.equal(p.applied[1].engineConfig.mode, "record-based")
  assert.equal(p.applied[1].match.score, 95)
  p.unmount()
})

test("stale replies stay hidden and mixed services disable bulk while leaving the representative cell available", async () => {
  const p = panel()
  p.update({ version: "newer" })
  await p.settle(0, result())
  assert.equal(match(p, "cell"), undefined)
  const mixed = result(10, 12)
  mixed.detail.bulkSafe = false
  mixed.detail.bulkReason =
    "Matching cells use different reconciliation services."
  await p.settle(p.requests.length - 1, mixed)
  assert.equal(match(p, "cell").props.disabled, false)
  assert.equal(match(p, "all").props.disabled, true)
  assert.match(text(p.view), /different reconciliation services/)
  await p.click(match(p, "all"))
  assert.equal(p.applied.length, 0)
  p.unmount()
})
