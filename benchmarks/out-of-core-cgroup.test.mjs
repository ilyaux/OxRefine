import test from "node:test"
import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { mkdtempSync, existsSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

test("cgroup coordinator refuses unapproved large/high workloads before creating a directory", () => {
  const dir = mkdtempSync(join(tmpdir(), "ox-ooc-coordinator-"))
  const output = join(dir, "must-not-exist")
  try {
    for (const [args, message] of [
      [["--rows", "50000001"], /--allow-large/],
      [["--rows", "50000001", "--cardinality", "high", "--allow-large"], /high-cardinality/],
      [["--rows", "not-an-integer"], /rows must be/],
    ]) {
      const r = spawnSync(process.execPath, [join(import.meta.dirname, "out-of-core-cgroup.mjs"), "--directory", output, ...args], { encoding: "utf8" })
      assert.equal(r.status, 1)
      assert.match(r.stderr, message)
      assert.equal(existsSync(output), false)
    }
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
