// Operation conformance catalog. gen.mjs applies each case to CORPUS in a fresh OpenRefine 3.10.1
// project and stores the resulting table (columns, rows, typed cells, stars, flags) in fixtures.json.

// Records: a blank `id` continues the record above (OpenRefine's key column is the first one).
export const CORPUS = `id,name,tags,city,score,joined,note
1,Alice Smith,red|green,New York,10,2024-01-15,first
,,blue,new york,,2024-02-01,
2,bob jones,green,Boston,7.5,2023-12-31,
3,Carol King,,boston ,12,,dup
3,Carol King,,boston ,12,,dup
,,red|blue|green,Chicago,x,2024-03-10,
4,Dan  Brown,blue,  Chicago,3,not a date,last
5,,red,,0,2024-01-15,
6,Eve Adams,green|red,New-York,-2,2022-06-30,"quoted, comma"
`

const all = { facets: [], mode: "row-based" }
const records = { facets: [], mode: "record-based" }
const text = (columnName, query, mode = "row-based") => ({ facets: [{ type: "text", name: columnName, columnName, query, mode: "text", caseSensitive: false, invert: false }], mode })
const list = (columnName, values, mode = "row-based", extra = {}) => ({
  facets: [{ type: "list", name: columnName, columnName, expression: "value", omitBlank: false, omitError: false,
    selection: values.map((v) => ({ v: { v, l: v } })), selectBlank: false, selectError: false, invert: false, ...extra }],
  mode,
})
const transform = (columnName, expression, engineConfig = all, onError = "keep-original") =>
  ({ op: "core/text-transform", engineConfig, columnName, expression: "grel:" + expression, onError, repeat: false, repeatCount: 10 })
const split = (columnName, extra) => ({ op: "core/column-split", engineConfig: all, columnName, guessCellType: true, removeOriginalColumn: true, mode: "separator", separator: "|", regex: false, maxColumns: 0, ...extra })
const sort = (...criteria) => ({ op: "core/row-reorder", mode: "row-based", sorting: { criteria } })
const crit = (column, valueType, extra = {}) => ({ column, valueType, reverse: false, blankPosition: 2, errorPosition: 1, ...(valueType === "string" ? { caseSensitive: false } : {}), ...extra })

export const CASES = [
  // columns
  { name: "column-move to front", ops: [{ op: "core/column-move", columnName: "city", index: 0 }] },
  { name: "column-move to end", ops: [{ op: "core/column-move", columnName: "id", index: 6 }] },
  { name: "column-move-left", ops: [{ op: "core/column-move-left", columnName: "city" }] },
  { name: "column-move-right", ops: [{ op: "core/column-move-right", columnName: "city" }] },
  { name: "column-move-first", ops: [{ op: "core/column-move-first", columnName: "note" }] },
  { name: "column-move-last", ops: [{ op: "core/column-move-last", columnName: "id" }] },
  { name: "column-reorder subset", ops: [{ op: "core/column-reorder", columnNames: ["name", "id", "score"] }] },
  { name: "column-multi-removal", ops: [{ op: "core/column-multi-removal", columnNames: ["tags", "note"] }] },
  { name: "rename then transform", ops: [{ op: "core/column-rename", oldColumnName: "name", newColumnName: "full name" }, transform("full name", "value.toUppercase()")] },

  // typed transforms
  { name: "toNumber keep-original", ops: [transform("score", "value.toNumber()")] },
  { name: "toNumber set-to-blank", ops: [transform("score", "value.toNumber()", all, "set-to-blank")] },
  { name: "toDate store-error", ops: [transform("joined", "value.toDate()", all, "store-error")] },
  { name: "transform reads other cells", ops: [transform("note", "cells['name'].value + ' @ ' + cells['city'].value", all, "set-to-blank")] },
  { name: "transform in facet only", ops: [transform("city", "value.trim().toTitlecase()", text("city", "york"))] },
  { name: "transform record mode facet", ops: [transform("tags", "value.toUppercase()", text("city", "chicago", "record-based"))] },
  { name: "column-addition per row", ops: [{ op: "core/column-addition", engineConfig: all, baseColumnName: "score", newColumnName: "double", columnInsertIndex: 5,
    expression: "grel:value.toNumber() * 2", onError: "set-to-blank" }] },
  { name: "mass-edit with blank", ops: [{ op: "core/mass-edit", engineConfig: all, columnName: "city", expression: "value",
    edits: [{ from: ["new york", "New-York"], fromBlank: false, fromError: false, to: "New York" }, { from: [], fromBlank: true, fromError: false, to: "(none)" }] }] },

  // column split
  { name: "split tags guess types", ops: [split("tags")] },
  { name: "split keep original no guess", ops: [split("tags", { guessCellType: false, removeOriginalColumn: false })] },
  { name: "split max columns", ops: [split("tags", { maxColumns: 2 })] },
  { name: "split name regex", ops: [split("name", { separator: "\\s+", regex: true })] },
  { name: "split score numbers", ops: [split("score", { separator: "." })] },
  { name: "split joined by lengths", ops: [split("joined", { mode: "lengths", fieldLengths: [4, 1, 2] })] },
  { name: "split only matching rows", ops: [{ ...split("tags"), engineConfig: list("city", ["Chicago"]) }] },

  // multi-valued cells and records
  { name: "multivalued split", ops: [{ op: "core/multivalued-cell-split", columnName: "tags", keyColumnName: "id", mode: "separator", separator: "|", regex: false }] },
  { name: "multivalued split regex", ops: [{ op: "core/multivalued-cell-split", columnName: "name", keyColumnName: "id", mode: "separator", separator: "\\s+", regex: true }] },
  { name: "multivalued join", ops: [{ op: "core/multivalued-cell-join", columnName: "tags", keyColumnName: "id", separator: ";" }] },
  { name: "multivalued split then join", ops: [
    { op: "core/multivalued-cell-split", columnName: "tags", keyColumnName: "id", mode: "separator", separator: "|", regex: false },
    { op: "core/multivalued-cell-join", columnName: "tags", keyColumnName: "id", separator: "," }] },
  { name: "multivalued join city", ops: [{ op: "core/multivalued-cell-join", columnName: "city", keyColumnName: "id", separator: " + " }] },

  // fill and blank down
  { name: "fill-down id", ops: [{ op: "core/fill-down", engineConfig: all, columnName: "id" }] },
  { name: "fill-down name", ops: [{ op: "core/fill-down", engineConfig: all, columnName: "name" }] },
  { name: "fill-down in facet", ops: [{ op: "core/fill-down", engineConfig: text("city", "york"), columnName: "score" }] },
  { name: "fill-down record mode", ops: [{ op: "core/fill-down", engineConfig: records, columnName: "name" }] },
  { name: "blank-down city", ops: [{ op: "core/blank-down", engineConfig: all, columnName: "city" }] },
  { name: "blank-down name", ops: [{ op: "core/blank-down", engineConfig: all, columnName: "name" }] },
  { name: "fill then blank down", ops: [{ op: "core/fill-down", engineConfig: all, columnName: "id" }, { op: "core/blank-down", engineConfig: all, columnName: "id" }] },

  // rows
  { name: "row-removal facet", ops: [{ op: "core/row-removal", engineConfig: text("city", "york") }] },
  { name: "row-removal list facet blank", ops: [{ op: "core/row-removal", engineConfig: list("note", [], "row-based", { selectBlank: true }) }] },
  { name: "row-keep-matched", ops: [{ op: "core/row-keep-matched", engineConfig: text("tags", "green") }] },
  { name: "row-removal record mode", ops: [{ op: "core/row-removal", engineConfig: text("tags", "blue", "record-based") }] },
  { name: "row-star facet", ops: [{ op: "core/row-star", engineConfig: text("city", "boston"), starred: true }] },
  { name: "row-flag then unflag", ops: [{ op: "core/row-flag", engineConfig: all, flagged: true }, { op: "core/row-flag", engineConfig: list("id", ["3"]), flagged: false }] },
  { name: "row-duplicate-removal id", ops: [{ op: "core/row-duplicate-removal", engineConfig: all, criteria: ["id"] }] },
  { name: "row-duplicate-removal name city", ops: [{ op: "core/row-duplicate-removal", engineConfig: all, criteria: ["name", "city"] }] },

  // sorting made permanent
  { name: "sort score as number", ops: [sort(crit("score", "number"))] },
  { name: "sort score as number reversed", ops: [sort(crit("score", "number", { reverse: true }))] },
  { name: "sort score blanks first", ops: [sort(crit("score", "number", { blankPosition: 0, errorPosition: 2 }))] },
  { name: "sort name as text", ops: [sort(crit("name", "string"))] },
  { name: "sort name case-sensitive", ops: [sort(crit("name", "string", { caseSensitive: true }))] },
  { name: "sort joined as date", ops: [transform("joined", "value.toDate()"), sort(crit("joined", "date"))] },
  { name: "sort typed numbers", ops: [transform("score", "value.toNumber()"), sort(crit("score", "number", { reverse: true }))] },
  { name: "sort two criteria", ops: [sort(crit("city", "string"), crit("score", "number", { reverse: true }))] },

  // a realistic recipe
  { name: "cleanup recipe", ops: [
    { op: "core/fill-down", engineConfig: all, columnName: "id" },
    transform("name", "value.replace(/\\s+/, ' ').trim().toTitlecase()"),
    transform("city", "value.trim()"),
    { op: "core/mass-edit", engineConfig: all, columnName: "city", expression: "value", edits: [{ from: ["new york", "New-York"], fromBlank: false, fromError: false, to: "New York" }] },
    transform("score", "value.toNumber()", all, "set-to-blank"),
    { op: "core/row-duplicate-removal", engineConfig: all, criteria: ["id", "name"] },
    sort(crit("score", "number", { reverse: true })),
  ] },
]
