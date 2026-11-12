// Regenerate tests/cluster/ppm-fixtures.json from OpenRefine 3.10.1's own PPM code:  node tests/cluster/ppm-gen.mjs
// Compiles ppm/PpmRef.java against the arithcode and vicino jars OpenRefine ships and runs it on OpenRefine's
// Java: for each string, its compressed length (ArithCodeOutputStream over PPMModel(8)) and vicino's PPM
// distance to the next string. Needs a JDK (javac) on the PATH or in JAVA_HOME.
import { execFileSync } from "node:child_process"
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { OR_HOME } from "../../tools/openrefine.mjs"

const lib = join(OR_HOME, "webapp/WEB-INF/lib")
const cp = [join(lib, "arithcode-1.2.jar"), join(lib, "vicino-1.2.1.jar")].join(process.platform === "win32" ? ";" : ":")
const javac = process.env.JAVA_HOME ? join(process.env.JAVA_HOME, "bin", "javac") : "javac"
const java = join(OR_HOME, "server/target/jre/bin", process.platform === "win32" ? "java.exe" : "java")

// the clustering corpus, then strings that reach the model's corners: a buffer that wraps around (over
// 288 bytes), counts past 8,192 (rescaling), repeats, UTF-8 beyond ASCII, one character, nothing
const values = JSON.parse(readFileSync(new URL("fixtures.json", import.meta.url), "utf8")).values
let seed = 7
const rand = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648)
const word = (n) => Array.from({ length: n }, () => "abcdefghij klmnop"[Math.floor(rand() * 17)]).join("")
const lines = [
  ...new Set(values),
  "",
  "a",
  "aa",
  "abababababababababab",
  "Zürich ZÜRICH zurich",
  "São Paulo · Sao Paulo",
  "東京都 東京 とうきょう",
  word(300),
  word(700),
  word(2000),
  "a".repeat(20000),
  "ab".repeat(12000),
  "the quick brown fox jumps over the lazy dog ".repeat(40),
  ...Array.from({ length: 40 }, (_, i) => word(5 + (i % 30))),
]

const dir = mkdtempSync(join(tmpdir(), "oxrefine-ppm-"))
execFileSync(javac, ["--release", "11", "-cp", cp, "-d", dir, new URL("ppm/PpmRef.java", import.meta.url).pathname.replace(/^\/(\w:)/, "$1")], { stdio: "inherit" })
writeFileSync(join(dir, "lines.txt"), lines.join("\n") + "\n")
const out = execFileSync(java, ["-Dfile.encoding=UTF-8", "-cp", [dir, cp].join(process.platform === "win32" ? ";" : ":"), "PpmRef", join(dir, "lines.txt")], { maxBuffer: 1 << 26 }).toString().trim().split(/\r?\n/)
const cases = lines.map((s, i) => {
  const [len, dist] = out[i].split("\t")
  return { s, len: Number(len), distanceToNext: Number(dist) }
})
writeFileSync(new URL("ppm-fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", jars: ["arithcode-1.2", "vicino-1.2.1"], cases }, null, 1) + "\n")
console.log(`${cases.length} strings -> tests/cluster/ppm-fixtures.json`)
