// Drive a real OpenRefine 3.10.1 (unpacked next to this repo) over its HTTP API.
// Used by the benchmark and by the conformance fixture generators.
import { spawn, execFileSync } from "node:child_process"
import { mkdirSync } from "node:fs"
import { join, resolve } from "node:path"

export const OR_HOME = process.env.OPENREFINE_HOME ?? resolve(import.meta.dirname, "../../OpenRefine/openrefine-3.10.1")
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

export function peakMB(pid) {
  const out = execFileSync("powershell", ["-NoProfile", "-Command", `(Get-Process -Id ${pid}).PeakWorkingSet64`]).toString()
  return Math.round(Number(out.trim()) / 2 ** 20)
}

export function kill(p) {
  try { execFileSync("taskkill", ["/PID", String(p.pid), "/T", "/F"], { stdio: "ignore" }) } catch {}
}

export async function waitFor(url, init, ms = 120_000) {
  const t = performance.now()
  while (performance.now() - t < ms) {
    try {
      if ((await fetch(url, init)).status < 500) return
    } catch {}
    await sleep(200)
  }
  throw new Error(`server at ${url} did not come up`)
}

/** Start a fresh OpenRefine JVM with its own workspace. Returns a small client. */
export async function startOpenRefine({ mem = "1400M", port = 3333, workspace, jvm = [] }) {
  mkdirSync(workspace, { recursive: true })
  const lib = join(OR_HOME, "server/target/lib")
  const proc = spawn(join(OR_HOME, "server/target/jre/bin/java.exe"), [
    "-cp", `${join(OR_HOME, "server/classes")};${lib}\\*`,
    ...jvm, "-Xms256M", `-Xmx${mem}`, `-Drefine.memory=${mem}`, "-Drefine.max_form_content_size=67108864",
    `-Drefine.port=${port}`, "-Drefine.interface=127.0.0.1", "-Drefine.host=127.0.0.1", `-Drefine.webapp=${join(OR_HOME, "webapp")}`,
    `-Drefine.data_dir=${workspace}`, "-Drefine.headless=true", `-Djava.library.path=${lib}/native/windows`, "com.google.refine.Refine",
  ], { cwd: OR_HOME, stdio: "ignore" })
  const url = `http://127.0.0.1:${port}`
  await waitFor(`${url}/command/core/get-csrf-token`)
  await sleep(1000)

  const token = async () => (await (await fetch(`${url}/command/core/get-csrf-token`)).json()).token
  /** POST a command. OpenRefine reads multipart only for uploads; everything else is urlencoded. */
  async function command(cmd, fields, { json = true } = {}) {
    const upload = Object.values(fields).some((v) => v instanceof Blob)
    const form = upload ? new FormData() : new URLSearchParams()
    for (const [k, v] of Object.entries(fields)) form.append(k, typeof v === "string" || v instanceof Blob ? v : JSON.stringify(v))
    const r = await fetch(`${url}/command/core/${cmd}?csrf_token=${await token()}`, { method: "POST", body: form, redirect: "manual" })
    if (r.status >= 400) throw new Error(`OpenRefine ${cmd}: HTTP ${r.status} ${(await r.text()).slice(0, 300)}`)
    return json ? r.json() : r
  }
  async function get(path) {
    return (await fetch(`${url}${path}`)).json()
  }
  /** Import CSV text or a Blob as a project; all cells stay strings unless guessTypes. */
  async function createProject(csv, { name = "project", guessTypes = false } = {}) {
    const file = csv instanceof Blob ? csv : new Blob([csv], { type: "text/csv" })
    const res = await command("create-project-from-upload", {
      "project-file": new File([file], `${name}.csv`, { type: "text/csv" }),
      "project-name": name,
      format: "text/line-based/*sv",
      options: { encoding: "UTF-8", separator: ",", ignoreLines: -1, headerLines: 1, skipDataLines: 0, limit: -1,
        storeBlankRows: true, guessCellValueTypes: guessTypes, processQuotes: true, quoteCharacter: '"',
        storeBlankCellsAsNulls: true, includeFileSources: false, includeArchiveFileName: false, trimStrings: false },
    }, { json: false })
    const project = new URL(res.headers.get("location") ?? "", url).searchParams.get("project")
    if (!project) throw new Error(`import failed: ${res.status} ${(await res.text()).slice(0, 300)}`)
    return project
  }
  async function idle(project) {
    for (;;) {
      if (!(await get(`/command/core/get-processes?project=${project}`)).processes?.length) return
      await sleep(50)
    }
  }
  return { url, pid: proc.pid, proc, command, get, createProject, idle, stop: () => kill(proc) }
}
