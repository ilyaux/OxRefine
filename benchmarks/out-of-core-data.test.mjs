import test from "node:test"
import assert from "node:assert/strict"
import { mkdtempSync, writeFileSync, readFileSync, existsSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { spec, rowAt, cellsAt, counts, valueAt, generateCsv, verifyCsv, header } from "./out-of-core-data.mjs"

test("small golden rows establish padding, error positions, group merge and end marker", () => {
  const s = spec({ rows: 2010, cardinality: "low", valueBytes: 32, columns: 4 })
  assert.equal(rowAt(0, s), "item-0000000000000000-abcdefghij,BAD,g0,block-0")
  assert.deepEqual(cellsAt(997, s, "errors"), ["ITEM-0000000000000005-ABCDEFGHIJ", "Unable to parse as number", "g5", ""])
  assert.equal(rowAt(2009, s, "merge"), "ITEM-0000000000000009-ABCDEFGHIJ,9,A,last")
  assert.deepEqual(counts(s), { rows: 2010, columns: 4, valueDistinct: 16, qtyErrors: 3, groupA: 1006, groupB: 1004 })
  assert.throws(() => spec({ ...s, format: "unknown" }), /unknown out-of-core dataset format/)
})

test("high cardinality has a different fixed-width value at every row", () => {
  const s = spec({ rows: 50_003, cardinality: "high", valueBytes: 64 })
  const values = new Set()
  for (let i = 0; i < s.rows; i++) {
    const value = valueAt(i, s)
    assert.equal(Buffer.byteLength(value), 64)
    values.add(value)
  }
  assert.equal(values.size, s.rows)
})

test("physical generated lines and manifest agree; wrong cells, ordering and truncation fail", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ox-ooc-data-"))
  try {
    const path = join(dir, "small.csv")
    const s = spec({ rows: 2101, cardinality: "high" })
    const manifest = await generateCsv(path, s)
    const got = await verifyCsv(path, s)
    assert.equal(got.rows, 2101)
    assert.equal(got.sha256, manifest.input.sha256)
    assert.equal(got.bytes, manifest.input.bytes)
    assert.equal(manifest.minimumCodeBytes, 2101 * 20 * 4)
    assert.equal(readFileSync(path, "utf8").split("\n")[1].split(",").length, 20)
    assert.equal(JSON.parse(readFileSync(`${path}.json`)).input.sha256, got.sha256)
    await assert.rejects(generateCsv(path, s), /refusing to overwrite/)
    const text = readFileSync(path, "utf8")
    writeFileSync(path, text.replace("g0", "g1"))
    await assert.rejects(verifyCsv(path, s), /CSV line 2/)
    writeFileSync(path, `${header(s)}\n${rowAt(1, s)}\n${rowAt(0, s)}\n`)
    await assert.rejects(verifyCsv(path, s), /CSV line 2/)
    writeFileSync(path, text.slice(0, text.lastIndexOf("\n", text.length - 2) + 1))
    await assert.rejects(verifyCsv(path, s), /row count differs/)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test("large generation requires explicit opt-in before creating anything", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ox-ooc-guard-"))
  const path = join(dir, "do-not-create.csv")
  try {
    await assert.rejects(generateCsv(path, spec({ rows: 50_000_001, cardinality: "low" })), /--allow-large/)
    assert.equal(existsSync(path), false)
    assert.equal(existsSync(`${path}.json`), false)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
