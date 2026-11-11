// The web server that fetch-URL cases call (see SERVER in cases.mjs); cases write its address as
// {BASE}. Returns { base, close }.
import { createServer } from "node:http"

export async function serve() {
  const counts = {}
  const server = createServer((req, res) => {
    const [, kind, rest = ""] = req.url.split("/")
    if (kind === "echo") return res.writeHead(200, { "content-type": "text/plain" }).end("echo:" + decodeURIComponent(rest))
    if (kind === "status") return res.writeHead(Number(rest), { "content-type": "text/plain" }).end("status " + rest)
    if (kind === "header") return res.writeHead(200, { "content-type": "text/plain" }).end(String(req.headers["x-ox"] ?? ""))
    if (kind === "count") return res.writeHead(200, { "content-type": "text/plain" }).end(String((counts[rest] = (counts[rest] ?? 0) + 1)))
    res.writeHead(404).end()
  }).listen(0, "127.0.0.1")
  await new Promise((r) => server.once("listening", r))
  return { base: `http://127.0.0.1:${server.address().port}`, close: () => server.close() }
}

/** The ops with {BASE} filled in. */
export const withBase = (ops, base) => JSON.parse(JSON.stringify(ops).replaceAll("{BASE}", base))
