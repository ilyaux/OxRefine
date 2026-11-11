// Regenerate tests/ops/fixtures.json from a real OpenRefine 3.10.1:  node tests/ops/gen.mjs
// Each case gets a fresh project from CORPUS; the result is the table in display order:
// cells are null | {"v": value} | {"v": iso, "t": "date"} | {"e": message}.
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, CORPUS } from "./cases.mjs"

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-ops-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
try {
  const out = []
  for (const c of CASES) {
    const project = await or.createProject(CORPUS, { name: "ops" })
    // core ops go through apply-operations; single edits through OpenRefine's own commands
    let res = { code: "ok" }
    for (let i = 0; i < c.ops.length && res.code === "ok"; ) {
      const op = c.ops[i]
      if (op.op === "ox/cell-edit") {
        const cols = (await or.get(`/command/core/get-models?project=${project}`)).columnModel.columns
        const cell = cols.find((x) => x.name === op.columnName).cellIndex
        res = await or.command("edit-one-cell", { project, row: op.row, cell, value: op.value, type: op.type })
        i++
      } else if (op.op === "ox/row-annotate") {
        const { op: _, row, ...what } = op
        res = await or.command("annotate-one-row", { project, row, ...what })
        i++
      } else {
        let j = i
        while (j < c.ops.length && !c.ops[j].op.startsWith("ox/")) j++
        res = await or.command("apply-operations", { project, operations: c.ops.slice(i, j) })
        await or.idle(project)
        i = j
      }
    }
    const history = (await or.get(`/command/core/get-history?project=${project}`)).past.map((h) => h.description)
    const operations = (await or.get(`/command/core/get-operations?project=${project}`)).entries.filter((e) => e.operation).length
    const models = await or.get(`/command/core/get-models?project=${project}`)
    const rows = (await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000`)).rows
    const cols = models.columnModel.columns
    const table = {
      columns: cols.map((x) => x.name),
      rows: rows.map((r) => ({ starred: r.starred, flagged: r.flagged, cells: cols.map((x) => r.cells[x.cellIndex] ?? null) })),
    }
    const entry = { name: c.name, ops: c.ops, ...table, history, operations }
    if (res.code !== "ok") entry.error = res.message ?? JSON.stringify(res)
    out.push(entry)
    await or.command("delete-project", { project })
    console.log(`${c.name.padEnd(36)} ${res.code}  ${table.columns.length} cols x ${table.rows.length} rows`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", corpus: CORPUS, cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/ops/fixtures.json`)
} finally {
  or.stop()
}
