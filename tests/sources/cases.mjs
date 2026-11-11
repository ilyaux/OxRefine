// Clipboard and web-address import catalog. gen.mjs runs each source through OpenRefine 3.10.1's own
// import wizard (format guess, default options, create project) and stores what it picked and built.
import { readFileSync } from "node:fs"

/** Served by a local web server in gen.mjs and in the Rust test: [content type, body]. */
export const FILES = {
  "/people.csv": ["text/csv", "id,name,city\n1, Ann ,Paris\n2,Bob,\"New York, NY\"\n"],
  "/data": ["text/tab-separated-values", "a\tb\n1\t\"two words\"\n3\t4\n"],
  "/noext": ["application/octet-stream", "x;y\n1;2\n"],
  "/rows.json": ["application/json", '[{"a":1,"b":"x"},{"a":2,"b":" y "}]'],
  "/api/export?format=csv": ["text/csv; charset=utf-8", "k,v\n1,2\n"],
  "/notes.txt": ["text/plain", "first line\nsecond line\n"],
  "/sheet.xlsx": ["application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", readFileSync(new URL("../import/sheets.xlsx", import.meta.url)).toString("base64"), "base64"],
}

export const CASES = [
  { name: "clipboard tab separated", clipboard: "name\tcity\nAnn\tParis\nBob\t\"New York\"\n" },
  { name: "clipboard comma separated", clipboard: "a,b\n1, two \n3,\"4,5\"\n" },
  { name: "clipboard semicolons", clipboard: "x;y;z\n1;2;3\n" },
  { name: "clipboard one line", clipboard: "one line only" },
  { name: "clipboard lines with blanks", clipboard: "  spaced  \n\n\nlast\n" },
  { name: "clipboard json text", clipboard: '[{"a":1}]' },
  { name: "url csv", url: "/people.csv" },
  { name: "url tsv by content type", url: "/data" },
  { name: "url without extension", url: "/noext" },
  // OpenRefine's wizard has no default record path for JSON; the user clicks one (here: each array item)
  { name: "url json", url: "/rows.json", choose: { recordPath: ["_", "_"] } },
  { name: "url with query", url: "/api/export?format=csv" },
  { name: "url plain text", url: "/notes.txt" },
  { name: "url excel, all sheets", url: "/sheet.xlsx" },
]
