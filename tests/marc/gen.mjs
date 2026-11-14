// Real MARC4J conversion + actual OpenRefine wizard imports. Never infer expected table data.
import { createHash } from "node:crypto"
import { execFileSync } from "node:child_process"
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs"
import { basename, dirname, delimiter, join } from "node:path"
import { tmpdir } from "node:os"
import { fileURLToPath } from "node:url"
import { OR_HOME, startOpenRefine } from "../../tools/openrefine.mjs"

const artifacts = {
  "webapp/WEB-INF/lib/openrefine-main.jar": "b1bee728e85b4aa44967427707ecdb501a6b4aa3d87608ba7b29115c858f0957",
  "webapp/WEB-INF/lib/marc4j-2.9.6.jar": "7a770fd4f35adc77ef09c1dbafd12de73895d80627aa056742bc74d7cc5d6995",
}
for (const [path, hash] of Object.entries(artifacts)) if (createHash("sha256").update(readFileSync(join(OR_HOME, path))).digest("hex") !== hash) throw new Error(`Unverified reference ${path}`)
const cases = JSON.parse(readFileSync(new URL("cases.json", import.meta.url), "utf8"))
const folder = mkdtempSync(join(tmpdir(), "oxrefine-marc-"))
let or
try {
  const classpath = [join(OR_HOME, "webapp/WEB-INF/lib/*"), join(OR_HOME, "server/target/lib/*")].join(delimiter)
  execFileSync("javac", ["--release", "11", "-cp", classpath, "-d", folder, fileURLToPath(new URL("Reference.java", import.meta.url))])
  or = await startOpenRefine({ workspace: join(folder, "workspace"), port: 3337 })
  const reportedVersion = await or.get("/command/core/get-version")
  async function post(subCommand, jobID, fields = {}, multipart = false) {
    const query = new URLSearchParams({ controller: "core/default-importing-controller", subCommand, jobID: String(jobID), csrf_token: (await or.get("/command/core/get-csrf-token")).token })
    const form = multipart ? new FormData() : new URLSearchParams()
    for (const [key, value] of Object.entries(fields)) form.append(key, typeof value === "string" || value instanceof Blob ? value : JSON.stringify(value))
    const response = await fetch(`${or.url}/command/core/importing-controller?${query}`, { method: "POST", body: form })
    const body = await response.text()
    if (!response.ok) throw new Error(`${subCommand}: ${response.status} ${body.slice(0, 200)}`)
    return body ? JSON.parse(body) : {}
  }
  const out = []
  for (const c of cases) {
    const file = fileURLToPath(new URL(c.file, import.meta.url))
    const xmlFile = join(folder, c.name + ".xml")
    let referenceXml, referenceError
    try {
      execFileSync(join(OR_HOME, "server/target/jre/bin/java.exe"), ["-cp", folder + delimiter + classpath, "Reference", file, xmlFile], { stdio: ["ignore", "pipe", "pipe"] })
      referenceXml = readFileSync(xmlFile, "utf8")
    } catch (e) {
      referenceError = String(e.stderr).split(/[\r\n]+/).find((s) => s.startsWith("Exception")) ?? "reference conversion failed"
      if (!c.reject) throw new Error(`${c.name}: ${referenceError}`)
    }
    if (c.reject) {
      out.push({ ...c, ...(referenceXml ? { referenceXml, knownDifference: c.knownDifference ?? "OxRefine rejects malformed ISO2709 structure; the permissive reference repairs this input." } : { referenceError }) })
      console.log(`${c.name}: ${referenceXml ? "reference repaired" : "reference rejected"}`)
      continue
    }
    const { jobID } = await or.command("create-importing-job", {})
    await post("load-raw-data", jobID, { upload: new File([readFileSync(file)], c.file) }, true)
    for (let attempt = 0; ; attempt++) {
      const cfg = (await or.command("get-importing-job-status", { jobID })).job?.config
      if (cfg?.state === "error" || attempt > 300) throw new Error(`${c.name}: ${JSON.stringify(cfg)}`)
      if (cfg?.state !== "loading-raw-data") break
      await new Promise((r) => setTimeout(r, 100))
    }
    const initialized = await post("initialize-parser-ui", jobID, { format: "text/marc" })
    const options = { trimStrings: false, guessCellValueTypes: false, storeEmptyStrings: true, limit: -1,
      includeFileSources: false, includeArchiveFileName: false, ...c.options,
      recordPath: (c.options?.recordPath ?? ["collection", "record"]).map((p) => `marc:${p}`), projectName: c.name }
    if (!initialized.options?.dom) throw new Error(`${c.name}: no MARCXML preview ${JSON.stringify(initialized).slice(0, 200)}`)
    await post("create-project", jobID, { format: "text/marc", options })
    let project
    for (let attempt = 0; ; attempt++) {
      const cfg = (await or.command("get-importing-job-status", { jobID })).job?.config
      if (cfg?.state === "error" || attempt > 300) throw new Error(`${c.name}: ${JSON.stringify(cfg).slice(0, 300)}`)
      if (cfg?.projectID) { project = cfg.projectID; break }
      await new Promise((r) => setTimeout(r, 100))
    }
    const columns = (await or.get(`/command/core/get-models?project=${project}`)).columnModel.columns
    const rows = (await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000`)).rows
    out.push({ ...c, options, referenceXml, columns: columns.map((x) => x.name), rows: rows.map((r) => columns.map((x) => r.cells[x.cellIndex] ?? null)) })
    console.log(`${c.name}: ${columns.length} cols, ${rows.length} rows`)
    await or.command("delete-project", { project })
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", reportedVersion,
    release: { url: "https://github.com/OpenRefine/OpenRefine/releases/download/3.10.1/openrefine-win-with-java-3.10.1.zip", sha256: "6155b20a8103244a26a8f21d41698ee52251538a838e3d5f2d5af507c8b6f3c8", artifacts },
    source: "https://github.com/OpenRefine/OpenRefine/blob/3.10.1/main/src/com/google/refine/importers/MarcImporter.java", cases: out }, null, 1) + "\n")
} finally {
  or?.stop()
  if (dirname(folder) !== tmpdir() || !basename(folder).startsWith("oxrefine-marc-")) throw new Error("unexpected temporary workspace")
  rmSync(folder, { recursive: true, force: true })
}
