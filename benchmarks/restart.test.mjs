import test from "node:test"
import assert from "node:assert/strict"
import { existsSync, readFileSync, writeFileSync } from "node:fs"
import { createServer } from "node:net"
import { join } from "node:path"
import { withOx } from "./ox.mjs"

// Explicit opt-in: ordinary Node unit tests do not start a native backend.
const bin = process.env.OX_BENCH_TEST_BINARY
async function port() {
  const s = createServer()
  await new Promise((ok) => s.listen(0, "127.0.0.1", ok))
  const n = s.address().port
  await new Promise((ok) => s.close(ok))
  return n
}
function exited(pid) { assert.throws(() => process.kill(pid, 0), { code: "ESRCH" }) }

test("restart replaces the backend and reads the saved cursor instead of cached project state", { skip: !bin }, async () => {
  let directory, before, after
  await withOx(async ({ ox, restart, pid, peak, store }) => {
    directory = store
    before = pid()
    const source = join(store, "source.csv")
    writeFileSync(source, "name\nalpha\nbeta\n")
    const summary = await ox("open", { path: source })
    await ox("apply", { op: { op: "core/text-transform", columnName: "name", expression: "grel:value.toUppercase()" } })
    const historyPath = join(store, "projects", summary.id, "history.json")
    const history = JSON.parse(readFileSync(historyPath))
    history.cursor = 0
    writeFileSync(historyPath, JSON.stringify(history))
    assert.equal((await ox("summary")).cursor, 1, "the active project still has its in-memory cursor")
    const previousSample = peak()
    const stopped = await restart()
    after = pid()
    assert.notEqual(before, after)
    assert.equal(stopped.previousPid, before)
    assert.equal(stopped.pid, after)
    assert(stopped.previousPeakMB >= 0 && previousSample >= 0 && stopped.idlePeakMB >= 0)
    assert(stopped.stopMs >= 0 && stopped.startupMs > 0)
    exited(before)
    const reopened = await ox("open_project", { id: summary.id })
    assert.equal(reopened.cursor, 0, "fresh backend must read the different saved cursor")
    assert.equal(reopened.history.length, 1)
    assert.deepEqual((await ox("rows", { limit: 2 })).rows.map((r) => r[1]), [["alpha"], ["beta"]])
    await ox("goto", { index: 1 })
    assert.deepEqual((await ox("rows", { limit: 2 })).rows.map((r) => r[1]), [["ALPHA"], ["BETA"]])
  }, { bin, port: await port() })
  assert.equal(existsSync(directory), false)
  exited(after)
})

test("callback failure after restart stops the replacement backend and removes its private library", { skip: !bin }, async () => {
  let directory, before, after
  await assert.rejects(withOx(async ({ store, pid, restart }) => {
    directory = store
    before = pid()
    await restart()
    after = pid()
    throw new Error("intentional post-restart fixture failure")
  }, { bin, port: await port() }), /intentional post-restart fixture failure/)
  assert.equal(existsSync(directory), false)
  exited(before)
  exited(after)
})
