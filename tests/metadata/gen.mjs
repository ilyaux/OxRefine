// Regenerate tests/metadata/ from a real OpenRefine 3.10.1:  node tests/metadata/gen.mjs
// A project with its metadata set through OpenRefine's own commands, exported as project.openrefine.tar.gz;
// fixtures.json holds what get-project-metadata then reports. OxRefine must read the same metadata from
// the archive (crates/ox-core/tests/orproject_conformance.rs), and OpenRefine the same from OxRefine's
// archive (check.mjs).
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"

// what OpenRefine's metadata dialog edits; license, homepage and image answer "ok" but aren't stored
export const FIELDS = { creator: "Ada Lovelace", contributors: "Bob, Cy", subject: "Birds", description: "Bird counts, 2024\nsecond line", title: "Birds 2024", version: "1.2" }
export const TAGS = "birds, 2024 , Field Work, birds"

const or = await startOpenRefine({ workspace: join(tmpdir(), "oxrefine-metadata-fixtures"), jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC"] })
try {
  const project = await or.createProject("a,b\n1,2\n", { name: "metadata" })
  for (const [name, value] of Object.entries(FIELDS)) {
    const r = await or.command("set-project-metadata", { project, name, value })
    if (r.code !== "ok") throw new Error(`${name}: ${r.message}`)
  }
  await or.command("set-project-metadata", { project, name: "name", value: "Bird survey" })
  await or.command("set-project-tags", { project, old: "", new: TAGS })
  const meta = await or.get(`/command/core/get-project-metadata?project=${project}`)
  const r = await fetch(`${or.url}/command/core/export-project?project=${project}`, { method: "POST" })
  writeFileSync(new URL("project.openrefine.tar.gz", import.meta.url), Buffer.from(await r.arrayBuffer()))
  const keep = ["name", "tags", ...Object.keys(FIELDS), "license", "homepage", "image", "customMetadata"]
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", tags: TAGS, metadata: Object.fromEntries(keep.map((k) => [k, meta[k]])) }, null, 1) + "\n")
  console.log(JSON.stringify(Object.fromEntries(keep.map((k) => [k, meta[k]]))))
} finally {
  or.stop()
}
