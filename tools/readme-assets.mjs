// Draws the README charts in docs/readme/ from benchmarks/results.json.
// node tools/readme-assets.mjs
// The SVGs follow the reader's light or dark theme and animate once when shown;
// without animation (or with reduced motion) they render their final state.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs"

const ROOT = new URL("../", import.meta.url)
const OUT = new URL("docs/readme/", ROOT)
mkdirSync(OUT, { recursive: true })
const res = JSON.parse(readFileSync(new URL("benchmarks/results.json", ROOT), "utf8"))

// Cases matching OpenRefine per suite: keep in step with the table in README.md.
const COMPAT = [
  ["GREL expressions", 559, 561],
  ["Operations", 83, 84],
  ["Facets", 39, 39],
  ["Import options", 43, 45],
  ["Clipboard and web import", 18, 18],
  ["Messy-data corpus", 26, 27],
  ["Exports", 16, 17],
  ["Clustering and phonetic()", 1148, 1157],
  ["Reconciliation", 49, 49],
  ["OpenRefine project archives", 137, 137],
]

const STEPS = ["import", "facet", "cluster", "recipe", "export"]
const ok = (r) => r && r.import != null
const total = (r) => STEPS.reduce((s, k) => s + r[k], 0)
const runs = res.results.map((x) => ({
  rows: x.rows,
  ox: x.ox,
  or: ok(x.or) ? x.or : null,
  oxT: total(x.ox),
  orT: ok(x.or) ? total(x.or) : null,
  cells: x.diff?.cells,
  same: x.diff?.same,
}))
const heap = (res.orMemory ?? "1400M").replace(/M$/, "")

const n = (v) => Math.round(v).toLocaleString("en-US")
const secs = (ms) => (ms >= 1000 ? (ms / 1000).toFixed(1) + " s" : ms >= 100 ? (ms / 1000).toFixed(2) + " s" : Math.round(ms) + " ms")
const rowsLabel = (r) => (r >= 1e6 ? r / 1e6 + "M" : r / 1e3 + "k")
const times = (a, b) => (b / a >= 10 ? Math.round(b / a) : (b / a).toFixed(1)) + "×"
const f = (v) => +v.toFixed(1)

const STYLE = `<style>
svg{--bg:#FFFFFF;--ink:#111113;--mute:#5F5F66;--rule:#E4E4E0;--ox:#1D4ED8;--ox2:#5B7FE6;--or:#9A9AA0;--or2:#C6C6CA;--warn:#B45309}
@media (prefers-color-scheme:dark){svg{--bg:#0D1117;--ink:#E6EDF3;--mute:#9198A1;--rule:#30363D;--ox:#4C8DFF;--ox2:#2B5CB8;--or:#7D8590;--or2:#484F58;--warn:#E3A008}}
text{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif;fill:var(--ink)}
.m{font-family:ui-monospace,SFMono-Regular,"SF Mono",Menlo,Consolas,monospace}
.mute{fill:var(--mute)}.b{font-weight:600}.bg{fill:var(--bg)}
.ox{fill:var(--ox)}.ox2{fill:var(--ox2)}.or{fill:var(--or)}.or2{fill:var(--or2)}.warn{fill:var(--warn)}
.rule{stroke:var(--rule);fill:none}.ink{stroke:var(--ink)}
</style>`

const svg = (w, h, label, body) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" role="img" aria-label="${label}">
<title>${label}</title>
${STYLE}
<rect class="bg" width="${w}" height="${h}"/>
<rect class="rule" x="0.5" y="0.5" width="${w - 1}" height="${h - 1}"/>
${body}
</svg>
`

// A bar that grows from zero after `delay` seconds; its final width is the static fallback.
const bar = (x, y, w, h, cls, delay = 0, dur = 0.9) => {
  const t = delay + dur
  return `<rect class="${cls}" x="${f(x)}" y="${y}" width="${f(w)}" height="${h}"><animate attributeName="width" values="0;0;${f(w)}" keyTimes="0;${(delay / t).toFixed(3)};1" dur="${t}s" calcMode="spline" keySplines="0 0 1 1;0.2 0.8 0.2 1" fill="freeze"/></rect>`
}
const fadeIn = (delay) => `<animate attributeName="opacity" values="0;0;1" keyTimes="0;${(delay / (delay + 0.3)).toFixed(3)};1" dur="${delay + 0.3}s" fill="freeze"/>`

// ---------- hero: a looping race over the 1M-row workflow ----------
function hero() {
  const r = runs.find((x) => x.rows === 1_000_000)
  const W = 1200, H = 400, X = 664, L = 464, loop = 7, finish = 0.8
  const k = L / r.orT
  const oxDone = (r.oxT / r.orT) * finish
  const lane = (who, cls, y) => {
    let x = X, out = ""
    STEPS.forEach((s, i) => {
      const w = who[s] * k
      out += `<rect class="${cls}${i % 2 ? "2" : ""}" x="${f(x)}" y="${y}" width="${f(w)}" height="22"/>`
      x += w
    })
    return out
  }
  const clip = (id, y, w, end) =>
    `<clipPath id="${id}"><rect x="${X}" y="${y}" width="${f(w)}" height="22"><animate attributeName="width" values="0;${f(w)};${f(w)}" keyTimes="0;${end.toFixed(3)};1" dur="${loop}s" repeatCount="indefinite"/></rect></clipPath>`
  const show = (at) => `<animate attributeName="opacity" values="0;0;1;1" keyTimes="0;${at.toFixed(3)};${(at + 0.01).toFixed(3)};1" dur="${loop}s" repeatCount="indefinite"/>`
  let labels = "", x = X
  STEPS.forEach((s) => {
    const w = r.or[s] * k
    if (w > 40) labels += `<text class="mute" x="${f(x + w / 2)}" y="270" font-size="11" text-anchor="middle">${s}</text>`
    x += w
  })
  let axis = ""
  for (let s = 0; s * 1000 <= r.orT; s += 5) {
    const ax = X + s * 1000 * k
    axis += `<line class="rule" x1="${f(ax)}" x2="${f(ax)}" y1="284" y2="290"/><text class="m mute" x="${f(ax)}" y="304" font-size="11" text-anchor="middle">${s} s</text>`
  }
  const sp = times(r.oxT, r.orT), mem = times(r.ox.peak, r.or.peak)
  const ok = COMPAT.reduce((s, c) => s + c[1], 0), all = COMPAT.reduce((s, c) => s + c[2], 0)
  return svg(W, H, `OxRefine runs the same 1,000,000-row workflow in ${secs(r.oxT)}; OpenRefine takes ${secs(r.orT)}`, `
<text x="48" y="118" font-size="60" class="b" letter-spacing="-2">OxRefine</text>
<text x="50" y="164" font-size="20" class="mute">OpenRefine’s way of cleaning messy data,</text>
<text x="50" y="192" font-size="20" class="mute">rebuilt in Rust.</text>
<line class="rule" x1="48" x2="600" y1="236" y2="236"/>
<text x="48" y="290" font-size="38" class="m b" letter-spacing="-1">${sp}</text>
<text x="50" y="316" font-size="13" class="mute">faster at 1M rows</text>
<text x="176" y="290" font-size="38" class="m b" letter-spacing="-1">${mem}</text>
<text x="178" y="316" font-size="13" class="mute">less memory</text>
<text x="304" y="290" font-size="38" class="m b" letter-spacing="-1">${n(ok)}<tspan class="mute" font-size="22"> / ${n(all)}</tspan></text>
<text x="306" y="316" font-size="13" class="mute">checks match OpenRefine</text>
<text x="50" y="364" font-size="13" class="mute">Facet · cluster · transform with GREL · reconcile · undo — free, MIT</text>

<rect class="rule" x="640.5" y="40.5" width="511" height="319"/>
<text x="${X}" y="74" font-size="14" class="b">Same workflow, ${n(r.rows)} rows</text>
<text x="${X}" y="94" font-size="12" class="mute">import → text facet → clustering → 5-step recipe → export</text>
<defs>${clip("cox", 138, r.oxT * k, oxDone)}${clip("cor", 206, r.orT * k, finish)}</defs>
<text x="${X}" y="130" font-size="13" class="b">OxRefine</text>
<text x="${X + L}" y="130" font-size="13" class="m b" text-anchor="end">${secs(r.oxT)}${show(oxDone)}</text>
<rect class="rule" x="${X}" y="138" width="${L}" height="22"/>
<g clip-path="url(#cox)">${lane(r.ox, "ox", 138)}</g>
<text x="${f(X + r.oxT * k + 10)}" y="154" font-size="12" class="mute">done, while OpenRefine is still importing${show(oxDone)}</text>
<text x="${X}" y="198" font-size="13" class="b">OpenRefine 3.10.1</text>
<text x="${X + L}" y="198" font-size="13" class="m b" text-anchor="end">${secs(r.orT)}${show(finish)}</text>
<rect class="rule" x="${X}" y="206" width="${L}" height="22"/>
<g clip-path="url(#cor)">${lane(r.or, "or", 206)}</g>
${labels}
<line class="rule" x1="${X}" x2="${X + L}" y1="284" y2="284"/>
${axis}
<text x="${X}" y="340" font-size="11" class="mute">identical output: ${n(r.same)} of ${n(r.cells)} cells · OpenRefine heap ${n(+heap)} MB</text>
`)
}

// ---------- total time per size ----------
function time() {
  const W = 960, X = 120, L = 640, top = 92, G = 58
  const max = Math.max(...runs.map((r) => r.orT ?? 0))
  const k = L / max
  let body = "", grid = ""
  for (let s = 0; s * 1000 <= max; s += 5) {
    const gx = X + s * 1000 * k
    grid += `<line class="rule" x1="${f(gx)}" x2="${f(gx)}" y1="${top - 8}" y2="${top + runs.length * G}"/><text class="m mute" x="${f(gx)}" y="${top + runs.length * G + 18}" font-size="11" text-anchor="middle">${s} s</text>`
  }
  runs.forEach((r, i) => {
    const y = top + i * G, d = i * 0.12
    body += `<text class="m b" x="24" y="${y + 27}" font-size="13">${rowsLabel(r.rows)} rows</text>`
    body += bar(X, y + 4, Math.max(r.oxT * k, 2), 16, "ox", d)
    body += `<text class="m" x="${f(X + Math.max(r.oxT * k, 2) + 8)}" y="${y + 17}" font-size="12">${secs(r.oxT)} <tspan class="mute">· ${n(r.ox.peak)} MB</tspan>${fadeIn(d + 0.4)}</text>`
    if (r.orT) {
      body += bar(X, y + 24, r.orT * k, 16, "or", d + 0.15, 1.6)
      body += `<text class="m" x="${f(X + r.orT * k + 8)}" y="${y + 37}" font-size="12">${secs(r.orT)} <tspan class="mute">· ${n(r.or.peak)} MB</tspan>${fadeIn(d + 1.6)}</text>`
      body += `<text class="m b" x="${W - 24}" y="${y + 27}" font-size="16" text-anchor="end">${times(r.oxT, r.orT)}${fadeIn(d + 1.7)}</text>`
    } else {
      body += `<rect x="${X}" y="${y + 24}" width="${L}" height="16" fill="none" stroke="var(--warn)" stroke-dasharray="4 3"/>`
      body += `<text class="warn" x="${X + 10}" y="${y + 36}" font-size="12">OpenRefine ran out of memory (${n(+heap)} MB heap)</text>`
    }
  })
  return svg(W, top + runs.length * G + 44, "Whole-workflow time and peak memory by table size, OxRefine against OpenRefine", `
<text x="24" y="38" font-size="17" class="b">Whole workflow, wall-clock</text>
<text x="24" y="60" font-size="12" class="mute">import + text facet + clustering + 5-step recipe + export, fresh process each run · lower is better</text>
<rect class="ox" x="${W - 260}" y="28" width="12" height="12"/><text x="${W - 242}" y="39" font-size="12">OxRefine</text>
<rect class="or" x="${W - 160}" y="28" width="12" height="12"/><text x="${W - 142}" y="39" font-size="12">OpenRefine</text>
${grid}${body}`)
}

// ---------- peak memory ----------
function memory() {
  const W = 960, H = 400, x0 = 84, x1 = 900, y0 = 340, y1 = 84, xmax = 2_000_000, ymax = 1500
  const px = (r) => x0 + (r / xmax) * (x1 - x0)
  const py = (mb) => y0 - (mb / ymax) * (y0 - y1)
  const line = (pts, cls, delay) =>
    `<path d="M${pts.map(([a, b]) => f(a) + " " + f(b)).join(" L")}" pathLength="1" fill="none" stroke="var(--${cls})" stroke-width="2.5" stroke-dasharray="1 1"><animate attributeName="stroke-dashoffset" values="1;1;0" keyTimes="0;${(delay / (delay + 1.4)).toFixed(3)};1" dur="${delay + 1.4}s" fill="freeze"/></path>`
  const oxPts = runs.map((r) => [px(r.rows), py(r.ox.peak)])
  const orRuns = runs.filter((r) => r.or)
  const orPts = orRuns.map((r) => [px(r.rows), py(r.or.peak)])
  const fail = runs.find((r) => !r.or)
  let grid = ""
  for (let mb = 0; mb <= ymax; mb += 500) grid += `<line class="rule" x1="${x0}" x2="${x1}" y1="${f(py(mb))}" y2="${f(py(mb))}"/><text class="m mute" x="${x0 - 10}" y="${f(py(mb) + 4)}" font-size="11" text-anchor="end">${n(mb)}</text>`
  for (let r = 0; r <= xmax; r += 500_000) grid += `<text class="m mute" x="${f(px(r))}" y="${y0 + 22}" font-size="11" text-anchor="middle">${r ? rowsLabel(r) : "0"}</text>`
  let dots = ""
  runs.forEach((r, i) => {
    dots += `<circle cx="${f(px(r.rows))}" cy="${f(py(r.ox.peak))}" r="4" class="ox">${fadeIn(0.3 + i * 0.25)}</circle><text class="m" x="${f(px(r.rows))}" y="${f(py(r.ox.peak) - 12)}" font-size="11" text-anchor="middle">${n(r.ox.peak)}${fadeIn(0.3 + i * 0.25)}</text>`
    if (r.or) dots += `<circle cx="${f(px(r.rows))}" cy="${f(py(r.or.peak))}" r="4" class="or">${fadeIn(0.9 + i * 0.25)}</circle><text class="m" x="${f(px(r.rows) - 10)}" y="${f(py(r.or.peak) - 10)}" font-size="11" text-anchor="end">${n(r.or.peak)}${fadeIn(0.9 + i * 0.25)}</text>`
  })
  const last = orPts.at(-1)
  let crash = ""
  if (fail) {
    const cx = px(fail.rows), cy = py(+heap)
    crash = `<path d="M${f(last[0])} ${f(last[1])} L${f(cx)} ${f(cy)}" fill="none" stroke="var(--or)" stroke-width="2" stroke-dasharray="3 4"/>
<g stroke="var(--warn)" stroke-width="2.5">${fadeIn(2.4)}<line x1="${f(cx - 7)}" y1="${f(cy - 7)}" x2="${f(cx + 7)}" y2="${f(cy + 7)}"/><line x1="${f(cx - 7)}" y1="${f(cy + 7)}" x2="${f(cx + 7)}" y2="${f(cy - 7)}"/></g>
<text class="warn b" x="${f(cx - 12)}" y="${f(cy - 14)}" font-size="12" text-anchor="end">OpenRefine: out of memory${fadeIn(2.4)}</text>`
  }
  return svg(W, H, "Peak memory by table size: OxRefine stays under 220 MB, OpenRefine runs out of its heap at 2 million rows", `
<text x="24" y="38" font-size="17" class="b">Peak memory</text>
<text x="24" y="60" font-size="12" class="mute">MB, whole workflow · every column is dictionary-encoded, so each distinct value is stored once</text>
<rect class="ox" x="${W - 260}" y="28" width="12" height="12"/><text x="${W - 242}" y="39" font-size="12">OxRefine</text>
<rect class="or" x="${W - 160}" y="28" width="12" height="12"/><text x="${W - 142}" y="39" font-size="12">OpenRefine</text>
${grid}
<line x1="${x0}" x2="${x1}" y1="${f(py(+heap))}" y2="${f(py(+heap))}" stroke="var(--warn)" stroke-dasharray="6 4"/>
<text class="warn" x="${x0 + 8}" y="${f(py(+heap) - 8)}" font-size="11">OpenRefine default heap · ${n(+heap)} MB</text>
${line(orPts, "or", 0.6)}${crash}${line(oxPts, "ox", 0)}${dots}
<text class="mute" x="${x1}" y="${y0 + 44}" font-size="11" text-anchor="end">rows</text>`)
}

// ---------- per step at 1M, log scale ----------
function steps() {
  const r = runs.find((x) => x.rows === 1_000_000)
  const W = 960, x0 = 170, x1 = 840, top = 100, G = 46
  const lo = 1, hi = 20000
  const px = (ms) => x0 + (Math.log10(ms / lo) / Math.log10(hi / lo)) * (x1 - x0)
  let grid = ""
  for (const [ms, l] of [[1, "1 ms"], [10, "10 ms"], [100, "100 ms"], [1000, "1 s"], [10000, "10 s"]])
    grid += `<line class="rule" x1="${f(px(ms))}" x2="${f(px(ms))}" y1="${top - 24}" y2="${top + STEPS.length * G - 18}"/><text class="m mute" x="${f(px(ms))}" y="${top + STEPS.length * G}" font-size="11" text-anchor="middle">${l}</text>`
  const names = { import: "Import", facet: "Text facet", cluster: "Clustering", recipe: "Recipe, 5 ops", export: "Export" }
  let body = ""
  STEPS.forEach((s, i) => {
    const y = top + i * G, a = px(r.ox[s]), b = px(r.or[s]), d = 0.2 + i * 0.15
    body += `<text x="24" y="${y + 4}" font-size="13">${names[s]}</text>`
    body += `<line x1="${f(a)}" y1="${y}" x2="${f(b)}" y2="${y}" stroke="var(--rule)" stroke-width="6"><animate attributeName="x2" values="${f(a)};${f(a)};${f(b)}" keyTimes="0;${(d / (d + 1)).toFixed(3)};1" dur="${d + 1}s" calcMode="spline" keySplines="0 0 1 1;0.2 0.8 0.2 1" fill="freeze"/></line>`
    body += `<circle cx="${f(a)}" cy="${y}" r="7" class="ox"/><text class="m" x="${f(a - 12)}" y="${y + 4}" font-size="11" text-anchor="end">${secs(r.ox[s])}</text>`
    body += `<circle cx="${f(b)}" cy="${y}" r="7" class="or"><animate attributeName="cx" values="${f(a)};${f(a)};${f(b)}" keyTimes="0;${(d / (d + 1)).toFixed(3)};1" dur="${d + 1}s" calcMode="spline" keySplines="0 0 1 1;0.2 0.8 0.2 1" fill="freeze"/></circle>`
    body += `<text class="m" x="${f(b + 12)}" y="${y + 4}" font-size="11">${secs(r.or[s])}${fadeIn(d + 0.9)}</text>`
    body += `<text class="m b" x="${W - 24}" y="${y + 5}" font-size="15" text-anchor="end">${times(r.ox[s], r.or[s])}${fadeIn(d + 1)}</text>`
  })
  return svg(W, top + STEPS.length * G + 24, "Each step at 1,000,000 rows on a log scale, OxRefine against OpenRefine", `
<text x="24" y="38" font-size="17" class="b">Each step at ${n(r.rows)} rows</text>
<text x="24" y="60" font-size="12" class="mute">log scale · facets, clustering and recipes run once per distinct value, not once per row</text>
<rect class="ox" x="${W - 260}" y="28" width="12" height="12"/><text x="${W - 242}" y="39" font-size="12">OxRefine</text>
<rect class="or" x="${W - 160}" y="28" width="12" height="12"/><text x="${W - 142}" y="39" font-size="12">OpenRefine</text>
${grid}${body}`)
}

// ---------- compatibility ----------
function compat() {
  const W = 960, X = 330, L = 460, top = 92, G = 32
  let body = ""
  COMPAT.forEach(([name, okN, all], i) => {
    const y = top + i * G, miss = all - okN, mw = miss ? Math.max((miss / all) * L, 3) : 0, d = i * 0.06
    body += `<text x="24" y="${y + 13}" font-size="13">${name}</text>`
    body += `<rect class="rule" x="${X}" y="${y + 2}" width="${L}" height="14"/>`
    body += bar(X, y + 2, L - mw, 14, "ox", d)
    if (miss) body += `<rect class="warn" x="${f(X + L - mw)}" y="${y + 2}" width="${f(mw)}" height="14">${fadeIn(d + 0.8)}</rect>`
    body += `<text class="m" x="${W - 24}" y="${y + 13}" font-size="12" text-anchor="end">${n(okN)} <tspan class="mute">/ ${n(all)}</tspan></text>`
  })
  const okN = COMPAT.reduce((s, c) => s + c[1], 0), all = COMPAT.reduce((s, c) => s + c[2], 0)
  const y = top + COMPAT.length * G + 14
  return svg(W, y + 36, `${okN} of ${all} checks match OpenRefine 3.10.1`, `
<text x="24" y="38" font-size="17" class="b">Checked against the real OpenRefine 3.10.1</text>
<text x="24" y="60" font-size="12" class="mute">fixtures come from running the same cases through OpenRefine · every remaining difference is written down with its reason</text>
<rect class="ox" x="${W - 290}" y="28" width="12" height="12"/><text x="${W - 272}" y="39" font-size="12">matches</text>
<rect class="warn" x="${W - 190}" y="28" width="12" height="12"/><text x="${W - 172}" y="39" font-size="12">known difference</text>
${body}
<line class="rule" x1="24" x2="${W - 24}" y1="${y - 8}" y2="${y - 8}"/>
<text x="24" y="${y + 14}" font-size="13" class="b">All suites</text>
<text class="m b" x="${X}" y="${y + 14}" font-size="13">${((okN / all) * 100).toFixed(1)}%</text>
<text class="m b" x="${W - 24}" y="${y + 14}" font-size="13" text-anchor="end">${n(okN)} <tspan class="mute">/ ${n(all)}</tspan></text>`)
}

const files = { "hero.svg": hero(), "bench-time.svg": time(), "bench-memory.svg": memory(), "bench-steps.svg": steps(), "compat.svg": compat() }
for (const [name, body] of Object.entries(files)) {
  writeFileSync(new URL(name, OUT), body)
  console.log("docs/readme/" + name, body.length, "bytes")
}
