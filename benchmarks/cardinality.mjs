// Additional workloads for check.mjs; generated data has no personal or external data.
import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { createReadStream, existsSync, openSync, closeSync, writeSync, readdirSync, statSync } from "node:fs"
import { createInterface } from "node:readline"
import { join } from "node:path"
import { DATA, noFacets, withOx } from "./ox.mjs"

export const MODES = ["low", "mixed", "high"]
export const CARDINALITY_STEPS = ["import", "value", "merge", "history", "row", "structure", "undo", "redo", "save", "reopen", "export"]
const hash = () => createHash("sha256")
const line = (h, s) => h.update(`${s}\n`)
const transform = (expression) => ({ op: "core/text-transform", columnName: "value", expression: `grel:${expression}` })

export function distinctValues(mode, rows) {
  assert(MODES.includes(mode), `unknown cardinality ${mode}`)
  assert(Number.isSafeInteger(rows) && rows > 0, "rows must be a positive integer")
  return mode === "low" ? Math.min(16, rows) : mode === "mixed" ? Math.max(1, Math.ceil(rows / 10)) : rows
}

const label = (i, distinct) => `label-${String(i % distinct).padStart(10, "0")}`
export const inputRow = (i, distinct) => `${i}, ${label(i, distinct)} ,g${i % 8}`
export const finalRow = (i, distinct, history) => `${i % 8 < 4 ? "A" : "B"},${i},${label(i, distinct).toUpperCase()}${Array.from({ length: history }, (_, k) => `-h${k}`).join("")}-${i}`

/** New values at every history step retain dictionaries; merge changes the group column's codes. */
export function operations(history) {
  assert(Number.isSafeInteger(history) && history > 0, "history must be a positive integer")
  return {
    value: [transform("value.trim().toUppercase()")],
    row: [transform("value + '-' + row.index")],
    merge: [{ op: "core/mass-edit", columnName: "group", expression: "value", edits: [
      { from: ["g0", "g1", "g2", "g3"], to: "A" }, { from: ["g4", "g5", "g6", "g7"], to: "B" },
    ] }],
    history: Array.from({ length: history }, (_, i) => transform(`value + '-h${i}'`)),
    structure: [
      { op: "core/column-addition", baseColumnName: "value", newColumnName: "derived", columnInsertIndex: 3,
        expression: 'grel:value + ":" + cells["id"].value' },
      { op: "core/column-rename", oldColumnName: "group", newColumnName: "category" },
      { op: "core/column-move", columnName: "category", index: 0 },
      { op: "core/column-removal", columnName: "derived" },
    ],
  }
}

/** Canonical line hash ignores only the CSV line-ending convention, never cell values. */
export async function csvDigest(path) {
  const h = hash()
  let lines = 0
  for await (const s of createInterface({ input: createReadStream(path), crlfDelay: Infinity })) {
    line(h, s)
    lines++
  }
  return { sha256: h.digest("hex"), lines }
}

export async function cardinalityDataset(rows, mode) {
  const distinct = distinctValues(mode, rows)
  const path = join(DATA, `cardinality-v1-${mode}-${rows}.csv`)
  const expected = hash()
  line(expected, "id,value,group")
  const fd = existsSync(path) ? null : openSync(path, "wx")
  try {
    if (fd !== null) writeSync(fd, "id,value,group\n")
    let block = ""
    for (let i = 0; i < rows; i++) {
      const s = inputRow(i, distinct)
      line(expected, s)
      if (fd !== null) {
        block += `${s}\n`
        if (i % 10_000 === 9_999) { writeSync(fd, block); block = "" }
      }
    }
    if (fd !== null && block) writeSync(fd, block)
  } finally { if (fd !== null) closeSync(fd) }
  const digest = { sha256: expected.digest("hex"), lines: rows + 1 }
  assert.deepEqual(await csvDigest(path), digest, `cached dataset was changed: ${path}`)
  return { path, rows, mode, distinct, input: digest }
}

function libraryBytes(path) {
  return readdirSync(path, { withFileTypes: true }).reduce((n, e) => {
    const p = join(path, e.name)
    return n + (e.isDirectory() ? libraryBytes(p) : statSync(p).size)
  }, 0)
}

/** Milliseconds exclude the OS memory probe. RAM is process high-water-mark so far, not per-step allocation.
 * HTTP writes already autosave. `save` rewrites native history/meta using goto(current); reopen reads
 * the original table and replays saved operations after close, without using the in-memory project. */
export async function runCardinality(data, out, { history = 40, ...options } = {}) {
  const ops = operations(history)
  const final = hash()
  line(final, "category,id,value")
  for (let i = 0; i < data.rows; i++) line(final, finalRow(i, data.distinct, history))
  const expected = { sha256: final.digest("hex"), lines: data.rows + 1 }
  return withOx(async ({ ox, peak, store }) => {
    const r = { rows: data.rows, distinct: data.distinct, historySteps: history, idle: peak(), phasePeakMB: {} }
    const time = async (name, f) => {
      const t = performance.now()
      const v = await f()
      r[name] = performance.now() - t
      r.phasePeakMB[name] = peak()
      return v
    }
    const summary = await time("import", () => ox("open", { path: data.path }))
    assert.equal(summary.rows, data.rows)
    assert(summary.id, "benchmark needs a persisted project")
    let end = 0
    for (const step of ["value", "merge", "history", "row", "structure"]) {
      const s = await time(step, () => ox("apply_recipe", { ops: ops[step] }))
      end += ops[step].length
      assert.equal(s.cursor, end)
    }
    const undo = await time("undo", () => ox("goto", { index: 0 }))
    assert.equal(undo.cursor, 0)
    // CSV export intentionally trims cells like OpenRefine; verify raw undo cells through paged rows.
    for (let start = 0; start < data.rows; start += 1000) {
      const page = await ox("rows", { start, limit: 1000, engine: noFacets })
      assert.equal(page.total, data.rows)
      assert.equal(page.rows.length, Math.min(1000, data.rows - start))
      page.rows.forEach((row, j) => {
        const i = start + j
        assert.equal(row[0], i)
        assert.deepEqual(row[1], [String(i), ` ${label(i, data.distinct)} `, `g${i % 8}`], `undo row ${i}`)
      })
    }
    const redo = await time("redo", () => ox("goto", { index: end }))
    assert.equal(redo.cursor, end)
    await time("save", () => ox("goto", { index: end }))
    r.savedBytes = libraryBytes(store)
    await ox("close")
    const reopened = await time("reopen", () => ox("open_project", { id: summary.id }))
    assert.equal(reopened.cursor, end)
    assert.equal(reopened.history.length, end)
    assert.deepEqual(reopened.columns, ["category", "id", "value"])
    await time("export", () => ox("export", { engine: noFacets, path: out }))
    r.output = await csvDigest(out)
    assert.deepEqual(r.output, expected, "output differs from independently generated expected rows")
    r.peak = Math.max(r.idle, ...Object.values(r.phasePeakMB), peak())
    return r
  }, options)
}
