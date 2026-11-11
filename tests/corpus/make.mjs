// Writes the messy-data corpus into tests/corpus/files:  node tests/corpus/make.mjs
// Each file is small but awkward in one way real-world exports are: encodings, byte order marks,
// ragged rows, line endings, embedded newlines, huge or wide cells, mixed types, broken quotes.
// The files are committed as exact bytes (see .gitattributes); this script documents how they were made.
import { mkdirSync, writeFileSync } from "node:fs"

const dir = new URL("files/", import.meta.url)
mkdirSync(dir, { recursive: true })
const write = (name, data) => writeFileSync(new URL(name, dir), data)

const utf16 = (text, bigEndian) => {
  const le = Buffer.from("﻿" + text, "utf16le")
  if (bigEndian) for (let i = 0; i < le.length; i += 2) [le[i], le[i + 1]] = [le[i + 1], le[i]]
  return le
}
// Windows-1252 for the characters used below (Node has no encoder for it)
const CP1252 = { "€": 0x80, "‚": 0x82, "„": 0x84, "…": 0x85, "‘": 0x91, "’": 0x92, "“": 0x93, "”": 0x94, "–": 0x96, "—": 0x97, "™": 0x99 }
const cp1252 = (text) => Buffer.from([...text].map((c) => CP1252[c] ?? (c.charCodeAt(0) < 256 ? c.charCodeAt(0) : 0x3f)))

const accents = "name,city,note\nZoë,Kraków,naïve café\n李雷,北京,😀 emoji\nRené,Zürich,\"quoted, with comma\"\n"

write("utf8-bom.csv", "﻿" + accents)
write("utf8-no-bom.csv", accents)
write("utf16le-bom.csv", utf16(accents, false))
write("utf16be-bom.tsv", utf16(accents.replaceAll(",", "\t").replace('"quoted\t with comma"', "tab\tinside"), true))
write("windows-1252.csv", cp1252("name,price,quote\nCafé,€5,“smart” quotes\nNaïve,£3,–dash– and ‘single’\nCrème brûlée,™9,ellipsis…\n"))

write("ragged.csv", "a,b,c\n1,2\n3,4,5,6,7\n\n8\n,,\n9,10,11\n")
write("crlf-embedded-newlines.csv", 'id,text\r\n1,"line one\r\nline two"\r\n2,"only lf\ninside"\r\n3,plain\r\n')
write("cr-only.csv", "a,b\r1,2\r3,4\r")
write("mixed-line-endings.csv", "a,b\n1,2\r\n3,4\r5,6\n")
write("no-final-newline.csv", "a,b\n1,2\n3,4")
write("header-only.csv", "a,b,c\n")
write("dup-blank-headers.csv", "name,,name,Column 2,\n1,2,3,4,5\n6,7,8,9,10\n")

write("huge-cell.csv", `id,blob,after\n1,"${"lorem ipsum ".repeat(10_000)}",x\n2,short,y\n`)
const wide = Array.from({ length: 500 }, (_, i) => `c${i + 1}`)
write("wide.csv", [wide.join(","), wide.map((_, i) => i).join(","), wide.map((_, i) => (i % 7 ? `v${i}` : "")).join(",")].join("\n") + "\n")

write(
  "mixed-types.csv",
  "label,value\n" +
    [
      ["int", "42"], ["negative", "-7"], ["decimal", "3.14"], ["exponent", "1e5"], ["thousands", '"1,234"'], ["leading zeros", "007"],
      ["hex", "0x1F"], ["padded", '" 42 "'], ["negative zero", "-0"], ["beyond long", "9223372036854775808"], ["nan", "NaN"],
      ["infinity", "Infinity"], ["bool", "true"], ["bool caps", "FALSE"], ["iso date", "2024-01-15"], ["dmy date", "15/01/2024"],
      ["iso datetime", "2024-01-15T10:20:30Z"], ["percent", "12%"], ["currency", "$5.00"], ["blank", ""],
    ]
      .map((r) => r.join(","))
      .join("\n") +
    "\n",
)
write("semicolon-decimal-comma.csv", "name;price;qty\nA;1,50;3\nB;2,75;10\nC;;0\n")
write("broken-quotes.csv", 'a,b\n"ok ""escaped""",1\nmid"quote,2\n"unclosed,3\n4,5\n')
write("whitespace.csv", "a,b\n  spaced  , nbsp \n\ttab\t, em space \nzero​width,\u0007bell\n")
write("pipe-separated.txt", "id|name|note\n1|Ann|a,b\n2|Bob|c;d\n")

write(
  "messy.json",
  JSON.stringify([
    { id: 1, name: "Ann", tags: ["a", "b"], addr: { city: "Paris" }, score: null },
    { id: "2", name: " Bob ", extra: true, f: 1.5e300, empty: "", addr: { city: "Lyon", zip: "69001" } },
    { id: 3, name: "été ☃", nested: { deep: { deeper: [1, 2, 3] } } },
  ]),
)
// beyond-long integers in JSON are written by hand; JSON.stringify would round them
write("big-numbers.json", '[{"n":12345678901234567890,"m":-9223372036854775808,"d":0.1},{"n":1,"m":2,"d":1e-7}]')
console.log("corpus written to tests/corpus/files")
