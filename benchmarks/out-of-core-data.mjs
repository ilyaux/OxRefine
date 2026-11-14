// Real streamed CSV rows, not a virtual/repeated fixture. No external or personal data.
import assert from "node:assert/strict"
import { createHash, randomUUID } from "node:crypto"
import { createReadStream, existsSync, mkdirSync, statfsSync, statSync, readFileSync } from "node:fs"
import { open, link, unlink } from "node:fs/promises"
import { createInterface } from "node:readline"
import { dirname, resolve } from "node:path"
import { pathToFileURL } from "node:url"

export const FORMAT = "oxrefine-ooc-v1"
export const HEADER = "value,qty,group,checkpoint"
export const ERROR = "Unable to parse as number" // OpenRefine 3.10.1: tests/grel/fixtures.json toNumber(value)
export const LARGE_ROWS = 50_000_001

export function spec({ format = FORMAT, rows, cardinality, valueBytes = 32, columns = 20 }) {
  assert.equal(format, FORMAT, "unknown out-of-core dataset format")
  assert(Number.isSafeInteger(rows) && rows > 0 && rows <= 0xffff_ffff, "rows must be in 1..4294967295")
  assert(["low", "high"].includes(cardinality), "cardinality must be low or high")
  assert(Number.isSafeInteger(valueBytes) && valueBytes >= 32 && valueBytes <= 4096, "valueBytes must be in 32..4096")
  assert(Number.isSafeInteger(columns) && columns >= 4 && columns <= 128, "columns must be in 4..128")
  return { format: FORMAT, rows, cardinality, valueBytes, columns }
}

export const header = (s) => [HEADER, ...Array.from({ length: s.columns - 4 }, (_, i) => `extra${i}`)].join(",")

export function valueAt(i, s) {
  const k = s.cardinality === "low" ? i % 16 : i
  const prefix = `item-${String(k).padStart(16, "0")}-`
  return (prefix + "abcdefghijklmnopqrstuvwxyz".repeat(Math.ceil(s.valueBytes / 26))).slice(0, s.valueBytes)
}

export function cellsAt(i, s, phase = "input") {
  assert(Number.isSafeInteger(i) && i >= 0 && i < s.rows, "row outside dataset")
  assert(["input", "uppercase", "errors", "merge"].includes(phase), "unknown oracle phase")
  const qty = i % 997 === 0 ? phase === "errors" || phase === "merge" ? ERROR : "BAD" : String(i % 1000)
  const group = phase === "merge" ? i % 8 < 4 ? "A" : "B" : `g${i % 8}`
  const checkpoint = i === s.rows - 1 ? "last" : i % 1_000_000 === 0 ? `block-${i / 1_000_000}` : ""
  return [phase === "input" ? valueAt(i, s) : valueAt(i, s).toUpperCase(), qty, group, checkpoint,
    ...Array.from({ length: s.columns - 4 }, (_, j) => String.fromCharCode(97 + (i + j) % (2 + j % 7)))]
}

export const rowAt = (i, s, phase = "input") => cellsAt(i, s, phase).join(",")

export function counts(s) {
  return { rows: s.rows, columns: s.columns, valueDistinct: s.cardinality === "low" ? Math.min(16, s.rows) : s.rows,
    qtyErrors: Math.floor((s.rows - 1) / 997) + 1,
    groupA: Math.floor(s.rows / 8) * 4 + Math.min(s.rows % 8, 4),
    groupB: Math.floor(s.rows / 8) * 4 + Math.max(s.rows % 8 - 4, 0) }
}

/** Conservative upper bound before writing. Existing input and result files are never overwritten. */
export function estimatedInputBytes(s) {
  return header(s).length + 1 + s.rows * (s.valueBytes + 4 + 3 + 24 + 4 + (s.columns - 4) * 2)
}

export function freeDiskBytes(path) {
  const f = statfsSync(path, { bigint: true })
  return f.bavail * f.bsize
}

/** Every line is checked against an independently evaluated row formula. Memory stays bounded by
 * the line reader. No trimming, sorting, sampling, row skipping or `rows` field trust. */
export async function verifyCsv(path, s, phase = "input") {
  const expected = spec(s)
  const digest = createHash("sha256")
  let lines = 0, qtyErrorTexts = 0, mergedA = 0, mergedB = 0
  for await (const line of createInterface({ input: createReadStream(path), crlfDelay: Infinity })) {
    const want = lines === 0 ? header(expected) : rowAt(lines - 1, expected, phase)
    assert.equal(line, want, `${phase} CSV line ${lines + 1}`)
    if (lines > 0) {
      const cells = line.split(",")
      if (cells[1] === ERROR) qtyErrorTexts++
      if (cells[2] === "A") mergedA++
      if (cells[2] === "B") mergedB++
    }
    digest.update(`${line}\n`)
    lines++
  }
  assert.equal(lines, expected.rows + 1, "CSV row count differs")
  return { lines, rows: lines - 1, sha256: digest.digest("hex"), bytes: statSync(path).size,
    qtyErrorTexts, mergedA, mergedB, errorCaveat: "CSV text equality does not by itself prove backend error cell types" }
}

export async function generateCsv(path, input, { allowLarge = false, progress = () => {} } = {}) {
  const s = spec(input)
  assert(s.rows < 1_000_000 || allowLarge, "large generation requires explicit --allow-large")
  assert(!existsSync(path), `refusing to overwrite ${path}`)
  assert(!existsSync(`${path}.json`), `refusing to overwrite ${path}.json`)
  mkdirSync(dirname(path), { recursive: true })
  assert(freeDiskBytes(dirname(path)) > BigInt(estimatedInputBytes(s) + 64 * 2 ** 20), "not enough free disk for the estimated input plus 64 MiB reserve")
  const temp = `${path}.${randomUUID()}.partial`
  const f = await open(temp, "wx")
  const hash = createHash("sha256")
  let bytes = 0
  const started = performance.now()
  const flush = async (block) => { hash.update(block); bytes += Buffer.byteLength(block); await f.writeFile(block) }
  try {
    await flush(`${header(s)}\n`)
    let block = ""
    for (let i = 0; i < s.rows; i++) {
      block += `${rowAt(i, s)}\n`
      if (block.length >= 2 ** 20) { await flush(block); block = ""; progress({ rows: i + 1, bytes }) }
    }
    if (block) await flush(block)
    await f.sync()
    await f.close()
    // Linking publishes without replacing a destination created concurrently.
    await link(temp, path)
    await unlink(temp)
    const manifest = { ...s, counts: counts(s), minimumCodeBytes: s.rows * s.columns * 4,
      input: { sha256: hash.digest("hex"), bytes, rows: s.rows },
      generatedMs: performance.now() - started, oracle: "every physical CSV line derives from the row number; high values are all distinct" }
    const mf = await open(`${path}.json`, "wx")
    try { await mf.writeFile(`${JSON.stringify(manifest, null, 2)}\n`) } finally { await mf.close() }
    return manifest
  } catch (e) {
    await f.close().catch(() => {})
    await unlink(temp).catch(() => {})
    throw e
  }
}

async function main() {
  const args = process.argv.slice(2)
  const command = args.shift()
  const opt = (key, fallback) => {
    const i = args.indexOf(key)
    if (i < 0) return fallback
    assert(args[i + 1] && !args[i + 1].startsWith("--"), `missing ${key} value`)
    return args.splice(i, 2)[1]
  }
  const path = opt("--file")
  const rows = Number(opt("--rows"))
  const cardinality = opt("--cardinality")
  const valueBytes = Number(opt("--value-bytes", 32))
  const columns = Number(opt("--columns", 20))
  const phase = opt("--phase", "input")
  const allowLarge = args.includes("--allow-large")
  if (allowLarge) args.splice(args.indexOf("--allow-large"), 1)
  assert(!args.length && path && ["generate", "verify"].includes(command),
    "usage: node benchmarks/out-of-core-data.mjs generate|verify --file FILE --rows N --cardinality low|high [--value-bytes 32] [--columns 20] [--phase input|uppercase|errors|merge] [--allow-large]")
  const s = spec({ rows, cardinality, valueBytes, columns })
  if (command === "generate") {
    let last = 0
    console.log(JSON.stringify(await generateCsv(resolve(path), s, { allowLarge, progress: (p) => {
      if (performance.now() - last > 10_000) { console.error(JSON.stringify(p)); last = performance.now() }
    } }), null, 2))
  } else {
    const got = await verifyCsv(resolve(path), s, phase)
    if (phase === "input" && existsSync(`${path}.json`)) {
      const manifest = JSON.parse(readFileSync(`${path}.json`, "utf8"))
      assert.deepEqual(spec(manifest), s)
      assert.equal(got.sha256, manifest.input.sha256, "input hash differs from generation manifest")
    }
    console.log(JSON.stringify(got, null, 2))
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  main().catch((e) => { console.error(e.message); process.exitCode = 1 })
}
