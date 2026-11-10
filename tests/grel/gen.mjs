// Regenerate tests/grel/fixtures.json from a real OpenRefine 3.10.1:  node tests/grel/gen.mjs
// Each result is exactly what OpenRefine's expression preview shows: a string, null, or {"message"} for an error.
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CONSTANT, INPUTS, PER_ROW } from "./cases.mjs"

const csvCell = (s) => (/[",\n\t]/.test(s) || /^\s|\s$/.test(s) ? `"${s.replace(/"/g, '""')}"` : s)
const csv = ["v,w", ...INPUTS.map((r) => r.map(csvCell).join(","))].join("\n") + "\n"

const or = await startOpenRefine({ workspace: join(tmpdir(), "oxrefine-grel-fixtures"), jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"] })
try {
  const project = await or.createProject(csv, { name: "grel-fixtures" })
  const rows = INPUTS.map((_, i) => i)
  const preview = async (expression, rowIndices) => {
    const token = (await or.get("/command/core/get-csrf-token")).token
    const r = await fetch(`${or.url}/command/core/preview-expression?csrf_token=${token}`, {
      method: "POST",
      body: new URLSearchParams({ project, cellIndex: "0", rowIndices: JSON.stringify(rowIndices), expression: "grel:" + expression, repeat: "false", repeatCount: "10" }),
    })
    const j = await r.json()
    return j.code === "ok" ? { results: j.results } : { parseError: j.message }
  }
  // sanity: the import kept every input as typed
  const check = await preview("value", rows)
  INPUTS.forEach(([v], i) => {
    const got = check.results[i]
    if ((v === "" ? null : v) !== got) throw new Error(`row ${i} imported as ${JSON.stringify(got)}, expected ${JSON.stringify(v)}`)
  })
  const cases = []
  for (const expr of PER_ROW) cases.push({ expr, rows: "all", ...(await preview(expr, rows)) })
  for (const expr of CONSTANT) cases.push({ expr, rows: "first", ...(await preview(expr, [0])) })
  const out = { openrefine: "3.10.1", generated: new Date().toISOString().slice(0, 10), columns: ["v", "w"], inputs: INPUTS, cases }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify(out, null, 1) + "\n")
  const errors = cases.reduce((n, c) => n + (c.parseError ? 1 : c.results.filter((x) => x && typeof x === "object").length), 0)
  console.log(`${cases.length} expressions, ${cases.reduce((n, c) => n + (c.results?.length ?? 1), 0)} results (${errors} errors) -> tests/grel/fixtures.json`)
} finally {
  or.stop()
}
