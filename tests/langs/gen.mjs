// Regenerate tests/langs/fixtures.json from a real OpenRefine 3.10.1:  node tests/langs/gen.mjs
// Each result is exactly what OpenRefine's expression preview shows: a string, null, or {"message"} for an error.
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CLOJURE_CONSTANT, CLOJURE_ROWS, INPUTS, JYTHON_CONSTANT, JYTHON_ROWS } from "./cases.mjs"

const csvCell = (s) => (/[",\n\t]/.test(s) || /^\s|\s$/.test(s) ? `"${s.replace(/"/g, '""')}"` : s)
const csv = ["v,w", ...INPUTS.map((r) => r.map(csvCell).join(","))].join("\n") + "\n"

const or = await startOpenRefine({ workspace: join(tmpdir(), "oxrefine-lang-fixtures"), jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"] })
try {
  const project = await or.createProject(csv, { name: "lang-fixtures" })
  const rows = INPUTS.map((_, i) => i)
  const preview = async (expression, rowIndices) => {
    const token = (await or.get("/command/core/get-csrf-token")).token
    const r = await fetch(`${or.url}/command/core/preview-expression?csrf_token=${token}`, {
      method: "POST",
      body: new URLSearchParams({ project, cellIndex: "0", rowIndices: JSON.stringify(rowIndices), expression, repeat: "false", repeatCount: "10" }),
    })
    const j = await r.json()
    return j.code === "ok" ? { results: j.results } : { parseError: j.message }
  }
  const cases = []
  for (const [lang, perRow, constant] of [["jython", JYTHON_ROWS, JYTHON_CONSTANT], ["clojure", CLOJURE_ROWS, CLOJURE_CONSTANT]]) {
    for (const expr of perRow) cases.push({ lang, expr, rows: "all", ...(await preview(`${lang}:${expr}`, rows)) })
    for (const expr of constant) cases.push({ lang, expr, rows: "first", ...(await preview(`${lang}:${expr}`, [0])) })
  }
  const out = { openrefine: "3.10.1", jython: "2.7.4", clojure: "1.12.3", generated: new Date().toISOString().slice(0, 10), columns: ["v", "w"], inputs: INPUTS, cases }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify(out, null, 1) + "\n")
  console.log(`${cases.length} expressions -> tests/langs/fixtures.json`)
} finally {
  or.stop()
}
