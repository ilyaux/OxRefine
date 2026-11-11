// Regenerate tests/cluster/fixtures.json from a real OpenRefine 3.10.1:  node tests/cluster/gen.mjs
// OpenRefine's kNN clusterer splits its work into one slice per CPU core and does not join neighbours
// found in different slices, so its clusters depend on the machine. With one core
// (-XX:ActiveProcessorCount=1) it gives its complete, machine-independent answer, which is what we keep.
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CLUSTERERS, ENCODINGS, VALUES } from "./cases.mjs"

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-cluster-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8", "-XX:ActiveProcessorCount=1"],
})
const quote = (v) => (/[",\n]/.test(v) || v !== v.trim() ? `"${v.replaceAll('"', '""')}"` : v)
try {
  const project = await or.createProject("v\n" + VALUES.map(quote).join("\n") + "\n", { name: "cluster" })
  const rows = (await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000`)).rows
  const values = rows.map((r) => r.cells[0]?.v ?? null)
  const indices = JSON.stringify(rows.map((_, i) => i))
  const phonetic = {}
  for (const enc of ENCODINGS) {
    const r = await or.command("preview-expression", { project, cellIndex: 0, expression: `grel:value.phonetic('${enc}')`, rowIndices: indices, repeat: false, repeatCount: 10 })
    phonetic[enc] = r.results.map((x) => (x && typeof x === "object" ? { error: x.message } : x))
    console.log(`phonetic ${enc}`)
  }
  const clusters = []
  for (const clusterer of CLUSTERERS) {
    const r = await or.command("compute-clusters", { project, engine: JSON.stringify({ facets: [], mode: "row-based" }), clusterer: JSON.stringify(clusterer) })
    const out = Array.isArray(r) ? r.map((c) => c.map((e) => [e.v, e.c])) : { error: String(r.message ?? r.stack ?? JSON.stringify(r)).slice(0, 300) }
    clusters.push({ clusterer, clusters: out })
    console.log(`${JSON.stringify(clusterer.params).padEnd(44)} ${clusterer.function.padEnd(18)} ${Array.isArray(out) ? out.length + " clusters" : "ERROR"}`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", values, phonetic, clusters }, null, 1) + "\n")
  console.log("-> tests/cluster/fixtures.json")
} finally {
  or.stop()
}
