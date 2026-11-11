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
  /** A POST to the importing controller: multipart only when loading raw data (OpenRefine reads
   * other bodies as urlencoded). Returns {} when the reply is not JSON. */
  async function post(path, fields = {}, multipart = false) {
    const form = multipart ? new FormData() : new URLSearchParams()
    for (const [k, v] of Object.entries(fields)) form.append(k, typeof v === "string" || v instanceof Blob ? v : JSON.stringify(v))
    const r = await fetch(`${url}/command/core/${path}${path.includes("?") ? "&" : "?"}csrf_token=${await token()}`, { method: "POST", body: form })
    try {
      return JSON.parse(await r.text())
    } catch {
      return {}
    }
  }
  /**
   * Run OpenRefine's import wizard the way its UI does with no changes: load the data
   * (`{upload: File}`, `{clipboard: text}` or `{download: url}`), take the top-ranked format and its
   * parser defaults, create the project. `choose` overrides options (a choice the wizard needs from
   * the user, like a JSON record path). Returns what it guessed and the table it built, then deletes it.
   */
  async function wizard(source, choose = {}) {
    const { jobID } = await post("create-importing-job")
    const ctl = (sub, extra = "") => `importing-controller?controller=core%2Fdefault-importing-controller&jobID=${jobID}&subCommand=${sub}${extra}`
    const status = async () => (await post(`get-importing-job-status?jobID=${jobID}`)).job?.config ?? {}
    await post(ctl("load-raw-data"), source, true)
    let cfg = await status()
    while (cfg.state === "loading-raw-data") (await sleep(100), (cfg = await status()))
    if (cfg.state === "error") throw new Error(`wizard: ${JSON.stringify(cfg).slice(0, 600)}`)
    const format = cfg.rankedFormats[0]
    const { options } = await post(ctl("initialize-parser-ui", `&format=${encodeURIComponent(format)}`))
    delete options.dom
    // the wizard page turns the sheet list into the `sheets` option the Excel parser reads
    const sheets = options.sheetRecords?.filter((r) => r.selected).map(({ name, fileNameAndSheetIndex }) => ({ name, fileNameAndSheetIndex, selected: true }))
    await post(ctl("create-project"), { format, options: { ...options, ...(sheets ? { sheets } : {}), ...choose, projectName: "wizard" } })
    for (let i = 0; !(cfg = await status()).projectID; i++) {
      if (cfg.state === "error" || i > 600) throw new Error(`wizard: no project: ${JSON.stringify(cfg).slice(0, 600)}`)
      await sleep(100)
    }
    const project = cfg.projectID
    const cols = (await get(`/command/core/get-models?project=${project}`)).columnModel.columns
    const rows = (await get(`/command/core/get-rows?project=${project}&start=0&limit=10000`)).rows
    await command("delete-project", { project })
    const file = cfg.retrievalRecord.files[0]
    return { fileName: file.fileName, format, options, columns: cols.map((x) => x.name), rows: rows.map((r) => cols.map((x) => r.cells[x.cellIndex] ?? null)) }
  }
  /** Apply a list of ops: core ops through apply-operations, OxRefine's single edits (`ox/cell-edit`,
   * `ox/row-annotate`) through OpenRefine's own commands. Returns the last reply ({code: "ok"} on success). */
  async function runOps(project, ops) {
    let res = { code: "ok" }
    for (let i = 0; i < ops.length && res.code === "ok"; ) {
      const op = ops[i]
      if (op.op === "ox/cell-edit") {
        const cols = (await get(`/command/core/get-models?project=${project}`)).columnModel.columns
        const cell = cols.find((x) => x.name === op.columnName).cellIndex
        res = await command("edit-one-cell", { project, row: op.row, cell, value: op.value, type: op.type })
        i++
      } else if (op.op === "ox/row-annotate") {
        const { op: _, row, ...what } = op
        res = await command("annotate-one-row", { project, row, ...what })
        i++
      } else {
        let j = i
        while (j < ops.length && !ops[j].op.startsWith("ox/")) j++
        res = await command("apply-operations", { project, operations: ops.slice(i, j) })
        await idle(project)
        // long-running operations (fetching URLs) answer "pending" and finish in the background
        if (res.code === "pending") res = { code: "ok" }
        i = j
      }
    }
    return res
  }
  /** The table as get-rows shows it, in column order: cells are null | {v, t?} | {e}. */
  /** `recon: "brief"` shows a reconciled cell's Recon as {j: judgment, m: matched id or null}. */
  async function table(project, { recon } = {}) {
    const cols = (await get(`/command/core/get-models?project=${project}`)).columnModel.columns
    const { rows, pool } = await get(`/command/core/get-rows?project=${project}&start=0&limit=100000`)
    const cell = (c) => {
      if (!c?.r || recon !== "brief") return c ?? null
      const r = pool.recons[c.r]
      return { ...c, r: { j: r.j, m: r.m?.id ?? null } }
    }
    return { columns: cols.map((x) => x.name), rows: rows.map((r) => ({ starred: r.starred, flagged: r.flagged, cells: cols.map((x) => cell(r.cells[x.cellIndex])) })) }
  }
  return { url, pid: proc.pid, proc, command, get, createProject, idle, wizard, runOps, table, stop: () => kill(proc) }
}
