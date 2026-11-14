import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { test } from "node:test"
import vm from "node:vm"
import ts from "typescript"

const fixture = JSON.parse(readFileSync(new URL("../../tests/facet_custom/fixtures.json", import.meta.url), "utf8"))
const jsx = (type, props) => ({ type, props })
function load(file, imports = {}, expose = "") {
  const exports = {}
  const code = ts.transpileModule(readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8") + expose, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2023, jsx: ts.JsxEmit.ReactJSX },
  }).outputText
  vm.runInNewContext(code, { exports, require(name) {
    if (imports[name]) return imports[name]
    if (name === "react") return { useState: (initial) => [initial, () => {}] }
    if (name === "react/jsx-runtime") return { jsx, jsxs: jsx }
    return new Proxy({}, { get: (_, key) => String(key) })
  } })
  return exports
}
function nodes(node) {
  if (Array.isArray(node)) return node.flatMap(nodes)
  return node && typeof node === "object" ? [node, ...nodes(node.props?.children)] : []
}
function text(node) {
  if (Array.isArray(node)) return node.map(text).join("")
  return node && typeof node === "object" ? text(node.props?.children) : typeof node === "string" ? node : ""
}

test("column menu and command palette dispatch all five actual OpenRefine preset expressions", () => {
  const grid = load("Grid.tsx", {}, "\nexport { ColumnMenu }\n")
  const palette = load("Palette.tsx", { "@/Grid": grid })
  const column = 'quoted "column": é', actions = [], closed = []
  const menu = grid.ColumnMenu({ column, onAction: (action) => actions.push(action) })
  const commands = palette.Palette({ open: true, column, columns: [column], commands: [], onColumn() {}, onOpenChange: (open) => closed.push(open), onAction: (action) => actions.push(action) })
  for (const preset of fixture.presets) {
    const entries = grid.PRESETS.filter(([label]) => label === preset.label)
    assert.equal(entries.length, 1, `${preset.label} appears once`)
    assert.deepEqual(Array.from(entries[0]), [preset.label, preset.type, preset.expression])
    const item = nodes(menu).find((node) => node.type === "DropdownMenuItem" && text(node) === preset.label)
    assert.ok(item, "preset is available through the column menu")
    item.props.onClick()
    const command = nodes(commands).find((node) => node.type === "CommandItem" && node.props.value === `${preset.label} ${column}`)
    assert.ok(command, "the palette imports the same preset")
    command.props.onSelect()
    const expected = { kind: "preset-facet", column, facet: preset.type, expression: preset.expression }
    assert.deepEqual(JSON.parse(JSON.stringify(actions.splice(0))), [expected, expected])
  }
  assert.deepEqual(closed, [false, false, false, false, false], "choosing a preset closes the palette normally")
})
