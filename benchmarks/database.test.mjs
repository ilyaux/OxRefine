import test from "node:test"
import assert from "node:assert/strict"
import { expectedRow, expectedDigest, ROWS, publicProfile } from "./database.mjs"

test("100k independent SQL fixture covers integers, strings and nulls", () => {
  assert.equal(ROWS, 100000)
  assert.deepEqual(expectedRow(0), ["0", "item-0", null])
  assert.deepEqual(expectedRow(99999), ["99999", "item-99999", "999"])
  assert.equal(Array.from({ length: ROWS }, (_, i) => expectedRow(i)[2]).filter((v) => v === null).length, 1031)
  assert.equal(expectedDigest().lines, 100001)
  assert.match(expectedDigest().sha256, /^[a-f0-9]{64}$/)
})

test("private connection fields are excluded from the profile", () => {
  const profile = publicProfile({ driver: "postgres", host: "localhost", password: "fixture-secret", schema: "private", other: "discard" })
  assert.equal(profile.driver, "postgres")
  assert.equal(profile.password, undefined)
  assert.equal(profile.schema, undefined)
  assert(!JSON.stringify(profile).includes("fixture-secret"))
})
