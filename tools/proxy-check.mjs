// `ox serve` behind a reverse proxy at a sub-path, as Apache's `ProxyPass /refine/ http://localhost:PORT/`
// or nginx's `location /refine/ { proxy_pass http://localhost:PORT/; }` would put it:
//   node tools/proxy-check.mjs path/to/ox
// Starts ox on a free port and two proxies in front of it: one that strips /refine, and one that
// adds a slash as Apache's `ProxyPass /refine http://localhost:PORT/` (a slash on one side only)
// does, sending //assets/…. Through each it checks that the page's scripts and styles load, that a
// missing file is a 404 rather than the page, and that the API and MCP answer there.
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

// a proxy that keeps Apache's default: it rewrites Host to the backend (ProxyPreserveHost Off);
// `map` turns the path after /refine into the backend's path
async function proxy(map) {
  const p = http.createServer((req, res) => {
    if (!req.url.startsWith("/refine")) return res.writeHead(404).end("outside the proxied path")
    const upstream = http.request({ host: "127.0.0.1", port, path: map(req.url.slice("/refine".length)), method: req.method, headers: { ...req.headers, host: `localhost:${port}` } }, (up) => {
      res.writeHead(up.statusCode, up.headers)
      up.pipe(res)
    })
    upstream.on("error", (e) => res.writeHead(502).end(String(e)))
    req.pipe(upstream)
  })
  await new Promise((ok) => p.listen(0, "127.0.0.1", ok))
  return p
}

async function check(name, base) {
  const page = await (await fetch(`${base}/refine/`)).text()
  assert.match(page, /<div id="root">/, `${name}: the page comes through the proxy`)
  // without the trailing slash the page sends the browser to /refine/ before anything else loads
  assert.match(page, /pathname \+ "\/"/, `${name}: the page adds the trailing slash`)
  const refs = [...page.matchAll(/(?:src|href)="([^"]+)"/g)].map((m) => m[1])
  assert.ok(refs.length >= 2, `${name}: scripts and styles referenced: ${refs}`)
  for (const ref of refs) {
    assert.ok(!ref.startsWith("/"), `${ref} would bypass the proxy's sub-path`)
    const r = await fetch(new URL(ref, `${base}/refine/`))
    assert.equal(r.status, 200, `${name}: ${ref}`)
    const type = r.headers.get("content-type")
    if (ref.endsWith(".js")) assert.match(type, /javascript/, `${name}: ${ref} is ${type}, not the script`)
    if (ref.endsWith(".css")) assert.match(type, /css/, `${name}: ${ref} is ${type}, not the styles`)
  }
  // a file that isn't there is a 404: the page in its place would leave the browser a blank screen
  const missing = await fetch(`${base}/refine/assets/index-missing.js`)
  assert.equal(missing.status, 404, `${name}: a missing script`)
  // the API resolves relative to the page too
  const api = await fetch(new URL("api/projects", `${base}/refine/`), { method: "POST", headers: { "Content-Type": "application/json", "X-Ox-Session": "proxy-check" }, body: "{}" })
  assert.equal(api.status, 200, `${name}: ${await api.clone().text()}`)
  assert.ok(Array.isArray((await api.json()).data), `${name}: projects come back as a list`)
  // and MCP clients connect at /refine/mcp
  const initialize = { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-11-25", capabilities: {}, clientInfo: { name: "proxy-check", version: "1" } } }
  const mcp = await fetch(`${base}/refine/mcp`, { method: "POST", headers: { "Content-Type": "application/json", Accept: "application/json, text/event-stream" }, body: JSON.stringify(initialize) })
  assert.equal(mcp.status, 200, `${name}: ${await mcp.clone().text()}`)
  assert.ok(mcp.headers.get("mcp-session-id"), `${name}: the MCP session id comes through the proxy`)
  assert.equal((await mcp.json()).result.serverInfo.name, "oxrefine")
  console.log(`ox serve works behind ${name} at /refine/: page, ${refs.length} assets, a 404, the API and MCP`)
}

const proxies = []
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
  for (const [name, map] of [
    ["a proxy that strips /refine", (rest) => rest || "/"],
    ["a proxy that sends //assets (ProxyPass /refine http://host:port/)", (rest) => "/" + rest],
  ]) {
    const p = await proxy(map)
    proxies.push(p)
    await check(name, `http://127.0.0.1:${p.address().port}`)
  }
} finally {
  server.kill()
  for (const p of proxies) p.close()
  rmSync(data, { recursive: true, force: true })
}
