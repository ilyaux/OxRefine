// `ox serve` behind a reverse proxy at a sub-path, with no proxy to install:
//   node tools/proxy-check.mjs path/to/ox
// Starts ox and small proxies in front of it: one that strips /refine and rewrites Host (Apache's
// `ProxyPass /refine/ http://localhost:PORT/`), one that sends //assets (a slash on one side only,
// `ProxyPass /refine http://localhost:PORT/`) and one that keeps the visitor's host name (Caddy,
// Traefik, Pound), which ox refuses with a note until it's started with --allow-host. Then a server
// with --token. tools/proxies/matrix.mjs runs the real proxies.
import { spawn } from "node:child_process"
import { mkdtempSync, rmSync } from "node:fs"
import http from "node:http"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { refused, works } from "./serve-check.mjs"

const ox = process.argv[2]
if (!ox) throw new Error("usage: node tools/proxy-check.mjs path/to/ox")
const folders = []
const servers = []
const proxies = []

async function serve(...extra) {
  const data = mkdtempSync(join(tmpdir(), "ox-proxy-"))
  folders.push(data)
  const port = 20000 + Math.floor(Math.random() * 20000)
  servers.push(spawn(ox, ["serve", "--host", "127.0.0.1", "--port", String(port), "--data", data, ...extra], { stdio: ["ignore", "ignore", "inherit"] }))
  for (let i = 0; ; i++) {
    try {
      await fetch(`http://127.0.0.1:${port}/healthz`)
      return port
    } catch (e) {
      if (i > 100) throw e
      await new Promise((ok) => setTimeout(ok, 200))
    }
  }
}

// `map` turns the path after /refine into the backend's path; `host` is the Host header it sends on
async function proxy(port, map, host = `localhost:${port}`) {
  const p = http.createServer((req, res) => {
    if (!req.url.startsWith("/refine")) return res.writeHead(404).end("outside the proxied path")
    const upstream = http.request({ host: "127.0.0.1", port, path: map(req.url.slice("/refine".length)), method: req.method, headers: { ...req.headers, host } }, (up) => {
      res.writeHead(up.statusCode, up.headers)
      up.pipe(res)
    })
    upstream.on("error", (e) => res.writeHead(502).end(String(e)))
    req.pipe(upstream)
  })
  await new Promise((ok) => p.listen(0, "127.0.0.1", ok))
  proxies.push(p)
  return `http://127.0.0.1:${p.address().port}/refine/`
}

const strip = (rest) => rest || "/"
const run = async (name, check) => console.log(`${name}: ${await check}`)
try {
  const open = await serve()
  await run("a proxy that strips /refine", works(await proxy(open, strip)))
  await run("a proxy that sends //assets", works(await proxy(open, (rest) => "/" + rest)))
  await run("a proxy that keeps the visitor's host", refused(await proxy(open, strip, "intranet.test"), { says: "--allow-host intranet.test" }))
  await run("the same, ox started with --allow-host", works(await proxy(await serve("--allow-host", "intranet.test"), strip, "intranet.test")))
  const token = "proxy-check-token"
  await run("a proxy in front of ox serve --token", works(await proxy(await serve("--token", token), strip, "intranet.test"), { token }))
} finally {
  for (const s of servers) s.kill()
  for (const p of proxies) p.close()
  for (const d of folders) rmSync(d, { recursive: true, force: true })
}
