// node roadmap/build.mjs  ->  ROADMAP.md from roadmap/tasks.json (the single source of the plan)
import { readFileSync, writeFileSync } from "node:fs"
const { phases, tasks } = JSON.parse(readFileSync(new URL("tasks.json", import.meta.url), "utf8"))
const mark = { done: "[x]", doing: "[~]", todo: "[ ]", backlog: "[ ]" }
const count = (f) => tasks.filter(f).length
const out = [
  "# OxRefine roadmap: full OpenRefine parity in Rust",
  "",
  `Generated from \`roadmap/tasks.json\` by \`node roadmap/build.mjs\`. ${count((t) => t.status === "done")} of ${tasks.length} tasks done.`,
  "",
  "Milestones: **v0.2** is a daily driver for cleaning CSV/Excel/JSON (persistence, full GREL core, all common operations, parity tests). **v0.5** covers the remaining formats, clustering methods and exporters plus installers and launch. **v1.0** adds reconciliation and full UI parity. **later** lists what only a minority of OpenRefine users need.",
  "",
  "| Milestone | Tasks | Done |",
  "|---|---|---|",
  ...["v0.1", "v0.2", "v0.5", "v1.0", "later"].map((m) => `| ${m} | ${count((t) => t.milestone === m)} | ${count((t) => t.milestone === m && t.status === "done")} |`),
  "",
]
for (const p of phases) {
  const ts = tasks.filter((t) => t.phase === p.id)
  out.push(`## ${p.name}`, "", p.goal, "")
  for (const t of ts) out.push(`- ${mark[t.status]} **${t.title}** · ${t.milestone} · ${t.priority} · ${t.size}. ${t.detail} _OpenRefine: ${t.ref}_`)
  out.push("")
}
writeFileSync(new URL("../ROADMAP.md", import.meta.url), out.join("\n"))
console.log(`ROADMAP.md: ${tasks.length} tasks in ${phases.length} phases`)
