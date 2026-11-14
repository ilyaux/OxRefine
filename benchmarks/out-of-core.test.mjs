import test from "node:test"
import assert from "node:assert/strict"
import { createServer } from "node:http"
import { jsonRequest, cancelImport, checkCodeBudget } from "./out-of-core.mjs"

test("JSON transport preserves backend failures and rejects an accidental table dump", async () => {
  const server = createServer((req, res) => {
    res.setHeader("Content-Type", "application/json")
    if (req.url === "/api/bad") { res.writeHead(400); res.end(JSON.stringify("no column named 'missing'")) }
    else if (req.url === "/api/large") res.end(JSON.stringify({ data: "x".repeat(9 * 2 ** 20) }))
    else res.end(JSON.stringify({ data: { rows: 50_000_001 }, ms: 1 }))
  })
  await new Promise((ok) => server.listen(0, "127.0.0.1", ok))
  const port = server.address().port
  try {
    assert.deepEqual(await jsonRequest(port, "test", "summary"), { rows: 50_000_001 })
    await assert.rejects(jsonRequest(port, "test", "bad"), (e) => e.status === 400 && e.message.includes("no column"))
    await assert.rejects(jsonRequest(port, "test", "large"), /8 MiB/)
  } finally { await new Promise((ok) => server.close(ok)) }
})

test("cancellation requires observable work, the cancel request and an actual cancelled result", async () => {
  let rejectWork, open = false, polls = 0
  const probe = { call: async (cmd) => {
    if (cmd === "projects") return []
    if (cmd === "processes") return open ? [{ id: 10, state: "running", done: ++polls, total: 1000, label: "Parsing" }] : []
    if (cmd === "open") { open = true; return new Promise((_, reject) => { rejectWork = reject }) }
    if (cmd === "cancel_process") { rejectWork(new Error("Cancelled")); return null }
    assert.fail(cmd)
  } }
  const c = await cancelImport(probe, "synthetic.csv", 50_000_001)
  assert.equal(c.id, 10)
  assert(c.progress > 0)
  assert.equal(c.error, "Cancelled")
})

test("a tiny import which finished before a cancel cannot masquerade as cancellation coverage", async () => {
  let cleaned = 0
  const probe = { call: async (cmd) => {
    if (cmd === "projects" || cmd === "processes") return []
    if (cmd === "delete_project") { cleaned++; return null }
    return { rows: 1, id: "new-owned-project" }
  } }
  await assert.rejects(cancelImport(probe, "tiny.csv", 1), /could not observe and cancel/)
  assert.equal(cleaned, 3)
})

test("code budget evidence rejects missing telemetry, unused policy and hidden heap overrun", () => {
  const manifest = { minimumCodeBytes: 4000, input: { bytes: 3000 } }
  const codes = { heap_bytes: 32, heap_peak_bytes: 64, mapped_bytes: 4000, mapped_peak_bytes: 4000,
    buffer_bytes: 0, buffer_peak_bytes: 65536, spill_files: 1 }
  const good = { codeHeapLimitBytes: 128, codes }
  const proof = checkCodeBudget(good, manifest, 128)
  assert(proof.verified)
  assert(proof.codeVolumeExceedsBudget)
  assert.equal(proof.totalHeapBounded, false)
  assert.equal(proof.bufferPeakBytes, 65536)
  assert.throws(() => checkCodeBudget(null, manifest, 128), /missing/)
  assert.throws(() => checkCodeBudget({ ...good, codeHeapLimitBytes: null }, manifest, 128), /requested/)
  assert.throws(() => checkCodeBudget({ ...good, codes: { ...codes, heap_peak_bytes: 129 } }, manifest, 128), /exceeded/)
  assert.throws(() => checkCodeBudget({ ...good, codes: { ...codes, mapped_bytes: 0 } }, manifest, 128), /no live spilled/)
})
