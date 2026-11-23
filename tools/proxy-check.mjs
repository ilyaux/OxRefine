// `ox serve` behind a reverse proxy at a sub-path, as Apache's `ProxyPass /refine/ http://localhost:PORT/`
// or nginx's `location /refine/ { proxy_pass http://localhost:PORT/; }` would put it:
//   node tools/proxy-check.mjs path/to/ox
// Starts ox on a free port and a proxy that strips /refine, then checks that /refine redirects to
// /refine/, that the page's scripts and styles load through the proxy, and that the API answers there.
import { spawn } from "node:child_process"
import { mkdtempSync, rmSync } from "node:fs"
import http from "node:http"
import { tmpdir } from "node:os"
import { join } from "node:path"
import assert from "node:assert/strict"

const ox = process.argv[2]
if (!ox) throw new Error("usage: node tools/proxy-check.mjs path/to/ox")
const data = mkdtempSync(join(tmpdir(), "ox-proxy-"))
const port = 20000 + Math.floor(Math.random() * 20000)
const server = spawn(ox, ["serve", "--host", "127.0.0.1", "--port", String(port), "--data", data], { stdio: ["ignore", "inherit", "inherit"] })

// the proxy keeps Apache's default: it rewrites Host to the backend (ProxyPreserveHost Off)
const proxy = http.createServer((req, res) => {
  if (!req.url.startsWith("/refine")) return res.writeHead(404).end("outside the proxied path")
  const path = req.url.slice("/refine".length) || "/"
  const upstream = http.request({ host: "127.0.0.1", port, path, method: req.method, headers: { ...req.headers, host: `localhost:${port}` } }, (up) => {
    res.writeHead(up.statusCode, up.headers)
    up.pipe(res)
  })
  upstream.on("error", (e) => res.writeHead(502).end(String(e)))
  req.pipe(upstream)
})
await new Promise((ok) => proxy.listen(0, "127.0.0.1", ok))
const base = `http://127.0.0.1:${proxy.address().port}`

try {
  for (let i = 0; ; i++) {
    try {
      await fetch(`http://127.0.0.1:${port}/`)
      break
    } catch (e) {
      if (i > 100) throw e
      await new Promise((ok) => setTimeout(ok, 200))
    }
  }
  const page = await (await fetch(`${base}/refine/`)).text()
  assert.match(page, /<div id="root">/, "the page comes through the proxy")
  // without the trailing slash the page sends the browser to /refine/ before anything else loads
  assert.match(page, /pathname \+ "\/"/, "the page adds the trailing slash")
  const refs = [...page.matchAll(/(?:src|href)="([^"]+)"/g)].map((m) => m[1])
  assert.ok(refs.length >= 2, `scripts and styles referenced: ${refs}`)
  for (const ref of refs) {
    assert.ok(!ref.startsWith("/"), `${ref} would bypass the proxy's sub-path`)
    const r = await fetch(new URL(ref, `${base}/refine/`))
    assert.equal(r.status, 200, ref)
    if (ref.endsWith(".js")) assert.match(r.headers.get("content-type"), /javascript/, ref)
  }
  // the API resolves relative to the page too
  const api = await fetch(new URL("api/projects", `${base}/refine/`), { method: "POST", headers: { "Content-Type": "application/json", "X-Ox-Session": "proxy-check" }, body: "{}" })
  assert.equal(api.status, 200, await api.clone().text())
  assert.ok(Array.isArray((await api.json()).data), "projects come back as a list")
  // and MCP clients connect at /refine/mcp
  const initialize = { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-11-25", capabilities: {}, clientInfo: { name: "proxy-check", version: "1" } } }
  const mcp = await fetch(`${base}/refine/mcp`, { method: "POST", headers: { "Content-Type": "application/json", Accept: "application/json, text/event-stream" }, body: JSON.stringify(initialize) })
  assert.equal(mcp.status, 200, await mcp.clone().text())
  assert.ok(mcp.headers.get("mcp-session-id"), "the MCP session id comes through the proxy")
  assert.equal((await mcp.json()).result.serverInfo.name, "oxrefine")
  console.log(`ox serve works behind a proxy at /refine/: page, ${refs.length} assets, the API and MCP`)
} finally {
  server.kill()
  proxy.close()
  rmSync(data, { recursive: true, force: true })
}
