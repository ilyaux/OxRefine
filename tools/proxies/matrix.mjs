// `ox serve` behind real reverse proxies, as people deploy it (Linux, run by CI):
//   node tools/proxies/matrix.mjs path/to/ox [nginx,apache,caddy,pound,traefik]
// Needs nginx, apache2, caddy and pound installed, Docker for Traefik, and intranet.test pointing to
// 127.0.0.1 in /etc/hosts. Three ox servers stand behind each proxy: one without a token, one with
// --allow-host intranet.test and one with --token. Each proxy serves them at the site root and at /ox/
// (Pound: the root only), with its usual configuration; tools/serve-check.mjs checks what a person
// does there. Proxies that rewrite the Host header (nginx, Apache) work with a plain ox; those that
// pass on the visitor's host name (Caddy, Pound, Traefik) get a refusal saying to add --allow-host,
// and work with it. The configurations are the ones docs/src/deploying.md shows.
import { execFileSync, spawn } from "node:child_process"
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs"
import net from "node:net"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { refused, works } from "../serve-check.mjs"

const [ox, only] = process.argv.slice(2)
if (!ox) throw new Error("usage: node tools/proxies/matrix.mjs path/to/ox [proxies]")
const work = mkdtempSync(join(tmpdir(), "ox-proxies-"))
const TOKEN = "matrix-token"
// the three servers: port, arguments, and whether a proxy that keeps the visitor's host name reaches it
const BACKENDS = [
  { name: "ox", port: 3101, args: [] },
  { name: "ox --allow-host", port: 3102, args: ["--allow-host", "intranet.test"] },
  { name: "ox --token", port: 3103, args: ["--token", TOKEN] },
]
const children = []
const stops = []

const listening = (port) =>
  new Promise((ok) => {
    const s = net.connect(port, "127.0.0.1", () => (s.end(), ok(true)))
    s.on("error", () => ok(false))
  })
async function ready(ports, what) {
  for (let i = 0; i < 150; i++) {
    if ((await Promise.all(ports.map(listening))).every(Boolean)) return
    await new Promise((ok) => setTimeout(ok, 200))
  }
  throw new Error(`${what} didn't listen on ${ports}`)
}
function start(cmd, args, what) {
  const child = spawn(cmd, args, { stdio: ["ignore", "inherit", "inherit"] })
  child.on("exit", (code) => code && console.error(`${what} exited with ${code}`))
  children.push(child)
  return child
}

// each proxy: how it's started, the ports it listens on (one per backend) and its paths
const ports = (base) => BACKENDS.map((_, i) => base + i + 1)
const PROXIES = {
  nginx: {
    keepsHost: false,
    paths: ["", "ox/"],
    async start(dir) {
      const servers = BACKENDS.map((b, i) => `
  server {
    listen ${8101 + i};
    location = /ox { return 301 /ox/; }
    location /ox/ { proxy_pass http://127.0.0.1:${b.port}/; }
    location / { proxy_pass http://127.0.0.1:${b.port}; }
  }`).join("")
      writeFileSync(join(dir, "nginx.conf"), `pid ${dir}/nginx.pid;
error_log stderr warn;
events {}
http {
  access_log off;
  client_body_temp_path ${dir}/body;
  proxy_temp_path ${dir}/proxy;
  fastcgi_temp_path ${dir}/fastcgi;
  uwsgi_temp_path ${dir}/uwsgi;
  scgi_temp_path ${dir}/scgi;
  client_max_body_size 0;
  proxy_read_timeout 600s;
  proxy_send_timeout 600s;${servers}
}
`)
      start("nginx", ["-e", "stderr", "-p", dir, "-c", join(dir, "nginx.conf"), "-g", "daemon off;"], "nginx")
      return ports(8100)
    },
  },
  apache: {
    keepsHost: false,
    // ox2/: a slash on one side only, as in ProxyPass /ox2 http://127.0.0.1:PORT/ (Apache sends //assets)
    paths: ["", "ox/", "ox2/"],
    async start(dir) {
      const m = "/usr/lib/apache2/modules"
      const hosts = BACKENDS.map((b, i) => `
Listen ${8201 + i}
<VirtualHost *:${8201 + i}>
  RedirectMatch ^/ox$ /ox/
  ProxyPass        /ox/ http://127.0.0.1:${b.port}/ timeout=600
  ProxyPassReverse /ox/ http://127.0.0.1:${b.port}/
  ProxyPass        /ox2 http://127.0.0.1:${b.port}/ timeout=600
  ProxyPassReverse /ox2 http://127.0.0.1:${b.port}/
  ProxyPass        /    http://127.0.0.1:${b.port}/ timeout=600
  ProxyPassReverse /    http://127.0.0.1:${b.port}/
</VirtualHost>`).join("")
      writeFileSync(join(dir, "apache.conf"), `ServerRoot ${dir}
ServerName localhost
PidFile ${dir}/httpd.pid
ErrorLog ${dir}/error.log
Mutex file:${dir} default
LoadModule mpm_event_module ${m}/mod_mpm_event.so
LoadModule authz_core_module ${m}/mod_authz_core.so
LoadModule alias_module ${m}/mod_alias.so
LoadModule proxy_module ${m}/mod_proxy.so
LoadModule proxy_http_module ${m}/mod_proxy_http.so
${hosts}
`)
      start("apache2", ["-f", join(dir, "apache.conf"), "-DFOREGROUND"], "apache")
      return ports(8200)
    },
  },
  caddy: {
    keepsHost: true,
    paths: ["", "ox/"],
    async start(dir) {
      const sites = BACKENDS.map((b, i) => `
:${8301 + i} {
  redir /ox /ox/
  handle_path /ox/* {
    reverse_proxy 127.0.0.1:${b.port}
  }
  handle {
    reverse_proxy 127.0.0.1:${b.port}
  }
}`).join("")
      writeFileSync(join(dir, "Caddyfile"), `{
  admin off
  auto_https off
  persist_config off
  storage file_system ${dir}/caddy
}
${sites}
`)
      start("caddy", ["run", "--config", join(dir, "Caddyfile"), "--adapter", "caddyfile"], "caddy")
      return ports(8300)
    },
  },
  pound: {
    keepsHost: true,
    // Pound serves a site's root; a sub-path would need it to rewrite the path
    paths: [""],
    async start(dir) {
      const version = execFileSync("pound", ["-V"], { encoding: "utf8" }).match(/(\d+)\.\d+/)?.[1] ?? "4"
      // the reporter's rule from issue #6; Pound 4 calls it Header, older ones HeadRequire
      const match = Number(version) >= 4 ? `Header "Host: .*(intranet.test).*"` : `HeadRequire "Host: .*(intranet.test).*"`
      const listeners = BACKENDS.map((b, i) => `
ListenHTTP
  Address 0.0.0.0
  Port ${8401 + i}
  Service
    ${match}
    Backend
      Address 127.0.0.1
      Port ${b.port}
      TimeOut 600
    End
  End
End`).join("")
      writeFileSync(join(dir, "pound.cfg"), `Daemon 0\nLogLevel 1\n${listeners}\n`)
      start("pound", ["-f", join(dir, "pound.cfg"), "-p", join(dir, "pound.pid")], "pound")
      return ports(8400)
    },
  },
  traefik: {
    keepsHost: true,
    paths: ["", "ox/"],
    async start(dir) {
      const routers = BACKENDS.flatMap((b, i) => [
        `    root${i}:\n      entryPoints: [e${i}]\n      rule: "PathPrefix(\`/\`)"\n      priority: 1\n      service: ox${i}`,
        `    sub${i}:\n      entryPoints: [e${i}]\n      rule: "PathPrefix(\`/ox\`)"\n      priority: 10\n      middlewares: [strip]\n      service: ox${i}`,
      ]).join("\n")
      const services = BACKENDS.map((b, i) => `    ox${i}:\n      loadBalancer:\n        servers:\n          - url: "http://127.0.0.1:${b.port}"`).join("\n")
      writeFileSync(join(dir, "traefik.yml"), `http:\n  routers:\n${routers}\n  middlewares:\n    strip:\n      stripPrefix:\n        prefixes: ["/ox"]\n  services:\n${services}\n`)
      const entry = BACKENDS.map((_, i) => `--entryPoints.e${i}.address=:${8501 + i}`)
      execFileSync("docker", ["run", "-d", "--rm", "--name", "ox-traefik", "--network", "host", "-v", `${dir}:/conf:ro`, "traefik:v3.1", ...entry, "--providers.file.filename=/conf/traefik.yml"], { stdio: "inherit" })
      stops.push(() => execFileSync("docker", ["rm", "-f", "ox-traefik"], { stdio: "ignore" }))
      return ports(8500)
    },
  },
}

const results = []
let failed = 0
try {
  for (const b of BACKENDS) {
    const data = join(work, b.name.replace(/\W+/g, "-"))
    mkdirSync(data, { recursive: true })
    start(ox, ["serve", "--host", "127.0.0.1", "--port", String(b.port), "--data", data, ...b.args], b.name)
  }
  await ready(BACKENDS.map((b) => b.port), "ox")
  for (const [name, proxy] of Object.entries(PROXIES)) {
    if (only && !only.split(",").includes(name)) continue
    const dir = join(work, name)
    mkdirSync(dir, { recursive: true })
    let listen
    try {
      listen = await proxy.start(dir)
      await ready(listen, name)
    } catch (e) {
      failed++
      results.push([name, "start", "", `FAILED ${e.message}`])
      continue
    }
    for (const [i, b] of BACKENDS.entries()) {
      for (const path of proxy.paths) {
        const base = `http://intranet.test:${listen[i]}/${path}`
        const expect = proxy.keepsHost && !b.args.length ? "refused" : "works"
        try {
          const said = expect === "refused" ? await refused(base, { says: "--allow-host intranet.test" }) : await works(base, { token: b.args.includes("--token") ? TOKEN : undefined })
          results.push([name, b.name, `/${path}`, said])
        } catch (e) {
          failed++
          results.push([name, b.name, `/${path}`, `FAILED ${e.message.split("\n")[0]}`])
        }
      }
    }
    for (const c of children.splice(BACKENDS.length)) c.kill()
    for (const stop of stops.splice(0)) stop()
  }
} finally {
  for (const c of children) c.kill()
  for (const stop of stops) stop()
  rmSync(work, { recursive: true, force: true })
}
for (const r of results) console.log(r.map((x, i) => (i < 3 ? x.padEnd([8, 16, 6][i]) : x)).join(" "))
console.log(failed ? `${failed} failed` : `${results.length} checks passed`)
process.exitCode = failed ? 1 : 0
