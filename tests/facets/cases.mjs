// Facet conformance catalog: each case sets up the ops corpus (optional operations first), then asks
// OpenRefine 3.10.1 for compute-facets and for the rows that match. gen.mjs stores both in fixtures.json.
export { CORPUS } from "../ops/cases.mjs"

const all = { facets: [], mode: "row-based" }
const list = (columnName, expression = "value", selection = [], extra = {}) => ({
  type: "list", name: columnName, columnName, expression, omitBlank: false, omitError: false,
  selection: selection.map((v) => ({ v: { v, l: String(v) } })), selectBlank: false, selectError: false, invert: false, ...extra,
})
const range = (columnName, expression, from, to, extra = {}) => ({
  type: "range", name: columnName, columnName, expression, from, to,
  selectNumeric: true, selectNonNumeric: true, selectBlank: true, selectError: true, ...extra,
})
const text = (columnName, query, extra = {}) => ({ type: "text", name: columnName, columnName, query, mode: "text", caseSensitive: false, invert: false, ...extra })
const transform = (columnName, expression, onError = "keep-original") =>
  ({ op: "core/text-transform", engineConfig: all, columnName, expression: "grel:" + expression, onError, repeat: false, repeatCount: 10 })
const rows = (...facets) => ({ facets, mode: "row-based" })
const records = (...facets) => ({ facets, mode: "record-based" })

export const CASES = [
  { name: "list city counts", engine: rows(list("city")) },
  { name: "list selection with text facet", engine: rows(list("city", "value", ["Boston", "boston "]), text("name", "a")) },
  { name: "list on expression", engine: rows(list("city", "value.toLowercase().trim()", ["new york"])) },
  { name: "list inverted", engine: rows(list("city", "value", ["Chicago"], { invert: true })) },
  { name: "list blank selected", engine: rows(list("note", "value", [], { selectBlank: true })) },
  { name: "word facet", engine: rows(list("tags", "value.split('|')", ["red"])) },
  { name: "text length facet", engine: rows(range("name", "value.length()", 9, 12)) },
  { name: "numeric facet on text", engine: rows(range("score", "value.toNumber()", 0, 10)) },
  { name: "numeric facet no blanks or errors", engine: rows(range("score", "value.toNumber()", 0, 10, { selectBlank: false, selectError: false })) },
  { name: "numeric facet only non-numeric", engine: rows(range("score", "value", -100, 100, { selectNumeric: false })) },
  { name: "numeric facet typed column", ops: [transform("score", "value.toNumber()")], engine: rows(range("score", "value", 3, 12)) },
  { name: "numeric facet typed, full range", ops: [transform("score", "value.toNumber()")], engine: rows(range("score", "value", -2, 13)) },
  { name: "list on numbers", ops: [transform("score", "value.toNumber()")], engine: rows(list("score")) },
  { name: "duplicates facet", engine: rows(list("name", "facetCount(value, 'value', 'name') > 1", [true])) },
  { name: "blank facet", engine: rows(list("note", "isBlank(value)", [true])) },
  { name: "error facet", ops: [transform("joined", "value.toDate()", "store-error")], engine: rows(list("joined", "isError(value)", [true])) },
  { name: "date choices", ops: [transform("joined", "value.toDate()", "store-error")], engine: rows(list("joined")) },
  { name: "star facet", ops: [{ op: "core/row-star", engineConfig: rows(text("city", "york")), starred: true }], engine: rows(list("id", "row.starred", [true])) },
  { name: "flag facet", ops: [{ op: "core/row-flag", engineConfig: rows(list("id", "value", ["3"])), flagged: true }], engine: rows(list("id", "row.flagged")) },
  { name: "text facet regex case-sensitive", engine: rows(text("name", "^[A-Z]\\w+ [A-Z]", { mode: "regex", caseSensitive: true })) },
  { name: "records: list counts and filter", engine: records(list("tags", "value", ["blue"])) },
  { name: "records: two facets", engine: records(list("city", "value", ["Chicago"]), text("tags", "red")) },
]
