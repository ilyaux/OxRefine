// Export conformance catalog. Each case exports the ops corpus (after SETUP) with OpenRefine's own
// export-rows command; gen.mjs stores the bytes (base64 for binary formats) in fixtures.json.
export { CORPUS } from "../ops/cases.mjs"

const all = { facets: [], mode: "row-based" }
const transform = (columnName, expression, onError = "keep-original") =>
  ({ op: "core/text-transform", engineConfig: all, columnName, expression: "grel:" + expression, onError, repeat: false, repeatCount: 10 })

/** Typed numbers, dates and errors, a starred row and markup that HTML must escape. */
export const SETUP = [
  transform("score", "value.toNumber()"),
  transform("joined", "value.toDate()", "store-error"),
  transform("tags", "if(value == 'blue', '<b>blue</b> & \"co\"', value)"),
  { op: "core/row-star", engineConfig: { facets: [{ type: "text", name: "city", columnName: "city", query: "york", mode: "text", caseSensitive: false, invert: false }], mode: "row-based" }, starred: true },
]

const york = { facets: [{ type: "text", name: "city", columnName: "city", query: "york", mode: "text", caseSensitive: false, invert: false }], mode: "row-based" }
const sv = (options) => ({ format: "*sv", params: { options: JSON.stringify(options) } })

// OpenRefine's default "Templating" export, as its dialog fills it in for this project
const rowTemplate = ["id", "name", "tags", "city", "score", "joined", "note"].map((c) => `      "${c}" : {{jsonize(cells["${c}"].value)}}`).join(",\n")
export const DEFAULT_TEMPLATE = { prefix: '{\n  "rows" : [\n', template: `    {\n${rowTemplate}\n    }`, separator: ",\n", suffix: "\n  ]\n}" }

export const CASES = [
  { name: "csv", format: "csv" },
  { name: "csv of facet rows", format: "csv", engine: york },
  { name: "tsv", format: "tsv" },
  { name: "html", format: "html" },
  { name: "template custom", format: "template", params: { template: "{{rowIndex}}:{{value}}|{{cells.city.value}}|{{row.starred}}|{{cells.score.value * 2}}", prefix: "BEGIN\n", suffix: "\nEND", separator: "\n" } },
  { name: "template default json", format: "template", params: DEFAULT_TEMPLATE },
  { name: "template of facet rows", format: "template", engine: york, params: { template: "{{jsonize(cells.name.value)}}", prefix: "[", suffix: "]", separator: "," } },
  { name: "custom columns and separator", ...sv({ separator: ";", lineSeparator: "\n", outputColumnHeaders: true, outputBlankRows: false, columns: [{ name: "name" }, { name: "score" }, { name: "joined" }] }) },
  { name: "custom quote all, no header, blank rows", ...sv({ separator: ",", lineSeparator: "\n", quoteAll: true, outputColumnHeaders: false, outputBlankRows: true, columns: [{ name: "city" }, { name: "note" }] }) },
  { name: "custom skip blank rows", ...sv({ separator: "|", lineSeparator: "\r\n", outputColumnHeaders: true, outputBlankRows: false, columns: [{ name: "name" }, { name: "note" }] }) },
  { name: "xlsx", format: "xlsx", binary: true },
  { name: "xlsx of facet rows", format: "xlsx", engine: york, binary: true },
  { name: "ods", format: "ods", binary: true },
  { name: "ods of facet rows", format: "ods", engine: york, binary: true },
  {
    name: "sql",
    format: "sql",
    params: {
      options: JSON.stringify({
        tableName: "people", includeStructure: true, includeDropStatement: true, includeContent: true, includeIfExistWithDropStatement: true,
        includeNullFieldValue: true, convertNulltoEmptyString: false,
        columns: ["id", "name", "score", "joined", "note"].map((name) => ({ name, type: name === "score" ? "DOUBLE" : "VARCHAR", size: name === "score" ? "" : "255", allowNull: true, defaultValue: "", nullValueToEmptyStr: false })),
      }),
    },
  },
]
