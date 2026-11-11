// OpenRefine project archives. gen.mjs builds each case in a real OpenRefine 3.10.1, exports the
// project (.openrefine.tar.gz) into archives/, and records the table at every history step.
import { CASES as OPS, CORPUS } from "../ops/cases.mjs"
import { CASES as RECON, CORPUS as RECON_CORPUS } from "../recon/cases.mjs"

export { CORPUS }
const all = { facets: [], mode: "row-based" }
const transform = (columnName, expression, onError = "keep-original") =>
  ({ op: "core/text-transform", engineConfig: all, columnName, expression: "grel:" + expression, onError, repeat: false, repeatCount: 10 })

export const CASES = [
  // every operation case of the ops suite, so every kind of change file shows up (fetch-URL steps
  // replay from what OpenRefine fetched; cross() looks into the "lookup" project)
  ...OPS.filter((c) => c.name !== "sort score blanks first"),
  // typed cells straight from the importer
  { name: "guessed types", guessTypes: true, ops: [transform("joined", "value.toDate()", "store-error")] },
  { name: "unicode and quotes", csv: 'a,b\n"Æsir ""q""",ü\n"line\nbreak",\t\n', ops: [transform("a", "value + '→'")] },
  // undone steps stay in the archive as future history
  { name: "undo leaves future steps", undo: 2, ops: [
    { op: "core/column-rename", oldColumnName: "name", newColumnName: "who" },
    transform("city", "value.trim()"),
    { op: "core/column-removal", columnName: "note" },
    { op: "core/row-star", engineConfig: all, starred: true },
  ] },
  // operations OxRefine has no engine for yet: their recorded changes still replay
  { name: "transpose columns into rows", ops: [{ op: "core/transpose-columns-into-rows", startColumnName: "city", columnCount: 2,
    ignoreBlankCells: true, fillDown: false, combinedColumnName: "pair", prependColumnName: true, separator: ":" }] },
  { name: "key-value columnize", ops: [
    { op: "core/transpose-columns-into-rows", startColumnName: "city", columnCount: 2, ignoreBlankCells: true, fillDown: true,
      keyColumnName: "key", valueColumnName: "value" },
    { op: "core/key-value-columnize", keyColumnName: "key", valueColumnName: "value", noteColumnName: "" },
  ] },
  { name: "remove then add a column", ops: [
    { op: "core/column-removal", columnName: "tags" },
    { op: "core/column-addition", engineConfig: all, baseColumnName: "name", newColumnName: "initial", columnInsertIndex: 1,
      expression: "grel:value[0]", onError: "set-to-blank" },
    transform("initial", "value.toLowercase()"),
  ] },
  // reconciliation (against tests/recon/service.mjs, written {RECON}): Recons in the data, the
  // change files and the column model, kept through later steps
  ...RECON.map((c) => ({ ...c, name: `recon: ${c.name}`, csv: RECON_CORPUS, ops: JSON.parse(JSON.stringify(c.ops).replaceAll("{BASE}", "{RECON}")) })),
  { name: "recon: undo leaves future steps", csv: RECON_CORPUS, undo: 1, ops: JSON.parse(JSON.stringify([
    RECON[1].ops[0],
    { op: "core/row-removal", engineConfig: { facets: [{ type: "text", name: "country", columnName: "country", query: "us", mode: "text", caseSensitive: false, invert: false }], mode: "row-based" } },
    { op: "core/column-move", columnName: "name", index: 2 },
  ]).replaceAll("{BASE}", "{RECON}")) },
]
