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
export const XML_DEFAULTS = { recordPath: [], limit: -1, trimStrings: false, guessCellValueTypes: false, storeEmptyStrings: true, includeFileSources: false, includeArchiveFileName: false }
const xml = (file, recordPath, options = {}) => ({ file, format: "text/xml", options: { ...XML_DEFAULTS, recordPath, ...options } })
export const FIXED_DEFAULTS = { encoding: "UTF-8", ignoreLines: -1, headerLines: 0, skipDataLines: 0, limit: -1, storeBlankRows: true, storeBlankColumns: true,
  storeBlankCellsAsNulls: true, guessCellValueTypes: false, includeFileSources: false, includeArchiveFileName: false }
const fixed = (columnWidths, options = {}) => ({ file: "fixed.txt", format: "text/line-based/fixed-width", options: { ...FIXED_DEFAULTS, columnWidths, ...options } })
const archive = (file, options = {}) => csv(file, { includeFileSources: true, includeArchiveFileName: true, ...options })

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
  // XML: the record path names elements from the root down
  { name: "xml books", ...xml("books.xml", ["library", "book"]) },
  { name: "xml books guess and trim", ...xml("books.xml", ["library", "book"], { guessCellValueTypes: true, trimStrings: true }) },
  { name: "xml books no empty strings", ...xml("books.xml", ["library", "book"], { storeEmptyStrings: false }) },
  { name: "xml books limit", ...xml("books.xml", ["library", "book"], { limit: 2 }) },
  { name: "xml whole library", ...xml("books.xml", ["library"]) },
  { name: "xml magazines", ...xml("books.xml", ["library", "magazine"]) },
  { name: "xml nested record", ...xml("books.xml", ["library", "book", "tags", "tag"]) },
  { name: "xml atom entries", ...xml("atom.xml", ["feed", "entry"]) },
  // fixed-width text
  { name: "fixed widths", ...fixed([5, 10, 9]) },
  { name: "fixed widths header", ...fixed([5, 10, 9], { headerLines: 1 }) },
  { name: "fixed widths guess types", ...fixed([5, 10, 9, 6], { headerLines: 1, guessCellValueTypes: true }) },
  { name: "fixed no blank rows", ...fixed([5, 10], { storeBlankRows: false, skipDataLines: 1, limit: 3 }) },
  { name: "fixed blanks as empty strings", ...fixed([5, 10, 9], { headerLines: 1, storeBlankCellsAsNulls: false }) },
  // archives: every data file inside, with the file and archive names as columns
  { name: "zip of csv files", ...archive("bundle.zip") },
  { name: "zip without name columns", ...archive("bundle.zip", { includeFileSources: false, includeArchiveFileName: false }) },
  { name: "tar.gz of csv files", ...archive("bundle.tar.gz") },
  { name: "gzip csv", ...csv("single.csv.gz") },
  { name: "gzip csv with file name", ...csv("single.csv.gz", { includeFileSources: true }) },
  { name: "tar.bz2 of csv files", ...archive("bundle.tar.bz2") },
  { name: "bzip2 csv", ...csv("single.csv.bz2") },
  { name: "bzip2 csv with file name", ...csv("single.csv.bz2", { includeFileSources: true }) },
]
