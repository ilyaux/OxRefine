import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

test("superseded dialog submissions leave conflict recovery to App without an unhandled rejection", async () => {
  const exports = {}
  const jsx = (type, props) => ({ type, props })
  const source = readFileSync(new URL("../src/Dialogs.tsx", import.meta.url), "utf8")
  vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX } }).outputText, {
    exports,
    require(name) {
      if (name === "react") return { useState: (initial) => [initial, () => {}] }
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx }
      if (name.endsWith("grel-docs.json")) return { functions: {} }
      return {}
    },
  })
  const form = (node) => node?.type === "form" ? node : (Array.isArray(node) ? node : [node?.props?.children]).flatMap((child) => Array.isArray(child) ? child : [child]).map(form).find(Boolean)
  for (const name of ["ProjectConflict", "ProjectContextChanged", "Error", null]) {
    let closed = 0
    const error = Object.assign(new Error("test outcome"), { name })
    const view = exports.AskDialog({ title: "Rename", label: "Name", initial: "new", onSubmit: async () => { if (name) throw error }, onClose: () => { closed++ } })
    const submit = form(view).props.onSubmit({ preventDefault() {} })
    if (name === "Error") await assert.rejects(submit, /test outcome/)
    else await submit
    assert.equal(closed, name === null ? 1 : 0)
  }
})
