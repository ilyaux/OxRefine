// Regenerate byte-exact charset references from OpenRefine 3.10.1: node tests/export/encoding-gen.mjs
// BOM controls and strict failures are OxRefine additions, not claims about OpenRefine options.
import { createHash } from "node:crypto"
import { readFileSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { OR_HOME, startOpenRefine } from "../../tools/openrefine.mjs"

// This verified official ZIP itself reports 3.10-SNAPSHOT/TRUNK. Pin actual engine bytes,
// and record the unmodified runtime version rather than overriding its version resources.
const release = {
  url: "https://github.com/OpenRefine/OpenRefine/releases/download/3.10.1/openrefine-win-with-java-3.10.1.zip",
  sha256: "6155b20a8103244a26a8f21d41698ee52251538a838e3d5f2d5af507c8b6f3c8",
  artifacts: {
    "webapp/WEB-INF/lib/openrefine-main.jar": "b1bee728e85b4aa44967427707ecdb501a6b4aa3d87608ba7b29115c858f0957",
    "webapp/WEB-INF/lib/grel-3.10.1.jar": "7158cd697ab4f6b43e21af7484a9eba1a8ac355759b3267d7f6ec9018f39e8dd",
    "webapp/WEB-INF/lib/core-3.10.1.jar": "a135638474451a220705560624e5c6a0720840edc9d7150b29132a30a3e3ac38",
    "server/target/lib/openrefine-3.10.1-server.jar": "5906b7db2ad82332cb9295490759b189adf5ef79939cb14b02878aff2903aece",
  },
}
for (const [path, expected] of Object.entries(release.artifacts)) {
  if (createHash("sha256").update(readFileSync(join(OR_HOME,path))).digest("hex") !== expected) throw new Error(`Reference artifact differs from the verified release: ${path}`)
}

const controls = Array.from({ length: 32 }, (_, i) => String.fromCharCode(0x80 + i)).join("")
const corpus = `name,city,note\nZoë,München,"café €; ""quote"""\nИлья,Москва,Привет\n😀,東京,astral\nControl,Latin,"${controls}"\n,,\n`
const all = { mode: "row-based", facets: [] }
const row = (query) => ({ mode: "row-based", facets: [{ type: "text", name: "name", columnName: "name", query, mode: "text", caseSensitive: true, invert: false }] })
const options = { separator: ";", lineSeparator: "\n", outputColumnHeaders: true, outputBlankRows: true, columns: [{ name: "name" }, { name: "city" }, { name: "note" }] }
const blankCorpus = "value,second,marker\nnull,,keep\nempty,,keep\nspaces,,keep\ntext,,keep\n"
const blankSetup = [{ op:"core/text-transform", columnName:"value", expression:"grel:if(value == 'null', null, if(value == 'empty', '', if(value == 'spaces', '   ', value)))", engineConfig:all, onError:"keep-original", repeat:false, repeatCount:10 }]
const cases = [
  { name:"CSV defaults UTF-8 without BOM or all-null rows",format:"csv",params:{} },
  { name:"custom defaults without options",format:"*sv",params:{} },
  ...["UTF-8", "UTF-16", "UTF-16LE", "UTF-16BE"].map((encoding) => ({ name: `custom ${encoding} unicode`, format: "*sv", params: { encoding, options } })),
  { name: "CSV UTF-16LE quotes and CRLF", format: "csv", params: { encoding: "UTF-16LE" } },
  { name: "TSV UTF-16BE", format: "tsv", params: { encoding: "UTF-16BE" } },
  { name: "windows-1251 Cyrillic", format: "*sv", engine: row("Илья"), params: { encoding: "windows-1251", options } },
  { name: "windows-1252 Latin and euro", format: "csv", engine: row("Zoë"), params: { encoding: "windows-1252" } },
  { name: "ISO-8859-1 Latin", format: "*sv", engine: row("Zoë"), params: { encoding: "ISO-8859-1", options: { ...options, columns: [{ name: "name" }, { name: "city" }] } } },
  { name: "ISO-8859-1 C1 controls are not windows-1252", format: "csv", engine: row("Control"), params: { encoding: "ISO-8859-1" } },
  { name: "US-ASCII represented text", format: "*sv", engine: row("Control"), params: { encoding: "US-ASCII", options: { ...options, columns: [{ name: "name" }, { name: "city" }] } } },
  { name: "windows-1251 replacement", format: "csv", params: { encoding: "windows-1251", encodingErrors: "replace" } },
  { name: "windows-1252 replacement", format: "csv", params: { encoding: "windows-1252", encodingErrors: "replace" } },
  { name: "ISO-8859-1 replacement", format: "csv", params: { encoding: "ISO-8859-1", encodingErrors: "replace" } },
  { name: "US-ASCII replacement", format: "csv", params: { encoding: "US-ASCII", encodingErrors: "replace" } },
  { name: "UTF-16 no selected rows or headers", format: "*sv", engine: row("no-match"), params: { encoding: "UTF-16", options: { ...options, outputColumnHeaders: false } } },
  { name: "UTF-16LE selected columns and quote-all", format: "*sv", engine: row("Zoë"), params: { encoding: "UTF-16LE", options: { ...options, quoteAll: true, outputColumnHeaders: false, columns: [{ name: "note" }, { name: "name" }] } } },
  { name: "UTF-16BE records retain sibling rows", format: "*sv", engine: { mode: "record-based", facets: [{ type: "text", name: "note", columnName: "note", query: "sibling", mode: "text", caseSensitive: true, invert: false }] }, corpus: "name,note\nrecord,root\n,sibling\nother,skip\n", params: { encoding: "UTF-16BE", options: { ...options, columns: [{ name: "name" }, { name: "note" }] } } },
  ...[
    ["csv", undefined], ["tsv",undefined], ["*sv",undefined],
    ["csv",{}], ["csv",{outputBlankRows:false}], ["csv",{outputBlankRows:true}],
    ["*sv",{outputBlankRows:false}], ["*sv",{outputBlankRows:true}],
  ].map(([format,blankOptions],i) => ({name:`null vs text-empty ${format} options ${i}`,format,corpus:blankCorpus,setup:blankSetup,params:{encoding:"UTF-8",options:{columns:[{name:"value"},{name:"second"}],...blankOptions}}})),
]

const or = await startOpenRefine({ port: 3340, workspace: fileURLToPath(new URL("../../target/export-encoding-reference/", import.meta.url)), jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"] })
try {
  const runtime = await or.get("/command/core/get-version")
  if (!["3.10.1","3.10-SNAPSHOT"].includes(runtime.version)) throw new Error(`Wrong reference version: ${JSON.stringify(runtime)}`)
  const project = await or.createProject(corpus, { name: "encoding-reference" })
  const out = []
  for (const c of cases) {
    const target = c.corpus ? await or.createProject(c.corpus, { name: c.name }) : project
    if (c.setup) {
      const result=await or.runOps(target,c.setup)
      if (result.code!=="ok") throw new Error(`Reference setup failed: ${JSON.stringify(result)}`)
    }
    const response = await or.command("export-rows", { project: target, format: c.format, engine: c.engine ?? all, ...c.params,
      ...(c.params.options ? { options: JSON.stringify(c.params.options) } : {}) }, { json: false })
    const bytes = Buffer.from(await response.arrayBuffer())
    out.push({ ...c, engine: c.engine ?? all, ...(c.setup ? {referenceTable:await or.table(target)} : {}), outputBase64: bytes.toString("base64") })
    console.log(`${c.name}: ${bytes.length} bytes`)
  }
  writeFileSync(new URL("encoding-fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", reportedVersion:runtime, release, source: "https://github.com/OpenRefine/OpenRefine/blob/3.10.1/main/src/com/google/refine/commands/project/ExportRowsCommand.java", corpus, policy: "OpenRefine replaces unmappable characters with the Java charset replacement. OxRefine requires encodingErrors=replace explicitly; its default is a strict preflight error. bom is an OxRefine option; these references do not send it.", cases: out }, null, 1) + "\n")
} finally {
  or.stop()
}
