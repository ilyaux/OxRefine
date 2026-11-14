import assert from "node:assert/strict"
import { mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { basename, dirname, join, resolve } from "node:path"
import { test } from "node:test"
import { packageLicenses } from "../../tools/package-licenses.mjs"

const temporaryRoot = realpathSync(tmpdir())
function cleanup(work) {
  assert.equal(dirname(work), temporaryRoot)
  assert.ok(basename(work).startsWith("ox-license-"))
  rmSync(work, { recursive: true, force: true })
}

test("CLI and desktop inventories share every listed embedded license and credit", () => {
  const root = resolve(import.meta.dirname, "../..")
  const resources = JSON.parse(readFileSync(join(root, "ui/src-tauri/tauri.conf.json"))).bundle.resources
  const destinations = Object.values(resources)
  for (const required of ["LICENSE", "NOTICE", "licenses/commons-codec/LICENSE.txt", "licenses/language-detector/LICENSE.txt",
    "licenses/language-detector/CREDITS.md", "licenses/language-detector/UPSTREAM-README.md", "licenses/marc8/README.md",
    "licenses/OpenRefine-3.10.1.txt", "licenses/parse_wiki_text-0.1.5.txt", "licenses/geist/LICENSE", "licenses/geist-mono/LICENSE"]) {
    assert.ok(destinations.includes(required), `${required} is missing from the package inventory`)
  }
  const work = mkdtempSync(join(temporaryRoot, "ox-license-package-"))
  try {
    // The fixture supplies each mapped file, including fonts normally installed by npm ci.
    const fixture = join(work, "repo")
    mkdirSync(join(fixture, "ui/src-tauri"), { recursive: true })
    for (const [source, destination] of Object.entries(resources)) {
      const file = resolve(fixture, "ui/src-tauri", source)
      mkdirSync(dirname(file), { recursive: true })
      writeFileSync(file, `License bytes for ${destination}\r\n\u00a9\n`)
    }
    const out = join(work, "package")
    assert.deepEqual(packageLicenses(out, fixture, resources), destinations)
    for (const [source, destination] of Object.entries(resources)) {
      assert.deepEqual(readFileSync(join(out, destination)), readFileSync(resolve(fixture, "ui/src-tauri", source)))
    }
  } finally { cleanup(work) }
})

test("an invalid inventory cannot write outside the package", () => {
  const work = mkdtempSync(join(temporaryRoot, "ox-license-bounds-"))
  try {
    mkdirSync(join(work, "ui/src-tauri"), { recursive: true })
    writeFileSync(join(work, "LICENSE"), "license")
    writeFileSync(join(work, "NOTICE"), "notice")
    writeFileSync(join(work, "EXTRA"), "extra")
    const resources = { "../../LICENSE": "LICENSE", "../../NOTICE": "NOTICE", "../../EXTRA": "licenses/../../outside" }
    assert.throws(() => packageLicenses(join(work, "package"), work, resources), /leaves/)
    assert.throws(() => readFileSync(join(work, "outside")), /ENOENT/)
  } finally { cleanup(work) }
})

test("normalized resource aliases cannot overwrite another license", () => {
  const work = mkdtempSync(join(temporaryRoot, "ox-license-collision-"))
  try {
    mkdirSync(join(work, "ui/src-tauri"), { recursive: true })
    for (const name of ["LICENSE", "NOTICE", "A", "B"]) writeFileSync(join(work, name), name)
    const resources = { "../../LICENSE": "LICENSE", "../../NOTICE": "NOTICE", "../../A": "licenses/a/../same", "../../B": "licenses/SAME" }
    assert.throws(() => packageLicenses(join(work, "package"), work, resources), /Duplicate license/)
  } finally { cleanup(work) }
})
