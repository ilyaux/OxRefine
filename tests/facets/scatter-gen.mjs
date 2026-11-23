// Regenerate tests/facets/scatter-fixtures.json from a real OpenRefine 3.10.1:  node tests/facets/scatter-gen.mjs
// Scatterplot facets: the rows a rectangle selection lets through, under linear and log scales and
// both rotations, on expressions, in records mode and next to other facets. The CSV's x, y and z
// columns are made numbers first (core/text-transform), as a scatterplot needs numeric columns.
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"

// key starts a record; x spreads evenly with negatives, y repeats and has a blank and a word, z grows fast (log scale)
const lines = ["key,x,y,z,label"]
for (let i = 0; i < 40; i++) {
  const y = i === 7 ? "" : i === 13 ? "n/a" : String(((i * 7) % 23) - 5)
  lines.push([i % 4 === 0 ? `r${i / 4}` : "", (i * 1.5 - 10).toFixed(1), y, String(i * i * 3 + 1), i % 3 === 0 ? "a" : "b"].join(","))
}
export const CSV = lines.join("\n") + "\n"
const number = (columnName) => ({ op: "core/text-transform", engineConfig: { facets: [], mode: "row-based" }, columnName, expression: "grel:value.toNumber()", onError: "keep-original", repeat: false, repeatCount: 10 })
export const OPS = [number("x"), number("y"), number("z")]

const scatter = (cx, cy, sel = {}, extra = {}) => ({
  type: "scatterplot", name: `${cx} (x) vs. ${cy} (y)`, cx, cy, l: 150, ex: "value", ey: "value", dot: 0.8, dim_x: "lin", dim_y: "lin", r: "none", ...sel, ...extra,
})
const box = (from_x, to_x, from_y, to_y) => ({ from_x, to_x, from_y, to_y })
const list = (columnName, values) => ({ type: "list", name: columnName, columnName, expression: "value", omitBlank: false, omitError: false, selection: values.map((v) => ({ v: { v, l: v } })), selectBlank: false, selectError: false, invert: false })
const rows = (...facets) => ({ facets, mode: "row-based" })
export const CASES = [
  { name: "no selection", engine: rows(scatter("x", "y")) },
  { name: "full box", engine: rows(scatter("x", "y", box(0, 1, 0, 1))) },
  { name: "lin box", engine: rows(scatter("x", "y", box(0.2, 0.6, 0.1, 0.7))) },
  { name: "lin corner", engine: rows(scatter("x", "y", box(0.5, 1, 0.5, 1))) },
  { name: "edges exactly", engine: rows(scatter("x", "y", box(0, 0.5, 0, 0.5))) },
  { name: "log x", engine: rows(scatter("z", "y", box(0.3, 0.9, 0, 0.6), { dim_x: "log" })) },
  { name: "log both", engine: rows(scatter("z", "x", box(0.1, 0.8, 0.2, 0.9), { dim_x: "log", dim_y: "log" })) },
  { name: "lin z for log comparison", engine: rows(scatter("z", "x", box(0.1, 0.8, 0.2, 0.9))) },
  { name: "rotated clockwise", engine: rows(scatter("x", "y", box(0.2, 0.7, 0.2, 0.7), { r: "cw" })) },
  { name: "rotated counter-clockwise", engine: rows(scatter("x", "y", box(0.2, 0.7, 0.2, 0.7), { r: "ccw" })) },
  { name: "rotated clockwise, low corner", engine: rows(scatter("x", "y", box(0, 0.4, 0, 0.4), { r: "cw" })) },
  { name: "expressions", engine: rows(scatter("x", "y", box(0.25, 0.75, 0.25, 0.75), { ex: "value * value", ey: "value - 3" })) },
  { name: "text expression on y", engine: rows(scatter("x", "label", box(0, 0.5, 0, 1), { ey: "if(value == 'a', 1, 2)" })) },
  { name: "with a list facet", engine: rows(scatter("x", "y", box(0.2, 0.6, 0.1, 0.7)), list("label", ["a"])) },
  { name: "records", engine: { facets: [scatter("x", "y", box(0.6, 1, 0, 0.4))], mode: "record-based" } },
  { name: "same column twice", engine: rows(scatter("x", "x", box(0.3, 0.6, 0, 1))) },
  // OpenRefine reads the scale and rotation as numbers (0 lin, 1 log; 0 none, 1 cw, 2 ccw) and ignores
  // the words its own UI sends: those cases above filter like linear, unrotated plots
  { name: "log x as a number", engine: rows(scatter("z", "y", box(0.3, 0.9, 0, 0.6), { dim_x: 1 })) },
  { name: "log both as numbers", engine: rows(scatter("z", "x", box(0.1, 0.8, 0.2, 0.9), { dim_x: 1, dim_y: 1 })) },
  { name: "clockwise as a number", engine: rows(scatter("x", "y", box(0.2, 0.7, 0.2, 0.7), { r: 1 })) },
  { name: "counter-clockwise as a number", engine: rows(scatter("x", "y", box(0.2, 0.7, 0.2, 0.7), { r: 2 })) },
  { name: "clockwise low corner as a number", engine: rows(scatter("x", "y", box(0, 0.4, 0, 0.4), { r: 1 })) },
  { name: "counter-clockwise high corner as a number", engine: rows(scatter("x", "y", box(0.5, 1, 0.5, 1), { r: 2 })) },
  { name: "log and rotation as numbers", engine: rows(scatter("z", "y", box(0.1, 0.6, 0.3, 0.9), { dim_x: 1, r: 1 })) },
  { name: "unrotated box for comparison", engine: rows(scatter("x", "y", box(0.2, 0.7, 0.2, 0.7))) },
]

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-scatter-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"],
})
try {
  const project = await or.createProject(CSV, { name: "scatter" })
  if ((await or.runOps(project, OPS)).code !== "ok") throw new Error("ops")
  const out = []
  for (const c of CASES) {
    const facets = (await or.command("compute-facets", { project, engine: c.engine })).facets
    const engine = encodeURIComponent(JSON.stringify(c.engine))
    const got = await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000&engine=${engine}`)
    // the plot itself is a PNG; keep what the facet reports about its data
    const scatter = facets.find((f) => !("choices" in f)) ?? {}
    delete scatter.image
    out.push({ ...c, facet: scatter, matching: got.rows.map((r) => r.i), filtered: got.filtered })
    console.log(`${c.name.padEnd(36)} ${String(got.rows.length).padStart(3)} rows  ${JSON.stringify(scatter).slice(0, 150)}`)
  }
  writeFileSync(new URL("scatter-fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", generator: "tests/facets/scatter-gen.mjs", csv: CSV, ops: OPS, cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/facets/scatter-fixtures.json`)
} finally {
  or.stop()
}
