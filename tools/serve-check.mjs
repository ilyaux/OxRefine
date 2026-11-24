// What a person behind a reverse proxy does with `ox serve`, as checks on an address such as
// http://intranet.test:8101/ox/ (used by tools/proxy-check.mjs and tools/proxies/matrix.mjs):
//   works(base, { token })    the page, its scripts and styles, a 404 for a missing file, the API, a
//                             5 MB upload opened as a project, an export downloaded, MCP, /healthz
//   refused(base, { says })   neither the page nor the API, and both say what to do (`says`)
import assert from "node:assert/strict"

const SIZE = 5 * 1024 * 1024

export async function works(base, { token } = {}) {
  const auth = token ? { Authorization: `Bearer ${token}` } : {}
  const api = async (cmd, args = {}) => {
    const r = await fetch(new URL(`api/${cmd}`, base), { method: "POST", headers: { ...auth, "Content-Type": "application/json" }, body: JSON.stringify(args) })
    const text = await r.text()
    assert.equal(r.status, 200, `api/${cmd}: ${r.status} ${text.slice(0, 300)}`)
    return JSON.parse(text).data
  }
  if (token) {
    // without the token not even the page, and the note says how to get in
    const page = await fetch(base)
    assert.equal(page.status, 403, "the page without the token")
    assert.match(await page.text(), /\?token=/, "the note asks for the link")
    const r = await fetch(new URL("api/projects", base), { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" })
    assert.equal(r.status, 403, "the API without the token")
  }
  const page = await fetch(token ? `${base}?token=${token}` : base)
  const html = await page.text()
  assert.equal(page.status, 200, `the page: ${html.slice(0, 300)}`)
  assert.match(html, /<div id="root">/, "the page is the app")
  assert.equal(page.headers.get("referrer-policy"), "no-referrer", "the token never goes on as the referrer")
  const refs = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((m) => m[1])
  assert.ok(refs.length >= 2, `scripts and styles referenced: ${refs}`)
  for (const ref of refs) {
    assert.ok(!ref.startsWith("/"), `${ref} would bypass a proxy's sub-path`)
    const r = await fetch(new URL(ref, base))
    assert.equal(r.status, 200, ref)
    const type = r.headers.get("content-type") ?? ""
    if (ref.endsWith(".js")) assert.match(type, /javascript/, `${ref} is ${type}, not the script`)
    if (ref.endsWith(".css")) assert.match(type, /css/, `${ref} is ${type}, not the styles`)
  }
  // a missing file is a 404: the page in its place would leave the browser a blank screen
  assert.equal((await fetch(new URL("assets/index-missing.js", base))).status, 404, "a missing script")
  // health checks need nothing and get an answer, HEAD included
  const health = await fetch(new URL("healthz", base), { method: "HEAD" })
  assert.equal(health.status, 200, "HEAD healthz")
  assert.ok(Array.isArray(await api("projects")), "projects come back as a list")

  // a file bigger than the usual proxy body limits goes up, opens as a project, and an export comes back
  const rows = Math.ceil(SIZE / 32)
  const csv = "id,text\n" + Array.from({ length: rows }, (_, i) => `${i},${"x".repeat(24)}`).join("\n") + "\n"
  const up = await fetch(new URL("upload", base), { method: "POST", headers: { ...auth, "X-File-Name": "big.csv" }, body: csv })
  const upText = await up.text()
  assert.equal(up.status, 200, `upload of ${(csv.length / 1048576).toFixed(1)} MB: ${up.status} ${upText.slice(0, 300)}`)
  const opened = await api("open", { path: JSON.parse(upText).data.path })
  assert.equal(opened.rows, rows, "the uploaded file arrived whole")
  const staged = await fetch(new URL("stage", base), { method: "POST", headers: { ...auth, "Content-Type": "application/json" }, body: JSON.stringify({ name: "out.tsv" }) })
  assert.equal(staged.status, 200, "stage")
  const path = (await staged.json()).data.path
  await api("export", { path, format: "tsv" })
  const download = await fetch(new URL(`download?path=${encodeURIComponent(path)}${token ? `&token=${token}` : ""}`, base))
  assert.equal(download.status, 200, "download")
  assert.equal((await download.text()).split("\n").filter(Boolean).length, rows + 1, "the export came back whole")

  // MCP clients connect at mcp
  const init = { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-11-25", capabilities: {}, clientInfo: { name: "serve-check", version: "1" } } }
  const mcp = await fetch(new URL("mcp", base), { method: "POST", headers: { ...auth, "Content-Type": "application/json", Accept: "application/json, text/event-stream" }, body: JSON.stringify(init) })
  assert.equal(mcp.status, 200, `mcp: ${await mcp.clone().text()}`)
  assert.equal((await mcp.json()).result.serverInfo.name, "oxrefine")
  return `page, ${refs.length} assets, 404, healthz, API, ${(csv.length / 1048576).toFixed(1)} MB up, export down, MCP`
}

export async function refused(base, { says }) {
  const page = await fetch(base)
  const text = await page.text()
  assert.equal(page.status, 403, `the page: ${text.slice(0, 200)}`)
  assert.doesNotMatch(text, /<div id="root">/, "no app for a refused request")
  assert.ok(text.includes(says), `the page says "${says}": ${text.slice(0, 400)}`)
  const r = await fetch(new URL("api/projects", base), { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" })
  const error = await r.text()
  assert.equal(r.status, 403, "the API")
  assert.ok(error.includes(says), `the API says "${says}": ${error}`)
  return `refused, saying ${says}`
}
