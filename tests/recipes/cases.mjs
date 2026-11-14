// Small real ecological datasets, plus explicitly modified inputs for recipe reuse.
import { readFileSync } from "node:fs"

const species = readFileSync(new URL("portal-species.csv", import.meta.url), "utf8")
const surveys = readFileSync(new URL("portal-surveys.csv", import.meta.url), "utf8")
const all = { facets: [], mode: "row-based" }
const transform = (columnName, expression, onError = "keep-original") => ({
  op: "core/text-transform", engineConfig: all, columnName, expression: `grel:${expression}`,
  onError, repeat: false, repeatCount: 10,
})
const scientificNames = [
  transform("genus", "value.trim()"),
  transform("species", "value.toLowercase()"),
  { op: "core/column-addition", engineConfig: all, baseColumnName: "genus", newColumnName: "scientificName", columnInsertIndex: 4,
    expression: "grel:value + ' ' + cells['species'].value", onError: "store-error" },
]
const measurements = [
  transform("hfl", "value.toNumber()", "store-error"),
  { op: "core/column-addition", engineConfig: all, baseColumnName: "hfl", newColumnName: "hfl_cm", columnInsertIndex: 9,
    expression: "grel:value / 10", onError: "store-error" },
]

export const CASES = [
  { name: "portal scientific names", csv: species, ops: scientificNames },
  { name: "portal changed names and blanks", csv: species.replace("Amphispiza,bilineata", " Amphispiza ,BILINEATA").replace("Baiomys,taylori", ","), ops: scientificNames },
  { name: "portal renamed required column", csv: species.replace("genus,species,", "genus,epithet,"), ops: scientificNames, failureStep: 2 },
  { name: "portal missing required column", csv: species.split("\n").map((line) => line.split(",").filter((_, i) => i !== 2).join(",")).join("\n"), ops: scientificNames, failureStep: 2 },
  { name: "portal numeric measurements", csv: surveys, ops: measurements },
  { name: "portal changed measurement types", csv: surveys.replace("M,32,", "M,unknown,").replace("M,33,", "M,33.5,"), ops: measurements, guessTypes: true },
  { name: "portal record selection", csv: species.replace("AH,Ammospermophilus", ",Ammospermophilus"), ops: [{
    op: "core/row-star", starred: true, engineConfig: { mode: "record-based", facets: [{ type: "text", name: "taxa", columnName: "taxa", query: "Bird", mode: "text", caseSensitive: false, invert: false }] },
  }] },
  { name: "portal unknown extension function", csv: species, ops: [scientificNames[0], transform("species", "value.portalExtension()")], failureStep: 2 },
  // OpenRefine includes Jython; OxRefine rejects it explicitly rather than claiming compatibility.
  { name: "portal Jython dependency", csv: species, ops: [scientificNames[0], { ...transform("species", "value"), expression: "jython:return value.upper()" }], oxFailureStep: 2, oxError: "expressions are not supported" },
  // A real operation shape emitted by OpenRefine's bundled Wikibase extension. Its runtime is not ported.
  { name: "portal Wikibase dependency", csv: species, ops: [scientificNames[0], { op: "wikidata/save-wikibase-schema", schema: {
    entityEdits: [], siteIri: "http://www.wikidata.org/entity/", entityTypeSiteIRI: { item: "http://www.wikidata.org/entity/" }, mediaWikiApiEndpoint: "https://www.wikidata.org/w/api.php",
  } }], oxPreflightError: "wikidata/save-wikibase-schema" },
]
