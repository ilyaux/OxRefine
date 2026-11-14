// Read-only release gate: versions and the main-branch CI run for this exact commit.
import { appendFileSync, readFileSync } from "node:fs"
import { randomUUID } from "node:crypto"
import { resolve } from "node:path"
import { fileURLToPath } from "node:url"

export const REQUIRED_JOBS = [
  "Engine (ubuntu-latest)", "Engine (windows-latest)", "Engine (macos-latest)",
  "UI typecheck and build", "Performance against the base build",
]
const STABLE_TAG = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/

export function releaseNotes(tag, root = process.cwd()) {
  if (!STABLE_TAG.test(tag ?? "")) throw new Error("Release notes require a stable vMAJOR.MINOR.PATCH tag")
  const path = `docs/releases/${tag}.md`
  let body
  try { body = readFileSync(resolve(root, path), "utf8").trim().replace(/\r\n/g, "\n") }
  catch { throw new Error(`Missing release notes: ${path}`) }
  if (!body) throw new Error(`Release notes are empty: ${path}`)
  const [heading, ...content] = body.split("\n")
  if (heading !== `# OxRefine ${tag.slice(1)}`) throw new Error(`Release notes version must match ${tag}: ${path}`)
  if (!content.join("\n").trim()) throw new Error(`Release notes are empty: ${path}`)
  return body
}

export function releaseBodyOutput(body) {
  let delimiter
  do { delimiter = `release-notes-${randomUUID()}` } while (body.includes(delimiter))
  return `release_body<<${delimiter}\n${body}\n${delimiter}\n`
}

export function versions(root = process.cwd()) {
  const read = (path) => readFileSync(resolve(root, path), "utf8")
  const json = (path) => JSON.parse(read(path))
  const tomlVersion = (text) => /^version\s*=\s*"([^"]+)"/m.exec(text)?.[1]
  const result = Object.fromEntries(["crates/ox-core/Cargo.toml", "crates/ox-cli/Cargo.toml", "ui/src-tauri/Cargo.toml"]
    .map((path) => [path, tomlVersion(read(path).split("[package]")[1]?.split(/^\[/m)[0] ?? "")]))
  for (const path of ["ui/package.json", "ui/src-tauri/tauri.conf.json"]) result[path] = json(path).version
  const lock = json("ui/package-lock.json")
  result["ui/package-lock.json"] = lock.version
  result["ui/package-lock.json packages root"] = lock.packages?.[""]?.version
  const packages = read("Cargo.lock").split("[[package]]")
  for (const name of ["ox-core", "ox-cli", "oxrefine"]) {
    const matching = packages.filter((text) => new RegExp(`^name\\s*=\\s*"${name}"$`, "m").test(text))
    result[`Cargo.lock ${name}`] = matching.length === 1 ? tomlVersion(matching[0]) : undefined
  }
  return result
}

export async function checkRelease({ refType, tag, sha, repository }, actualVersions, get, {
  attempts = 60, pause = () => new Promise((done) => setTimeout(done, 30_000)), log = () => {},
} = {}) {
  if (refType !== "tag" || !STABLE_TAG.test(tag ?? "")) {
    throw new Error("Release requires a stable vMAJOR.MINOR.PATCH tag; dispatching a branch is not supported")
  }
  const version = tag.slice(1)
  const mismatches = Object.entries(actualVersions).filter(([, value]) => value !== version)
  if (!Object.keys(actualVersions).length || mismatches.length) {
    throw new Error(`Tag ${tag} does not match package versions: ${mismatches.map(([file, value]) => `${file}=${value ?? "missing"}`).join(", ")}`)
  }
  if (!/^[a-f0-9]{40}$/i.test(sha ?? "") || !/^[\w.-]+\/[\w.-]+$/.test(repository ?? "")) throw new Error("Missing release commit or repository")
  const base = `/repos/${repository}`
  const release = await get(`${base}/releases/tags/${encodeURIComponent(tag)}`, true)
  if (release && !release.draft) throw new Error(`${tag} is already published; use a new version instead of replacing public assets`)

  for (let attempt = 0; attempt < attempts; attempt++) {
    const data = await get(`${base}/actions/workflows/ci.yml/runs?head_sha=${sha}&per_page=100`)
    const run = data.workflow_runs?.filter((r) => r.head_sha === sha && r.event === "push" && r.head_branch === "main")
      .sort((a, b) => b.id - a.id)[0]
    if (run?.status === "completed") {
      if (run.conclusion !== "success") throw new Error(`Main CI for ${sha} did not succeed: ${run.conclusion} (${run.html_url})`)
      const { jobs } = await get(`${base}/actions/runs/${run.id}/jobs?filter=latest&per_page=100`)
      const missing = REQUIRED_JOBS.filter((name) => !jobs?.some((job) => job.name === name && job.conclusion === "success"))
      if (missing.length) throw new Error(`Release CI is missing successful required jobs: ${missing.join(", ")}`)
      return { version, sha, ciRun: run.html_url }
    }
    log(`Waiting for main CI for ${sha}: ${run?.status ?? "no run yet"} (${attempt + 1}/${attempts})`)
    if (attempt + 1 < attempts) await pause()
  }
  throw new Error(`No successful main CI for ${sha} within the release wait limit; push this commit to main and let CI finish before releasing`)
}

async function main() {
  const { GITHUB_REF_TYPE: refType, GITHUB_REF_NAME: tag, GITHUB_SHA: sha, GITHUB_REPOSITORY: repository, GH_TOKEN: token } = process.env
  const body = releaseNotes(tag)
  if (!process.env.GITHUB_OUTPUT) throw new Error("Missing GITHUB_OUTPUT file for release notes")
  const get = async (path, optional = false) => {
    const response = await fetch(`https://api.github.com${path}`, {
      headers: { accept: "application/vnd.github+json", authorization: `Bearer ${token}`, "X-GitHub-Api-Version": "2026-03-10" },
      signal: AbortSignal.timeout(30_000),
    })
    if (optional && response.status === 404) return null
    if (!response.ok) throw new Error(`GitHub release check failed: HTTP ${response.status}`)
    return response.json()
  }
  const result = await checkRelease({ refType, tag, sha, repository }, versions(), get, { log: console.log })
  appendFileSync(process.env.GITHUB_OUTPUT, releaseBodyOutput(body))
  console.log(`Release ${tag}: versions match and all required CI jobs passed for ${result.sha}: ${result.ciRun}`)
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1 })
}
