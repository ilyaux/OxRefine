// Running OxRefine through the benchmark's steps, shared by compare.mjs and check.mjs.
import { spawn, execFileSync } from "node:child_process"
import { existsSync, mkdtempSync, openSync, readFileSync, rmSync } from "node:fs"
import { join, resolve } from "node:path"
import { tmpdir } from "node:os"
import { createServer } from "node:net"
import { randomUUID } from "node:crypto"

export const HERE = import.meta.dirname
export const DATA = join(HERE, "data")
export const OX = resolve(HERE, "../target/release", process.platform === "win32" ? "ox.exe" : "ox")
export const recipe = JSON.parse(readFileSync(join(HERE, "recipe.json"), "utf8"))
export const countryFacet = {
  facets: [{ type: "list", name: "country", columnName: "country", expression: "value", omitBlank: false, omitError: false,
    selection: [], selectBlank: false, selectError: false, invert: false }],
  mode: "row-based",
}
export const noFacets = { facets: [], mode: "row-based" }
export const STEPS = ["import", "facet", "cluster", "recipe", "export"]
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const now = () => performance.now()

/** The process's peak resident memory so far, in MB. */
export function peakMB(pid) {
  if (process.platform === "win32") {
    const out = execFileSync("powershell", ["-NoProfile", "-Command", `(Get-Process -Id ${pid}).PeakWorkingSet64`]).toString()
    return Math.round(Number(out.trim()) / 2 ** 20)
  }
  if (process.platform === "linux") {
    const kb = /VmHWM:\s+(\d+)/.exec(readFileSync(`/proc/${pid}/status`, "utf8"))?.[1]
    return Math.round(Number(kb) / 1024)
  }
  // macOS has no peak for another process: the current resident size
  return Math.round(Number(execFileSync("ps", ["-o", "rss=", "-p", String(pid)]).toString().trim()) / 1024)
}

export async function waitFor(url, init, ms = 120_000, check = () => {}) {
  const t = now()
  while (now() - t < ms) {
    check()
    try {
      const r = await fetch(url, init)
      if (r.status < 500) return
    } catch {}
    await sleep(200)
  }
  throw new Error(`server at ${url} did not come up`)
}

export function kill(p) {
  if (!p.pid || p.exitCode !== null) return
  if (process.platform === "win32") {
    try { execFileSync("taskkill", ["/PID", String(p.pid), "/T", "/F"], { stdio: "ignore" }) } catch {}
  } else p.kill("SIGKILL")
}

/** The benchmark's data file of `n` rows, made once by gen.mjs. */
export function dataset(n) {
  const csv = join(DATA, `people_${n}.csv`)
  if (!existsSync(csv)) execFileSync(process.execPath, [join(HERE, "gen.mjs"), String(n)], { stdio: ["ignore", openSync(csv, "w"), "inherit"] })
  return csv
}

/** A fresh benchmark server with a private project library, removed after the callback. */
export async function withOx(run, { bin = OX, port = 3030 } = {}) {
  // Fail before spawning if another server owns the port; never benchmark its data by mistake.
  await new Promise((ok, fail) => {
    const probe = createServer()
    probe.once("error", fail)
    probe.listen({ host: "127.0.0.1", port, exclusive: true }, () => probe.close(ok))
  })
  const store = mkdtempSync(join(tmpdir(), "ox-bench-"))
  const token = randomUUID()
  const headers = { "Content-Type": "application/json", Authorization: `Bearer ${token}` }
  const p = spawn(bin, ["serve", "--port", String(port), "--data", store, "--token", token], { stdio: "ignore" })
  let spawnError
  p.on("error", (e) => { spawnError = e })
  const check = () => {
    if (spawnError) throw spawnError
    if (p.exitCode !== null) throw new Error(`benchmark server exited with ${p.exitCode}`)
  }
  const url = `http://127.0.0.1:${port}/api`
  const ox = async (cmd, args = {}) => {
    const r = await fetch(`${url}/${cmd}`, { method: "POST", headers, body: JSON.stringify(args) })
    const j = await r.json()
    if (!r.ok) throw new Error(`ox ${cmd}: ${j}`)
    return j.data
  }
  try {
    await waitFor(`${url}/summary`, { method: "POST", headers, body: "{}" }, 120_000, check)
    check()
    const denied = await fetch(`${url}/projects`, { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" })
    if (denied.status !== 403) throw new Error("benchmark endpoint does not enforce its private token")
    if ((await ox("projects")).length) throw new Error("benchmark library is not empty")
    return await run({ ox, peak: () => peakMB(p.pid), store })
  } finally {
    kill(p)
    await sleep(200)
    rmSync(store, { recursive: true, force: true })
  }
}

/** One run of `bin` over `csv`: milliseconds per step and peak resident RAM. */
export async function runOx(csv, out, options = {}) {
  return withOx(async ({ ox, peak }) => {
    const r = { idle: peak() }
    let t = now(); const s = await ox("open", { path: csv }); r.import = now() - t; r.rows = s.rows
    t = now(); const f = await ox("facets", { engine: countryFacet }); r.facet = now() - t; r.facetChoices = f[0].distinct
    t = now(); const c = await ox("cluster", { engine: noFacets, column: "city", method: "fingerprint" }); r.cluster = now() - t; r.clusters = c.length
    t = now(); await ox("apply_recipe", { ops: recipe }); r.recipe = now() - t
    t = now(); await ox("export", { engine: noFacets, path: out }); r.export = now() - t
    r.peak = peak()
    return r
  }, options)
}
