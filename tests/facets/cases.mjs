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
const time = (columnName, expression, from, to, extra = {}) => ({
  type: "timerange", name: columnName, columnName, expression, from: Date.parse(from), to: Date.parse(to),
  selectTime: true, selectNonTime: true, selectBlank: true, selectError: true, ...extra,
})
const text = (columnName, query, extra = {}) => ({ type: "text", name: columnName, columnName, query, mode: "text", caseSensitive: false, invert: false, ...extra })
const transform = (columnName, expression, onError = "keep-original") =>
  ({ op: "core/text-transform", engineConfig: all, columnName, expression: "grel:" + expression, onError, repeat: false, repeatCount: 10 })
const rows = (...facets) => ({ facets, mode: "row-based" })
const records = (...facets) => ({ facets, mode: "record-based" })

const dates = [transform("joined", "value.toDate()", "store-error")]
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
  // the facet choice limit, a preference: more choices than that and OpenRefine lists none
  { name: "choice limit exceeded", prefs: { "ui.browsing.listFacet.limit": 3 }, engine: rows(list("city")) },
  { name: "choice limit not reached", prefs: { "ui.browsing.listFacet.limit": 30 }, engine: rows(list("city")) },
  { name: "choice limit with a selection", prefs: { "ui.browsing.listFacet.limit": 2 }, engine: rows(list("city", "value", ["Boston"])) },
  { name: "row.record facet", engine: rows(list("id", "row.record.rowCount", [2])) },
  { name: "records: row.record facet", engine: records(list("name", "row.record.cells['tags'].value.length()", [3])) },

  // timeline facets (OpenRefine's timerange): from and to are milliseconds since 1970
  { name: "timeline full range", ops: dates, engine: rows(time("joined", "value", "2022-01-01T00:00:00Z", "2025-01-01T00:00:00Z")) },
  { name: "timeline part", ops: dates, engine: rows(time("joined", "value", "2023-12-31T00:00:00Z", "2024-02-01T00:00:00Z")) },
  { name: "timeline only dates", ops: dates, engine: rows(time("joined", "value", "2022-01-01T00:00:00Z", "2025-01-01T00:00:00Z", { selectNonTime: false, selectBlank: false, selectError: false })) },
  { name: "timeline only blanks", ops: dates, engine: rows(time("joined", "value", "2022-01-01T00:00:00Z", "2025-01-01T00:00:00Z", { selectTime: false, selectNonTime: false, selectError: false })) },
  { name: "timeline on text dates", engine: rows(time("joined", "value", "2022-01-01T00:00:00Z", "2025-01-01T00:00:00Z")) },
  { name: "timeline on expression", engine: rows(time("joined", "value.toDate()", "2024-01-01T00:00:00Z", "2024-03-01T00:00:00Z")) },
  { name: "timeline in minutes", engine: rows(time("id", "'2024-01-01'.toDate().inc(rowIndex * 7, 'minutes')", "2024-01-01T00:10:00Z", "2024-01-01T00:40:00Z")) },
  { name: "timeline in hours", engine: rows(time("id", "'2024-01-01'.toDate().inc(rowIndex * 13, 'hours')", "2024-01-01T00:00:00Z", "2024-01-03T00:00:00Z")) },
  { name: "timeline in years", engine: rows(time("id", "'1900-01-01'.toDate().inc(rowIndex * 15, 'years')", "1900-01-01T00:00:00Z", "2100-01-01T00:00:00Z")) },
  { name: "timeline one date", engine: rows(time("id", "'2024-05-05'.toDate()", "2024-05-05T00:00:00Z", "2024-05-06T00:00:00Z")) },
  { name: "timeline exact bounds", ops: dates, engine: rows(time("joined", "value", "2024-01-15T00:00:00Z", "2024-03-10T00:00:00Z")) },
  { name: "timeline with list facet", ops: dates, engine: rows(time("joined", "value", "2022-01-01T00:00:00Z", "2025-01-01T00:00:00Z"), list("city", "value", ["Boston"])) },
]
