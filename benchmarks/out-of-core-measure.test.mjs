import test from "node:test"
import assert from "node:assert/strict"
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, openSync, closeSync, unlinkSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { parseLinuxMemory, processMemory, diskUsage, anonymousSpillDisk } from "./out-of-core-measure.mjs"

test("mmap resident pages are never presented as heap or private committed memory", () => {
  const m = parseLinuxMemory("VmRSS: 2000 kB\nVmHWM: 3000 kB\nVmData: 900 kB\nRssAnon: 500 kB\nRssFile: 1400 kB\nRssShmem: 100 kB\n")
  assert.equal(m.currentResidentBytes, 2000 * 1024)
  assert.equal(m.peakResidentBytes, 3000 * 1024)
  assert.equal(m.residentAnonymousBytes, 500 * 1024)
  assert.equal(m.residentFileBytes, 1400 * 1024)
  assert.equal(m.heapBytes, null)
  assert.equal(m.privateCommittedBytes, null)
  assert.throws(() => parseLinuxMemory("VmRSS: 20 MB\nVmHWM: 30 MB"), /unavailable/)
})

test("owned file tree reports total logical size separately from allocated bytes", () => {
  const dir = mkdtempSync(join(tmpdir(), "ox-ooc-disk-"))
  try {
    mkdirSync(join(dir, "nested"))
    writeFileSync(join(dir, "a"), "12345")
    writeFileSync(join(dir, "nested", "b"), "123")
    const d = diskUsage(dir)
    assert.equal(d.files, 2)
    assert.equal(d.logicalBytes, 8)
    assert(d.allocatedBytes >= 0)
    assert.match(d.allocatedMetric, process.platform === "win32" ? /fallback/ : /blocks/)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test("OS sample for this test process is a measurement with an explicit heap unknown", () => {
  const m = processMemory(process.pid)
  assert(m.currentResidentBytes > 0)
  assert(m.peakResidentBytes > 0)
  assert.equal(m.heapBytes, null)
  if (process.platform === "win32") assert(m.privateCommittedBytes > 0)
})

test("Linux anonymous files consume disk even when a directory walk sees no files", { skip: process.platform !== "linux" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "ox-ooc-anon-"))
  const path = join(dir, "unlinked")
  const fd = openSync(path, "w+")
  try {
    writeFileSync(fd, "1234")
    unlinkSync(path)
    assert.equal(diskUsage(dir).logicalBytes, 0)
    const d = anonymousSpillDisk(process.pid, dir)
    assert.equal(d.files, 1)
    assert.equal(d.logicalBytes, 4)
    assert(d.allocatedBytes >= 4)
  } finally { closeSync(fd); rmSync(dir, { recursive: true, force: true }) }
})
