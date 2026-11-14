import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

// Execute the real component with deterministic hook effects, timers and HTTP responses.
// The harness provides React's state/dependency/cleanup mechanics, not Jobs' polling logic.
function jobsPanel(memory) {
  const hooks = [],
    timers = new Map(),
    calls = []
  let cursor = 0,
    dirty = true,
    effects = [],
    view,
    pendingListener
  let now = 0,
    timerId = 0,
    data = []
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
    useRef(initial) {
      const i = cursor++
      return (hooks[i] ??= { current: initial })
    },
    useEffect(effect, deps) {
      const i = cursor++
      const previous = hooks[i]
      if (
        !previous ||
        deps.some((value, index) => !Object.is(value, previous.deps[index]))
      ) {
        hooks[i] = { deps, cleanup: previous?.cleanup }
        effects.push(() => {
          hooks[i].cleanup?.()
          hooks[i].cleanup = effect()
        })
      }
    },
  }
  const jsx = (type, props, key) => ({ type, props, key })
  const exports = {}
  const code = ts.transpileModule(
    readFileSync(new URL("../src/Jobs.tsx", import.meta.url), "utf8"),
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
      if (name === "lucide-react") return {}
      if (name === "@/components/ui/button") return { Button: "button" }
      if (name === "@/lib/utils") return { cn: (...values) => values.join(" ") }
      if (name === "@/ProcessMemory") return { ProcessMemory: "process-memory" }
      if (name === "@/api")
        return {
          onPending(listener) {
            pendingListener = listener
            listener(0)
            return () => {
              pendingListener = undefined
            }
          },
          async call(cmd, args, options) {
            assert.equal(cmd, "processes")
            assert.equal(options.quiet, true)
            calls.push(now)
            return { data }
          },
        }
      throw new Error(`Unexpected component dependency: ${name}`)
    },
    setTimeout(callback, delay) {
      const id = ++timerId
      timers.set(id, { at: now + delay, callback })
      return id
    },
    clearTimeout(id) {
      timers.delete(id)
    },
  })
  function flush() {
    let renders = 0
    while (dirty) {
      assert.ok(
        ++renders < 20,
        "component must settle after state/effect updates"
      )
      dirty = false
      cursor = 0
      view = exports.Jobs({ memory })
      const queued = effects
      effects = []
      queued.forEach((effect) => effect())
    }
  }
  flush()
  return {
    calls,
    get view() {
      return view
    },
    set data(value) {
      data = value
    },
    pending(n) {
      pendingListener(n)
      flush()
    },
    click(node) {
      node.props.onClick()
      flush()
    },
    unmount() {
      hooks.forEach((hook) => hook.cleanup?.())
    },
    async advance(ms) {
      const until = now + ms
      for (;;) {
        const next = [...timers]
          .filter(([, timer]) => timer.at <= until)
          .sort((a, b) => a[1].at - b[1].at)[0]
        if (!next) break
        const [id, timer] = next
        timers.delete(id)
        now = timer.at
        await timer.callback()
        flush()
      }
      now = until
      flush()
    },
  }
}

function nodes(node) {
  if (Array.isArray(node)) return node.flatMap(nodes)
  if (node && typeof node === "object")
    return [node, ...nodes(node.props?.children)]
  return []
}

function text(node) {
  if (Array.isArray(node)) return node.map(text).join(" ")
  if (node && typeof node === "object") return text(node.props?.children)
  return typeof node === "string" || typeof node === "number"
    ? String(node)
    : ""
}

function job(id, state = "running") {
  const stage = (label, outcome) => ({
    label,
    state: outcome,
    done: 0,
    total: 0,
    unit: "items",
    elapsedMs: 600,
  })
  const stages =
    state === "running"
      ? [stage(`Fetching recipe ${id}`, "running")]
      : [
          stage(`Fetching recipe ${id}`, state),
          stage("Saving the project", "completed"),
        ]
  return {
    ...stages.at(-1),
    id,
    state,
    cancelRequested: state === "cancelled",
    elapsedMs: 1200,
    stages,
  }
}

test("cancelled stages survive quick summary/profile/rows refreshes and stop polling when idle", async () => {
  const panel = jobsPanel()
  panel.data = [job(1)]
  panel.pending(1)
  await panel.advance(299)
  assert.equal(panel.calls.length, 0, "short commands must not show progress")
  await panel.advance(1)
  assert.match(text(panel.view), /Fetching recipe 1/)

  panel.data = [job(1, "cancelled")]
  panel.pending(0)
  await panel.advance(0)
  const stages = nodes(panel.view).find(
    (node) => node.props?.["aria-label"] === "Work stages"
  )
  assert.match(text(stages), /Fetching recipe 1 cancelled/)
  assert.match(text(stages), /Saving the project completed/)

  // App refreshes state with several brief calls after the recipe response.
  for (const command of ["summary", "profile", "rows"]) {
    panel.pending(1)
    await panel.advance(40)
    assert.match(
      text(panel.view),
      /Fetching recipe 1/,
      `${command} must preserve the final job`
    )
    panel.pending(0)
    await panel.advance(0)
    assert.match(text(panel.view), /cancelled/)
  }
  const calls = panel.calls.length
  await panel.advance(5000)
  assert.equal(
    panel.calls.length,
    calls,
    "finished jobs must not keep the poller alive"
  )
  panel.unmount()
})

test("a newly observed long job replaces the previous result and its finished panel can be dismissed", async () => {
  const panel = jobsPanel()
  panel.data = [job(1)]
  panel.pending(1)
  await panel.advance(300)
  panel.data = [job(1, "cancelled")]
  panel.pending(0)
  await panel.advance(0)

  panel.data = [job(1, "cancelled"), job(2)]
  panel.pending(1)
  await panel.advance(300)
  assert.match(text(panel.view), /Fetching recipe 2/)
  assert.doesNotMatch(text(panel.view), /Fetching recipe 1/)
  panel.data = [job(1, "cancelled"), job(2, "completed")]
  panel.pending(0)
  await panel.advance(0)
  const dismiss = nodes(panel.view).find(
    (node) => node.props?.["aria-label"] === "Dismiss finished jobs"
  )
  assert.ok(dismiss)
  panel.click(dismiss)
  assert.equal(panel.view, null)
  const calls = panel.calls.length
  await panel.advance(1000)
  assert.equal(panel.calls.length, calls)
  panel.unmount()
})

test("unmount cancels a scheduled poll and quick initial commands never show stale server history", async () => {
  const panel = jobsPanel()
  panel.data = [job(5, "failed")]
  panel.pending(1)
  await panel.advance(100)
  panel.pending(0)
  await panel.advance(1000)
  assert.equal(panel.view, null)
  assert.equal(panel.calls.length, 0)
  panel.pending(1)
  panel.unmount()
  await panel.advance(1000)
  assert.equal(panel.calls.length, 0)
})

test("Jobs shows the shared backend memory sample without starting another memory poller", async () => {
  const memory = { pid: 123, currentBytes: 1024, peakBytes: 2048, metric: "rss", peakSource: "os", error: null }
  const panel = jobsPanel(memory)
  panel.data = [job(7)]
  panel.pending(1)
  await panel.advance(300)
  const memoryLabel = nodes(panel.view).find((node) => node.type === "process-memory")
  assert.ok(memoryLabel)
  assert.equal(memoryLabel.props.memory, memory)
  panel.data = [job(7, "completed")]
  panel.pending(0)
  await panel.advance(0)
  assert.equal(nodes(panel.view).find((node) => node.type === "process-memory").props.memory, memory)
  panel.unmount()
})
