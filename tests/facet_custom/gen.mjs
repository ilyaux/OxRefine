// Regenerate only these preset cases with the real OpenRefine 3.10.1 distribution.
import { createHash } from "node:crypto"
import { readFileSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import vm from "node:vm"
import { OR_HOME, startOpenRefine } from "../../tools/openrefine.mjs"

// Verified against GitHub's official 3.10.1 Windows release ZIP. That release itself reports
// 3.10-SNAPSHOT/TRUNK over get-version, so pin the engine artifacts and retain the actual reply.
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
  const actual = createHash("sha256").update(readFileSync(join(OR_HOME, path))).digest("hex")
  if (actual !== expected) throw new Error(`Reference artifact differs from the verified release: ${path}`)
}
const sourcePath = "modules/core/scripts/views/data-table/menu-facets.js"
const source = readFileSync(join(OR_HOME, "webapp", sourcePath), "utf8")
const translation = JSON.parse(readFileSync(join(OR_HOME, "webapp/modules/core/langs/translation-en.json"), "utf8"))
const wanted = ["core/numeric-log-facet", "core/bounded-numeric-log-facet", "core/log-text-length-facet", "core/null-facet", "core/empty-string-facet"]
const menu = [], presets = []
vm.runInNewContext(source, {
  DataTableColumnHeaderUI: { extendMenu: (f) => f({ name: "v" }, {}, []) },
  MenuSystem: { appendTo: (_, __, items) => menu.push(...items), insertAfter: (_, __, items) => menu.push(...items) },
  FacetIconRegistry: { getIcon: () => null }, $: { i18n: (key) => translation[key] },
  ui: { browsingEngine: { addFacet(type, config) { presets.push({ label: current.label, type, expression: config.expression }) } } },
})
const flatten = (items) => items.flatMap((item) => [item, ...flatten(item.submenu ?? [])])
let current
for (const id of wanted) {
  current = flatten(menu).find((item) => item.id === id)
  if (!current) throw new Error(`OpenRefine menu is missing ${id}`)
  current.click()
}
const corpus = `id,v,gate
A,-10,keep
,0,drop
B,0.1,keep
,1,keep
C,10,drop
,100,keep
D,alpha,keep
,,drop
E,empty,keep
, ,keep
F,é😀,drop
,bad,keep
G,true,keep
,date,drop
H,10,keep
`
const ops = [{ op: "core/text-transform", engineConfig: { facets: [], mode: "row-based" }, columnName: "v",
  expression: "grel:if(rowIndex == 8, '', if(rowIndex == 12, true, if(rowIndex == 13, '2024-01-01'.toDate(), if(or(rowIndex < 6, rowIndex == 11), value.toNumber(), value))))",
  onError: "store-error", repeat: false, repeatCount: 10 }]
const facet = (preset, selected) => preset.type === "range" ? {
  type: "range", name: "v", columnName: "v", expression: preset.expression,
  from: selected ? 0 : -Number.MAX_VALUE, to: selected ? 1 : Number.MAX_VALUE,
  selectNumeric: true, selectNonNumeric: !selected, selectBlank: !selected, selectError: !selected,
} : {
  type: "list", name: "v", columnName: "v", expression: preset.expression,
  selection: selected ? [{ v: { v: true, l: "true" } }] : [], selectBlank: false, selectError: false, invert: false,
}
const gate = { type: "list", name: "gate", columnName: "gate", expression: "value",
  selection: [{ v: { v: "keep", l: "keep" } }], selectBlank: false, selectError: false, invert: false }
const or = await startOpenRefine({ port: 3347, workspace: join(tmpdir(), `oxrefine-facet-custom-${process.pid}`),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"] })
try {
  const version = await or.get("/command/core/get-version")
  if (!["3.10.1", "3.10-SNAPSHOT"].includes(version.version)) throw new Error(`Wrong reference version: ${JSON.stringify(version)}`)
  const project = await or.createProject(corpus, { name: "facet-custom" })
  const applied = await or.runOps(project, ops)
  if (applied.code !== "ok") throw new Error(`Reference setup failed: ${applied.message}`)
  const table = await or.table(project)
  const cases = []
  for (const preset of presets) {
    const preview = await or.command("preview-expression", { project, cellIndex: "1", rowIndices: JSON.stringify(Array.from({ length: 15 }, (_, i) => i)),
      expression: `grel:${preset.expression}`, repeat: "false", repeatCount: "10" })
    if (preview.code !== "ok") throw new Error(`${preset.label}: ${JSON.stringify(preview)}`)
    for (const [name, mode, selected] of [["all rows", "row-based", false], ["selected rows", "row-based", true], ["selected records", "record-based", true]]) {
      const engine = { facets: [facet(preset, selected), ...(selected ? [gate] : [])], mode }
      const { facets } = await or.command("compute-facets", { project, engine })
      const rows = await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000&engine=${encodeURIComponent(JSON.stringify(engine))}`)
      cases.push({ name: `${preset.label}: ${name}`, engine, facets, matching: rows.rows.map((row) => row.i), filtered: rows.filtered })
    }
    preset.preview = preview.results
  }
  const fixture = { openrefine: "3.10.1", reportedVersion: version, release, source: `https://github.com/OpenRefine/OpenRefine/blob/3.10.1/main/webapp/${sourcePath}`,
    menuSha256: createHash("sha256").update(source).digest("hex"), corpus, ops, table, presets, cases }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify(fixture, null, 1) + "\n")
  console.log(`${presets.length} actual-menu presets, ${cases.length} actual-reference facet/row cases -> tests/facet_custom/fixtures.json`)
  await or.command("delete-project", { project })
} finally { or.stop() }
