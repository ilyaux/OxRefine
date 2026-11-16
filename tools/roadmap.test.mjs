import test from "node:test"
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { renderRoadmap } from "../roadmap/build.mjs"

test("roadmap separates owner completion, review, active and planned tasks", () => {
  const plan = {
    currentRelease: { tag: "v0.6.0", url: "https://example.com/releases/v0.6.0", published: "2026-11-14" },
    phases: [{ id: "phase", name: "Phase", goal: "Goal" }],
    tasks: ["done", "review", "doing", "todo", "backlog"].map((status) => ({
      id: status, status, phase: "phase", milestone: "v1.0", title: status, detail: "Detail", ref: "Reference", priority: "P1", size: "S",
    })),
  }
  const before = JSON.stringify(plan), markdown = renderRoadmap(plan)
  assert(markdown.includes("Current release: [**v0.6.0**](https://example.com/releases/v0.6.0), published 2026-11-14."))
  assert(markdown.includes("1 of 5 tasks done."))
  assert(markdown.includes("| v1.0 | 5 | 1 | 1 | 1 | 2 |"))
  assert(markdown.includes("| **Total** | **5** | **1** | **1** | **1** | **2** |"))
  assert(markdown.includes("**review** (review)"))
  assert(markdown.includes("Shipped features may still be in review."))
  assert.equal(JSON.stringify(plan), before, "Rendering cannot change workflow decisions")
})

test("published roadmap exactly matches its checked-in task specification", () => {
  const plan = JSON.parse(readFileSync(new URL("../roadmap/tasks.json", import.meta.url)))
  const markdown = readFileSync(new URL("../ROADMAP.md", import.meta.url), "utf8").replaceAll("\r\n", "\n")
  assert.equal(markdown, renderRoadmap(plan))
  assert.equal(new Set(plan.tasks.map((t) => t.id)).size, plan.tasks.length)
  assert(!markdown.includes("undefined"))
})
