// Import conformance catalog: each case imports one file from tests/import with OpenRefine's own
// importer options; gen.mjs stores what OpenRefine 3.10.1 produced in fixtures.json.

export const CSV_DEFAULTS = {
  encoding: "UTF-8", separator: ",", ignoreLines: -1, headerLines: 1, skipDataLines: 0, limit: -1,
  storeBlankRows: true, guessCellValueTypes: false, processQuotes: true, quoteCharacter: '"',
  storeBlankCellsAsNulls: true, includeFileSources: false, includeArchiveFileName: false, trimStrings: false,
}
export const SHEET_DEFAULTS = { headerLines: 1, ignoreLines: -1, skipDataLines: 0, limit: -1, storeBlankRows: true, storeBlankCellsAsNulls: true, includeFileSources: false }
export const JSON_DEFAULTS = { recordPath: ["_", "_"], limit: -1, trimStrings: false, guessCellValueTypes: false, storeEmptyStrings: true, includeFileSources: false }

const csv = (file, options = {}) => ({ file, format: "text/line-based/*sv", options: { ...CSV_DEFAULTS, ...options } })
const sheet = (file, sheets, options = {}) => ({
  file,
  format: file.endsWith(".ods") ? "text/xml/ods" : "binary/text/xml/xls/xlsx",
  options: { ...SHEET_DEFAULTS, sheets: sheets.map((i) => ({ name: `${file}#${i}`, fileNameAndSheetIndex: `${file}#${i}`, rows: 100, selected: true })), ...options },
})
const json = (file, options = {}) => ({ file, format: "text/json", options: { ...JSON_DEFAULTS, ...options } })

export const CASES = [
  { name: "semicolons, BOM, quoted newline", ...csv("semi.csv", { separator: ";" }) },
  { name: "semicolons no blank rows", ...csv("semi.csv", { separator: ";", storeBlankRows: false }) },
  { name: "semicolons blanks as empty strings", ...csv("semi.csv", { separator: ";", storeBlankCellsAsNulls: false }) },
  { name: "two header lines", ...csv("semi.csv", { separator: ";", headerLines: 2 }) },
  { name: "tsv", ...csv("tabbed.tsv", { separator: "\t" }) },
  { name: "tsv trim strings", ...csv("tabbed.tsv", { separator: "\t", trimStrings: true }) },
  { name: "ignore, skip and limit", ...csv("messy.csv", { ignoreLines: 2, skipDataLines: 1, limit: 3 }) },
  { name: "no header line", ...csv("messy.csv", { ignoreLines: 3, headerLines: 0 }) },
  { name: "guess cell types", ...csv("numbers.csv", { guessCellValueTypes: true }) },
  { name: "guess types and trim", ...csv("numbers.csv", { guessCellValueTypes: true, trimStrings: true }) },
  { name: "quotes processed", ...csv("quotes.csv") },
  { name: "quotes as text", ...csv("quotes.csv", { processQuotes: false }) },
  { name: "latin-1 encoding", ...csv("latin1.csv", { encoding: "ISO-8859-1" }) },
  { name: "xlsx first sheet", ...sheet("sheets.xlsx", [0]) },
  { name: "xlsx second sheet", ...sheet("sheets.xlsx", [1]) },
  { name: "xlsx both sheets", ...sheet("sheets.xlsx", [0, 1]) },
  { name: "xlsx no header", ...sheet("sheets.xlsx", [1], { headerLines: 0 }) },
  { name: "xlsx no blank rows", ...sheet("sheets.xlsx", [0], { storeBlankRows: false }) },
  { name: "ods first sheet", ...sheet("sheets.ods", [0]) },
  { name: "ods second sheet", ...sheet("sheets.ods", [1]) },
  { name: "json array of objects", ...json("people.json") },
  { name: "json guess and trim", ...json("people.json", { guessCellValueTypes: true, trimStrings: true }) },
  { name: "json limit", ...json("people.json", { limit: 2 }) },
  { name: "json nested record path", ...json("nested.json", { recordPath: ["_", "data", "items", "_"] }) },
]
