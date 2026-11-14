// Actual OpenRefine 3.10.1 tables for JSON path/JSON Lines regressions, separate from the legacy badge.
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs"
import { basename, dirname, join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { JSON_DEFAULTS } from "./cases.mjs"
import { createHash } from "node:crypto"
import { OR_HOME } from "../../tools/openrefine.mjs"

const release = {
  url: "https://github.com/OpenRefine/OpenRefine/releases/download/3.10.1/openrefine-win-with-java-3.10.1.zip",
  sha256: "6155b20a8103244a26a8f21d41698ee52251538a838e3d5f2d5af507c8b6f3c8",
  artifacts: {
    "webapp/WEB-INF/lib/openrefine-main.jar": "b1bee728e85b4aa44967427707ecdb501a6b4aa3d87608ba7b29115c858f0957",
    "webapp/WEB-INF/lib/grel-3.10.1.jar": "7158cd697ab4f6b43e21af7484a9eba1a8ac355759b3267d7f6ec9018f39e8dd",
    "webapp/WEB-INF/lib/core-3.10.1.jar": "a135638474451a220705560624e5c6a0720840edc9d7150b29132a30a3e3ac38",
    "server/target/lib/openrefine-3.10.1-server.jar": "5906b7db2ad82332cb9295490759b189adf5ef79939cb14b02878aff2903aece",
  },
}
for (const [path, expected] of Object.entries(release.artifacts)) {
  if (createHash("sha256").update(readFileSync(join(OR_HOME, path))).digest("hex") !== expected) throw new Error(`Unverified reference artifact: ${path}`)
}

const nested = readFileSync(new URL("nested.json", import.meta.url), "utf8")
const cases = [
  { name: "nested records", text: nested, recordPath: ["_", "data", "items", "_"] },
  { name: "root object with nested fields", text: '{"id":1,"nested":{"city":"Zürich"},"tags":["a","b"]}', recordPath: ["_"] },
  { name: "root array records", text: '[{"id":1,"n":{"v":"a"}},{"id":2,"n":{"v":"b"}}]', recordPath: ["_", "_"] },
  { name: "array container as one record", text: nested, recordPath: ["_", "data", "items"] },
  { name: "empty array", text: '[]', recordPath: ["_", "_"] },
  { name: "null root", text: 'null', recordPath: ["_"] },
  { name: "empty object", text: '{}', recordPath: ["_"] },
  { name: "wrong path", text: nested, recordPath: ["_", "missing", "_"] },
  { name: "JSON Lines records", text: '{"id":1,"n":{"x":"a"}}\n{"id":2,"n":{"x":"b"}}\n', lines: true, recordPath: ["_"], legacyRecordPath: ["_", "_"] },
  { name: "JSON Lines nested arrays", text: '{"items":[{"id":1},{"id":2}]}\n{"items":[{"id":3}]}\n', lines: true, recordPath: ["_", "items", "_"], legacyRecordPath: ["_", "_", "items", "_"] },
  { name: "JSON Lines literal underscore key", text: '{"_":[{"id":1},{"id":2}]}\n{"_":[{"id":3}]}\n', lines: true, recordPath: ["_", "_", "_"], legacyRecordPath: ["_", "_", "_", "_"] },
  { name: "JSON Lines root arrays", text: '[{"id":1},{"id":2}]\n[{"id":3}]\n', lines: true, recordPath: ["_", "_"], legacyRecordPath: ["_", "_", "_"] },
  { name: "JSON Lines limit", text: '{"id":1}\n{"id":2}\n', lines: true, limit: 1, recordPath: ["_"], legacyRecordPath: ["_", "_"], knownDifference: "OpenRefine resets its row limit for each JSON root; OxRefine keeps the pre-existing global JSONL record limit.", oxRows: 1 },
  { name: "JSON Lines limit before malformed tail", text: '{"id":1}\n'.repeat(100) + 'INVALID\n', lines: true, limit: 1, recordPath: ["_"], legacyRecordPath: ["_", "_"], knownDifference: "OxRefine stops its JSONL reader at the requested record limit; this OpenRefine release rejects the source during import setup." },
  { name: "punctuation in path keys", text: '{"data.with/slash":{"rows,here":[{"field":2}]}}', recordPath: ["_", "data.with/slash", "rows,here", "_"] },
  { name: "colliding flattened field names", text: '{"a - b":"literal","a":{"b":"nested"}}', recordPath: ["_"] },
  { name: "JSON Lines colliding flattened field names", text: '{"a - b":"literal","a":{"b":"nested"}}\n{"a - b":"second","a":{"b":"deep"}}\n', lines: true, recordPath: ["_"], legacyRecordPath: ["_", "_"] },
  { name: "array record with nested subrows", text: '[{"id":1,"items":[{"x":"a"},{"x":"b"}]},{"id":2}]', recordPath: ["_"] },
  { name: "JSON Lines canonical array record", text: '[{"id":1,"items":[{"x":"a"},{"x":"b"}]},{"id":2}]\n', lines: true, recordPath: ["_"] },
]
const folder = mkdtempSync(join(tmpdir(), "oxrefine-json-paths-"))
const or = await startOpenRefine({ workspace: join(folder, "workspace"), port: 3337 })
try {
  const reportedVersion = await or.get("/command/core/get-version")
  const out = []
  for (const c of cases) {
    const options = { ...JSON_DEFAULTS, recordPath: c.recordPath, ...(c.limit ? { limit: c.limit } : {}) }
    const res = await or.command("create-project-from-upload", {
      "project-file": new File([c.text], c.lines ? "records.jsonl" : "records.json"),
      "project-name": "path fixture", format: "text/json", options,
    }, { json: false })
    const project = new URL(res.headers.get("location") ?? "", or.url).searchParams.get("project")
    if (!project) throw new Error(`${c.name}: ${res.status} ${(await res.text()).slice(0, 500)}`)
    const models = await or.get(`/command/core/get-models?project=${project}`)
    if (!models.columnModel) {
      if (!c.knownDifference) throw new Error(`${c.name}: ${JSON.stringify(models)}`)
      console.log(`${c.name}: reference rejects source: ${JSON.stringify(models).slice(0, 250)}`)
      out.push({ ...c, referenceError: JSON.parse(JSON.stringify(models).replaceAll(project, "<project>")) })
      continue
    }
    const columns = models.columnModel.columns
    const { rows } = await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000`)
    out.push({ ...c, columns: columns.map((x) => x.name), rows: rows.map((r) => columns.map((x) => r.cells[x.cellIndex] ?? null)) })
    console.log(`${c.name}: ${columns.length} cols, ${rows.length} rows`)
    await or.command("delete-project", { project })
  }
  writeFileSync(new URL("json-paths-fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", reportedVersion, release, source: "https://github.com/OpenRefine/OpenRefine/blob/3.10.1/main/src/com/google/refine/importers/JsonImporter.java", cases: out }, null, 1) + "\n")
} finally {
  or.stop()
  if (dirname(folder) !== tmpdir() || !basename(folder).startsWith("oxrefine-json-paths-")) throw new Error("unexpected temporary workspace")
  rmSync(folder, { recursive: true, force: true })
}
