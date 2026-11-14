// Real GREL outputs from the verified OpenRefine release; its get-version reports SNAPSHOT upstream.
import { createHash } from "node:crypto"
import { execFileSync } from "node:child_process"
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { basename, dirname, delimiter, join } from "node:path"
import { fileURLToPath } from "node:url"
import { OR_HOME, startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES } from "./corpus.mjs"

const artifacts = {
  "webapp/WEB-INF/lib/openrefine-main.jar": "b1bee728e85b4aa44967427707ecdb501a6b4aa3d87608ba7b29115c858f0957",
  "webapp/WEB-INF/lib/grel-3.10.1.jar": "7158cd697ab4f6b43e21af7484a9eba1a8ac355759b3267d7f6ec9018f39e8dd",
  "webapp/WEB-INF/lib/language-detector-0.6.jar": "f53ecc3d71da9ebc82edd10fb35638d32e8b9d849273dd717a021eca02f2278d",
}
for (const [path, expected] of Object.entries(artifacts)) {
  if (createHash("sha256").update(readFileSync(join(OR_HOME, path))).digest("hex") !== expected) throw new Error(`Unverified reference artifact: ${path}`)
}
const folder = mkdtempSync(join(tmpdir(), "oxrefine-language-fixtures-"))
let or
try {
  writeFileSync(join(folder, "corpus.json"), JSON.stringify(CASES))
  const classpath = [join(OR_HOME, "webapp/WEB-INF/lib/*"), join(OR_HOME, "server/target/lib/*")].join(delimiter)
  execFileSync("javac", ["--release", "11", "-cp", classpath, "-d", folder, fileURLToPath(new URL("ReferenceText.java", import.meta.url))])
  execFileSync(join(OR_HOME, "server/target/jre/bin/java.exe"), ["-Xmx512M", "-cp", folder + delimiter + classpath, "ReferenceText", join(folder, "corpus.json"), join(folder, "traces.json")])
  const traces = JSON.parse(readFileSync(join(folder, "traces.json"), "utf8"))
  or = await startOpenRefine({ workspace: join(folder, "workspace"), port: 3337, mem: "1400M",
    jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"] })
  const reportedVersion = await or.get("/command/core/get-version")
  const project = await or.createProject("v\nx\n", { name: "language" })
  const out = []
  for (const c of CASES) {
    const expression = c.expression ?? `${JSON.stringify(c.text)}.detectLanguage()`
    const result = await or.command("preview-expression", { project, cellIndex: "0", rowIndices: "[0]", expression: `grel:${expression}`, repeat: "false", repeatCount: "10" })
    if (result.code !== "ok") throw new Error(`${c.name}: ${JSON.stringify(result)}`)
    out.push({ ...c, result: result.results[0], ...(c.trace ? { normalizedUnits: traces[c.name] } : {}) })
    console.log(`${out.length}/${CASES.length} ${c.name.slice(0, 60)} -> ${JSON.stringify(result.results[0])}`)
  }
  await or.command("delete-project", { project })
  const fixture = { openrefine: "3.10.1", reportedVersion,
    release: { url: "https://github.com/OpenRefine/OpenRefine/releases/download/3.10.1/openrefine-win-with-java-3.10.1.zip", sha256: "6155b20a8103244a26a8f21d41698ee52251538a838e3d5f2d5af507c8b6f3c8", artifacts },
    library: "com.optimaize.languagedetector:language-detector:0.6", source: "https://github.com/optimaize/language-detector/tree/language-detector-0.6",
    corpus: "Authored test paragraphs and edge cases; language tags are not expected detector answers.", cases: out }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify(fixture, null, 1) + "\n")
} finally {
  or?.stop()
  if (dirname(folder) !== tmpdir() || !basename(folder).startsWith("oxrefine-language-fixtures-")) throw new Error("unexpected temporary workspace")
  rmSync(folder, { recursive: true, force: true })
}
