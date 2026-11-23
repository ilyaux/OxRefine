// Regenerate tests/cluster/custom-fixtures.json from a real OpenRefine 3.10.1:  node tests/cluster/custom-gen.mjs
// Custom clustering functions (OpenRefine 3.8+): key collision with a user-defined keying expression
// (`value` bound) and nearest neighbours with a user-defined distance (`value1`, `value2` bound), as
// the clustering dialog sends them (UserDefinedKeyer / UserDefinedDistance, params.expression).
// One core, as in gen.mjs: OpenRefine's kNN answer is complete and machine-independent only then.
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { VALUES } from "./cases.mjs"

const key = (expression) => ({ type: "binning", function: "UserDefinedKeyer", column: "v", params: { expression } })
const dist = (expression, radius, block) => ({ type: "knn", function: "UserDefinedDistance", column: "v", params: { expression, radius, "blocking-ngram-size": block } })
export const CUSTOM = [
  key("grel:value.toLowercase()"),
  key("grel:value.fingerprint()"),
  key("grel:value.length()"),
  key("grel:value.length() / 4.0"),
  key("grel:value.length() > 8"),
  key("grel:value.substring(0, 2)"),
  key("grel:null"),
  key("grel:PI"),
  key("grel:value.noSuchFunction()"),
  key("grel:[value.length()]"),
  key("jython:return value.lower()[0:3]"),
  key("clojure:(subs value 0 1)"),
  dist("grel:levenshteinDistance(value1, value2)", 1, 6),
  dist("grel:levenshteinDistance(value1, value2)", 2, 3),
  dist("grel:abs(value1.length() - value2.length())", 0, 3),
  dist("grel:if(value1.toLowercase() == value2.toLowercase(), 0, 10)", 0.5, 2),
  dist("grel:(value1.length() - value2.length()).toString()", 0, 4),
  dist("grel:value1.length() / 10.0", 0.55, 4),
  dist("grel:'far'", 1, 6),
  dist("grel:null", 1, 6),
  dist("grel:levenshteinDistance(value1, value2)", -1, 8),
  dist("jython:return abs(len(value1) - len(value2))", 0, 5),
  dist("clojure:(if (= value1 value2) 0 1)", 0, 4),
]

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-cluster-custom"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8", "-XX:ActiveProcessorCount=1"],
})
const quote = (v) => (/[",\n]/.test(v) || v !== v.trim() ? `"${v.replaceAll('"', '""')}"` : v)
try {
  const project = await or.createProject("v\n" + VALUES.map(quote).join("\n") + "\n", { name: "cluster" })
  const clusters = []
  for (const clusterer of CUSTOM) {
    const r = await or.command("compute-clusters", { project, engine: JSON.stringify({ facets: [], mode: "row-based" }), clusterer: JSON.stringify(clusterer) })
    const out = Array.isArray(r) ? r.map((c) => c.map((e) => [e.v, e.c])) : { error: String(r.message ?? r.stack ?? JSON.stringify(r)).slice(0, 300) }
    clusters.push({ clusterer, clusters: out })
    console.log(`${clusterer.params.expression.padEnd(60)} ${Array.isArray(out) ? out.length + " clusters" : "ERROR " + out.error.slice(0, 120)}`)
  }
  writeFileSync(new URL("custom-fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", generator: "tests/cluster/custom-gen.mjs", clusters }, null, 1) + "\n")
  console.log("-> tests/cluster/custom-fixtures.json")
} finally {
  or.stop()
}
