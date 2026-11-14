// Pin guesses to the real OpenRefine 3.10.1 wizard's fixed-width parser initialization.
// Run: node tests/import/fixed-width-gen.mjs
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { basename, dirname, join } from "node:path"
import { startOpenRefine } from "../../tools/openrefine.mjs"

const cases = [
  { name: "existing fixed.txt", text: readFileSync(new URL("fixed.txt", import.meta.url), "utf8") },
  { name: "aligned fields", text: "AAA BBB CCC\nDDD EEE FFF\nGGG HHH III\n" },
  { name: "consecutive separator spaces", text: "aa   bb  cc\ndd   ee  ff\ngg   hh  ii\n" },
  { name: "two lines cannot establish widths", text: "aa bb\ncc dd\n" },
  { name: "blank lines ignored", text: "\naa bb\n\ncc dd\nee ff\n" },
  { name: "shorter first line caps positions", text: "a b\nc d extra\ne f extra\n" },
  { name: "short row disqualifies later spaces", text: "aa bb cc\ndd\nee ff gg\n" },
  { name: "tabs are not separators", text: "aa\tbb\ncc\tdd\nee\tff\n" },
  { name: "CR and CRLF breaks", text: "aa bb\rcc dd\r\nee ff\r" },
  { name: "BMP Unicode character positions", text: "Жя éé  京\nЮё öö  市\nДя ää  村\n" },
  { name: "astral characters before padding", text: "😀  bb  cc\n🦀  dd  ee\n🦊  ff  gg\n", knownDifference: "OpenRefine counts UTF-16 units; OxRefine widths count Unicode scalar characters.", scalarWidths: [2, 4, 1] },
  { name: "spaces are nonempty lines", text: "aa bb\n     \ncc dd\n" },
  { name: "first 100 nonempty lines", text: "aa bb\n".repeat(100) + "xxxxx\n" },
]
const folder = mkdtempSync(join(tmpdir(), "oxrefine-fixed-guess-"))
const or = await startOpenRefine({ workspace: join(folder, "workspace"), port: 3337 })
try {
  async function post(subCommand, jobID, fields = {}, multipart = false) {
    const query = new URLSearchParams({ controller: "core/default-importing-controller", subCommand, jobID: String(jobID), csrf_token: (await or.get("/command/core/get-csrf-token")).token })
    const form = multipart ? new FormData() : new URLSearchParams()
    for (const [k, v] of Object.entries(fields)) form.append(k, typeof v === "string" || v instanceof Blob ? v : JSON.stringify(v))
    const response = await fetch(`${or.url}/command/core/importing-controller?${query}`, { method: "POST", body: form })
    if (!response.ok) throw new Error(`${subCommand}: ${response.status} ${(await response.text()).slice(0, 200)}`)
    const body = await response.text()
    return body ? JSON.parse(body) : {}
  }
  const results = []
  for (const c of cases) {
    const { jobID } = await or.command("create-importing-job", {})
    await post("load-raw-data", jobID, { upload: new File([c.text], "fixed.txt", { type: "text/plain" }) }, true)
    for (let attempts = 0; ; attempts++) {
      const cfg = (await or.command("get-importing-job-status", { jobID })).job?.config
      if (cfg?.state === "error" || attempts > 300) throw new Error(`loading ${c.name}: ${JSON.stringify(cfg)}`)
      if (cfg?.state !== "loading-raw-data") break
      await new Promise((r) => setTimeout(r, 100))
    }
    const { options } = await post("initialize-parser-ui", jobID, { format: "text/line-based/fixed-width" })
    const columnWidths = options.columnWidths
    console.log(`${c.name}: ${JSON.stringify(columnWidths)}`)
    results.push({ ...c, columnWidths })
  }
  writeFileSync(new URL("fixed-width-guesses.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", method: "FixedWidthImporter.guessColumnWidths via initialize-parser-ui", source: "https://github.com/OpenRefine/OpenRefine/blob/3.10.1/main/src/com/google/refine/importers/FixedWidthImporter.java", cases: results }, null, 1) + "\n")
} finally {
  or.stop()
  if (dirname(folder) !== tmpdir() || !basename(folder).startsWith("oxrefine-fixed-guess-")) throw new Error("unexpected temporary workspace")
  rmSync(folder, { recursive: true, force: true })
}
