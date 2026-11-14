import assert from "node:assert/strict"
import { test } from "node:test"
import { mkdtempSync, mkdirSync, writeFileSync, realpathSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { basename, dirname, join, resolve } from "node:path"
import { spawnSync } from "node:child_process"
import { REQUIRED_JOBS, checkRelease, releaseNotes, releaseBodyOutput, versions } from "../../tools/release-preflight.mjs"

const repositoryRoot = resolve(import.meta.dirname, "../..")
const sha = "a".repeat(40)
const context = { refType: "tag", tag: "v0.6.0", sha, repository: "ilyaux/OxRefine" }
const packageVersions = Object.fromEntries(Object.keys(versions(repositoryRoot)).map((file) => [file, "0.6.0"]))
const run = { id: 7, head_sha: sha, event: "push", head_branch: "main", status: "completed", conclusion: "success", html_url: "https://github.com/ilyaux/OxRefine/actions/runs/7" }
const jobs = REQUIRED_JOBS.map((name) => ({ name, conclusion: "success" }))
const api = ({ release = null, runs = [run], required = jobs } = {}) => async (path) =>
  path.includes("/releases/tags/") ? release : path.includes("/jobs?") ? { jobs: required } : { workflow_runs: runs }

test("every version source is read, including both lock files", () => {
  const actual = versions(repositoryRoot)
  assert.equal(Object.keys(actual).length, 10)
  assert.ok(Object.values(actual).every((v) => /^\d+\.\d+\.\d+$/.test(v)))
})

test("release notes must exist, contain a body and match the requested version", (t) => {
  const temporaryRoot = realpathSync(tmpdir())
  const root = mkdtempSync(join(temporaryRoot, "ox-release-notes-"))
  t.after(() => {
    assert.equal(dirname(root), temporaryRoot)
    assert.ok(basename(root).startsWith("ox-release-notes-"))
    rmSync(root, { recursive: true, force: true })
  })
  mkdirSync(join(root, "docs/releases"), { recursive: true })
  const path = join(root, "docs/releases/v0.6.0.md")
  assert.throws(() => releaseNotes("v0.6.0", root), /Missing release notes/)
  for (const body of ["", " \n\t", "# OxRefine 0.6.0\n \t"]) {
    writeFileSync(path, body)
    assert.throws(() => releaseNotes("v0.6.0", root), /empty|version must match/)
  }
  writeFileSync(path, "# OxRefine 0.5.3\nOld notes")
  assert.throws(() => releaseNotes("v0.6.0", root), /version must match v0.6.0/)
  writeFileSync(path, "# OxRefine 0.6.0\r\n\r\nReviewed notes.\r\n")
  assert.equal(releaseNotes("v0.6.0", root), "# OxRefine 0.6.0\n\nReviewed notes.")
  assert.throws(() => releaseNotes("v0.6.1", root), /Missing release notes/)
  assert.throws(() => releaseNotes("../v0.6.0", root), /stable/)
  assert.match(releaseNotes(context.tag, repositoryRoot), /^# OxRefine 0\.6\.0\n/)
  const failed = spawnSync(process.execPath, [join(import.meta.dirname, "../../tools/release-preflight.mjs")], {
    cwd: root, encoding: "utf8", env: { GITHUB_REF_NAME: "v0.6.1", GITHUB_OUTPUT: join(root, "output") },
  })
  assert.equal(failed.status, 1)
  assert.match(failed.stderr, /Missing release notes: docs\/releases\/v0.6.1.md/)
})

test("multiline release output preserves Markdown without interpreting command-like lines", () => {
  const body = "# OxRefine 0.6.0\n\n`code` and ${{ text }}\nEOF\nrelease_body=other\n::warning::literal"
  const output = releaseBodyOutput(body)
  const lines = output.split("\n")
  const delimiter = lines[0].slice("release_body<<".length)
  assert.match(lines[0], /^release_body<<release-notes-[a-f0-9-]+$/)
  assert.equal(lines.at(-2), delimiter)
  assert.equal(lines.slice(1, -2).join("\n"), body)
  assert.equal(body.includes(delimiter), false)
})

test("branches, malformed tags and mismatched versions fail before API requests", async () => {
  const untouched = () => assert.fail("invalid release must not call GitHub")
  for (const tag of ["main", "v0.6", "v00.6.0", "v0.6.0-rc.1", "v0.6.0\n"]) {
    await assert.rejects(checkRelease({ ...context, tag }, packageVersions, untouched), /stable/)
  }
  await assert.rejects(checkRelease({ ...context, refType: "branch" }, packageVersions, untouched), /stable/)
  await assert.rejects(checkRelease(context, { ...packageVersions, "Cargo.lock ox-core": "0.5.3" }, untouched), /Cargo.lock ox-core=0.5.3/)
})

test("published releases cannot be overwritten; draft rebuilds remain possible", async () => {
  await assert.rejects(checkRelease(context, packageVersions, api({ release: { draft: false } })), /already published/)
  assert.equal((await checkRelease(context, packageVersions, api({ release: { draft: true } }))).version, "0.6.0")
})

test("green CI must be from main for this exact SHA and include every OS, UI and benchmark", async () => {
  const accepted = await checkRelease(context, packageVersions, api())
  assert.equal(accepted.sha, sha)
  for (const changed of [{ head_sha: "b".repeat(40) }, { event: "pull_request" }, { head_branch: "feature" }]) {
    await assert.rejects(checkRelease(context, packageVersions, api({ runs: [{ ...run, ...changed }] }), { attempts: 1 }), /wait limit/)
  }
  for (const name of REQUIRED_JOBS) {
    await assert.rejects(checkRelease(context, packageVersions, api({ required: jobs.filter((j) => j.name !== name) })), /missing successful/)
    await assert.rejects(checkRelease(context, packageVersions, api({ required: jobs.map((j) => j.name === name ? { ...j, conclusion: "skipped" } : j) })), /missing successful/)
  }
})

test("the newest run cannot hide behind an older green run", async () => {
  await assert.rejects(checkRelease(context, packageVersions, api({ runs: [run, { ...run, id: 8, conclusion: "failure" }] })), /did not succeed/)
})

test("an active run is waited for, and missing CI has a finite timeout", async () => {
  let reads = 0
  let pauses = 0
  const get = async (path) => {
    if (path.includes("/releases/tags/")) return null
    if (path.includes("/jobs?")) return { jobs }
    return { workflow_runs: [{ ...run, status: ++reads === 1 ? "in_progress" : "completed" }] }
  }
  await checkRelease(context, packageVersions, get, { attempts: 2, pause: async () => { pauses++ } })
  assert.equal(pauses, 1)
  await assert.rejects(checkRelease(context, packageVersions, api({ runs: [] }), { attempts: 2, pause: async () => { pauses++ } }), /wait limit/)
  assert.equal(pauses, 2)
})
