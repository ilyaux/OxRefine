import test from "node:test"
import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { mkdtempSync, writeFileSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { createServer } from "node:net"
import { csvDigest } from "./cardinality.mjs"
import { withOx } from "./ox.mjs"

test("output equality tolerates line endings but catches cell whitespace and missing rows", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ox-bench-digest-"))
  try {
    const a = join(dir, "a.csv"), b = join(dir, "b.csv")
    writeFileSync(a, "id,value\n0, label \n1,x\n")
    writeFileSync(b, "id,value\r\n0, label \r\n1,x\r\n")
    const expected = await csvDigest(a)
    assert.deepEqual(await csvDigest(b), expected)
    writeFileSync(b, "id,value\n0,label\n1,x\n")
    assert.notDeepEqual(await csvDigest(b), expected)
    writeFileSync(b, "id,value\n0, label \n")
    assert.notDeepEqual(await csvDigest(b), expected)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test("bad workload sizes and profile names fail before launching a server", () => {
  for (const args of [["--profile", "typo"], ["--runs", "0"], ["--history", "-1"], ["--new"], ["--out"], ["not-a-row-count"]]) {
    const r = spawnSync(process.execPath, [join(import.meta.dirname, "check.mjs"), "--record", ...args], { encoding: "utf8" })
    assert.equal(r.status, 2, r.stderr)
    assert.match(r.stderr, /usage:/)
  }
})

test("an occupied port is rejected without contacting its owner", async () => {
  let contacted = false
  const server = createServer((s) => { contacted = true; s.destroy() })
  await new Promise((ok) => server.listen(0, "127.0.0.1", ok))
  try {
    await assert.rejects(withOx(() => assert.fail("should not run"), { bin: "not-a-binary", port: server.address().port }), { code: "EADDRINUSE" })
    assert.equal(contacted, false)
  } finally { await new Promise((ok) => server.close(ok)) }
})

test("missing binary reports spawn failure rather than waiting two minutes", async () => {
  const probe = createServer()
  await new Promise((ok) => probe.listen(0, "127.0.0.1", ok))
  const port = probe.address().port
  await new Promise((ok) => probe.close(ok))
  await assert.rejects(withOx(() => assert.fail("should not run"), { bin: join(tmpdir(), "ox-bench-nonexistent-binary"), port }), { code: "ENOENT" })
})
