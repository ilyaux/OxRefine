// OS measurements deliberately distinguish resident file mappings from private/anonymous memory.
import assert from "node:assert/strict"
import { execFileSync } from "node:child_process"
import { readFileSync, readdirSync, statSync, existsSync, readlinkSync } from "node:fs"
import { join, resolve } from "node:path"
import { totalmem, freemem, cpus, platform, release } from "node:os"

export function parseLinuxMemory(text) {
  const fields = Object.fromEntries(text.split("\n").flatMap((line) => {
    const m = /^(VmRSS|VmHWM|VmData|RssAnon|RssFile|RssShmem):\s+(\d+)\s+kB\s*$/.exec(line)
    if (!m) return []
    const bytes = Number(m[2]) * 1024
    assert(Number.isSafeInteger(bytes), `memory value outside integer range: ${m[1]}`)
    return [[m[1], bytes]]
  }))
  assert(fields.VmRSS !== undefined && fields.VmHWM !== undefined, "Linux resident metrics unavailable")
  return { currentResidentBytes: fields.VmRSS, peakResidentBytes: fields.VmHWM,
    residentAnonymousBytes: fields.RssAnon ?? null, residentFileBytes: fields.RssFile ?? null,
    residentSharedBytes: fields.RssShmem ?? null, virtualDataBytes: fields.VmData ?? null,
    privateCommittedBytes: null, heapBytes: null,
    source: "proc-status", caveat: "RSS includes file mappings; RssAnon is resident anonymous memory, VmData is virtual data, neither measures total heap allocations." }
}

export function processMemory(pid) {
  assert(Number.isSafeInteger(pid) && pid > 0, "invalid process id")
  if (process.platform === "linux") return { pid, ...parseLinuxMemory(readFileSync(`/proc/${pid}/status`, "utf8")) }
  if (process.platform === "win32") {
    const text = execFileSync("powershell", ["-NoProfile", "-NonInteractive", "-Command",
      `$p = Get-Process -Id ${pid} -ErrorAction Stop; @{ currentResidentBytes = $p.WorkingSet64; peakResidentBytes = $p.PeakWorkingSet64; privateCommittedBytes = $p.PrivateMemorySize64 } | ConvertTo-Json -Compress`], { encoding: "utf8" })
    const got = JSON.parse(text)
    for (const v of Object.values(got)) assert(Number.isSafeInteger(v) && v >= 0, "invalid Windows process metric")
    return { pid, ...got, heapBytes: null, source: "windows-process",
      caveat: "Working set includes mapped file pages; private committed memory includes heaps, stacks and other private pages, not a heap allocator counter." }
  }
  throw new Error("out-of-core memory validation currently supports Windows and Linux only")
}

/** Counts physical files without following links; allocated bytes can differ from logical size. */
export function diskUsage(path) {
  let logicalBytes = 0, allocatedBytes = 0, files = 0
  for (const e of readdirSync(path, { withFileTypes: true })) {
    assert(!e.isSymbolicLink(), `unexpected link in benchmark-owned directory: ${e.name}`)
    const p = join(path, e.name)
    if (e.isDirectory()) {
      const sub = diskUsage(p)
      logicalBytes += sub.logicalBytes; allocatedBytes += sub.allocatedBytes; files += sub.files
    } else if (e.isFile()) {
      const s = statSync(p)
      files++
      logicalBytes += s.size
      allocatedBytes += typeof s.blocks === "number" ? s.blocks * 512 : s.size
    }
  }
  return { logicalBytes, allocatedBytes, files, allocatedMetric: process.platform === "win32" ? "logical-size fallback; NTFS allocation not measured" : "stat.blocks * 512" }
}

/** Unix anonymous spill files are unlinked while open, so directory walks/du cannot see their disk. */
export function anonymousSpillDisk(pid, spillDir) {
  if (process.platform !== "linux") return { available: false, reason: "anonymous open-file allocation probe currently supports Linux" }
  const prefix = `${resolve(spillDir)}/`
  let logicalBytes = 0, allocatedBytes = 0, files = 0
  const seen = new Set()
  for (const fd of readdirSync(`/proc/${pid}/fd`)) {
    try {
      const path = `/proc/${pid}/fd/${fd}`
      const target = readlinkSync(path)
      if (!target.startsWith(prefix) || !target.endsWith(" (deleted)")) continue
      const s = statSync(path)
      const id = `${s.dev}:${s.ino}`
      if (!s.isFile() || seen.has(id)) continue
      seen.add(id)
      logicalBytes += s.size
      allocatedBytes += s.blocks * 512
      files++
    } catch (e) { if (e.code !== "ENOENT") throw e }
  }
  return { available: true, logicalBytes, allocatedBytes, files,
    scope: "open unlinked regular files in the private spill directory, deduplicated by device/inode" }
}

export function hostFacts() {
  return { platform: platform(), release: release(), cpu: cpus()[0]?.model, cpus: cpus().length,
    totalMemoryBytes: totalmem(), freeMemoryBytes: freemem(), node: process.version }
}

/** Read an already installed cgroup limit. This never changes the host or installs a constraint. */
export function cgroupMemory(pid) {
  if (process.platform !== "linux") return { verified: false, reason: "no Linux cgroup evidence on this platform" }
  try {
    assert(Number.isSafeInteger(pid) && pid > 0)
    const relative = /^0::([^\n]*)$/m.exec(readFileSync(`/proc/${pid}/cgroup`, "utf8"))?.[1]
    assert(relative, "process is not in a unified cgroup")
    const root = "/sys/fs/cgroup"
    const dir = resolve(root, `.${relative}`)
    assert(dir === root || dir.startsWith(`${root}/`), "cgroup path escaped its mount")
    const number = (name) => {
      const text = readFileSync(join(dir, name), "utf8").trim()
      if (text === "max") return null
      const n = Number(text)
      assert(Number.isSafeInteger(n) && n >= 0, `invalid cgroup ${name}`)
      return n
    }
    const limitBytes = number("memory.max")
    const currentBytes = number("memory.current")
    const peakBytes = existsSync(join(dir, "memory.peak")) ? number("memory.peak") : null
    const swapLimitBytes = number("memory.swap.max")
    const stat = Object.fromEntries(readFileSync(join(dir, "memory.stat"), "utf8").trim().split("\n").flatMap((s) => {
      const [name, value] = s.split(/\s+/)
      return ["anon", "file", "file_mapped", "file_dirty", "file_writeback", "pagetables"].includes(name) ? [[name, Number(value)]] : []
    }))
    const events = Object.fromEntries(readFileSync(join(dir, "memory.events"), "utf8").trim().split("\n").map((s) => {
      const [name, value] = s.split(/\s+/)
      return [name, Number(value)]
    }))
    return { verified: limitBytes !== null, kind: "cgroup-v2", limitBytes, currentBytes, peakBytes, swapLimitBytes, stat, events,
      scope: "whole cgroup anonymous memory and charged file cache, not allocator heap; shared pages can stay charged to their creating cgroup" }
  } catch (e) { return { verified: false, reason: e.message } }
}
