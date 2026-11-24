# Running OxRefine on a server

`ox serve` puts the whole app in a browser. This page covers serving it to other people: on a network, behind a reverse proxy for HTTPS or an intranet address, as a system service and in Docker. CI runs each proxy below with these settings on every change (`tools/proxies/matrix.mjs`): at the site root and at a sub-path, with and without a token, uploads, downloads and MCP included.

## Who gets in: one rule

- **With a token,** the token decides. A person opens the link `ox serve` prints, ending in `?token=…`; the page keeps the token and sends it with every request. Without it, the address shows only a note asking for the link, and every request is refused. API clients send `Authorization: Bearer <token>`.
- **Without a token,** OxRefine answers only requests addressed to this machine (`127.0.0.1`, `localhost` or `::1`, on any port) or to a host name given with `--allow-host`. This keeps web pages from reaching your server through a DNS name they control.

When `--host` makes OxRefine listen beyond this machine, it requires a token and makes one up if you don't give one. A refused browser gets a page saying why and what to do; a refused API client gets the same text in its error; whoever runs `ox serve` reads it once on stderr.

```bash
ox serve --host 0.0.0.0 --data /srv/oxrefine                     # prints a link with a new token
ox serve --host 0.0.0.0 --data /srv/oxrefine --token <secret>    # your own token
OXREFINE_TOKEN=<secret> ox serve --host 0.0.0.0 --data /srv/oxrefine
```

Prefer `OXREFINE_TOKEN` to `--token` on a shared machine: other users can read command lines in the process list. Anyone with the link can read and write files as the user OxRefine runs as, so run it under a user with limited rights, and change the token to take access away. One token is shared by everyone: OxRefine has no accounts, and the people with the link see the same projects. Several can work at once; each browser tab opens its own project, and an edit made on an outdated view is refused rather than overwriting someone's newer change.

## Behind a reverse proxy

Let OxRefine listen on this machine only and put the proxy in front for HTTPS, an intranet name or a path of an existing site. The app uses relative addresses, so it works at the root or under any path, such as `https://intranet.example.com/ox/`.

```bash
ox serve --host 127.0.0.1 --port 3030 --data /srv/oxrefine
```

What matters is whether the proxy passes on the visitor's host name:

| Proxy | Host header OxRefine sees | Start OxRefine with |
|---|---|---|
| nginx, Apache httpd (default settings) | `127.0.0.1:3030` | nothing more |
| Caddy, Traefik, Pound, Apache with `ProxyPreserveHost On` | `intranet.example.com` | `--allow-host intranet.example.com`, or a token |

`--allow-host` names the address people use; repeat it for several. It's enough when the proxy itself decides who gets in (its own login, a VPN, an intranet). Add a token as well to have OxRefine check every request too; then people open `https://intranet.example.com/ox/?token=<secret>`.

### nginx

```nginx
location = /ox { return 301 /ox/; }
location /ox/ {
    proxy_pass http://127.0.0.1:3030/;
    proxy_read_timeout 600s;
    proxy_send_timeout 600s;
    client_max_body_size 0;
}
```

At the site root, `location / { proxy_pass http://127.0.0.1:3030; }`. Keep the slash after the port in a sub-path's `proxy_pass`: it replaces `/ox/` with `/`. `client_max_body_size 0` lifts nginx's 1 MB upload limit.

### Apache httpd

```apache
RedirectMatch ^/ox$ /ox/
ProxyPass        /ox/ http://127.0.0.1:3030/ timeout=600
ProxyPassReverse /ox/ http://127.0.0.1:3030/
```

Needs `mod_proxy` and `mod_proxy_http` (`a2enmod proxy_http` on Debian and Ubuntu). At the site root, `ProxyPass / http://127.0.0.1:3030/`. A slash on one side only (`ProxyPass /ox http://127.0.0.1:3030/`) also works since 0.8.0.

### Caddy

```caddy
intranet.example.com {
    redir /ox /ox/
    handle_path /ox/* {
        reverse_proxy 127.0.0.1:3030
    }
}
```

`handle_path` removes `/ox` before passing the request on. Caddy passes on the visitor's host name: start OxRefine with `--allow-host intranet.example.com` or a token.

### Traefik

```yaml
http:
  routers:
    oxrefine:
      rule: "Host(`intranet.example.com`) && PathPrefix(`/ox`)"
      middlewares: [oxrefine-path]
      service: oxrefine
  middlewares:
    oxrefine-path:
      stripPrefix:
        prefixes: ["/ox"]
  services:
    oxrefine:
      loadBalancer:
        servers:
          - url: "http://127.0.0.1:3030"
```

Traefik passes on the visitor's host name (`passHostHeader`): start OxRefine with `--allow-host intranet.example.com` or a token.

### Pound

```
ListenHTTP
    Address 0.0.0.0
    Port    80
    Service
        Header "Host: .*intranet.example.com.*"
        Backend
            Address 127.0.0.1
            Port    3030
            TimeOut 600
        End
    End
End
```

Pound serves OxRefine at a site's root (Pound 2 calls `Header` `HeadRequire`). It passes on the visitor's host name: start OxRefine with `--allow-host intranet.example.com` or a token.

## As a system service

A systemd unit, with the token in a file only root and the service read:

```ini
# /etc/systemd/system/oxrefine.service
[Unit]
Description=OxRefine
After=network.target

[Service]
User=oxrefine
EnvironmentFile=/etc/oxrefine.env
ExecStart=/usr/local/bin/ox serve --host 127.0.0.1 --port 3030 --data /srv/oxrefine
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

```bash
echo "OXREFINE_TOKEN=$(openssl rand -hex 18)" | sudo tee /etc/oxrefine.env
sudo chmod 600 /etc/oxrefine.env
sudo systemctl enable --now oxrefine
journalctl -u oxrefine        # the link with the token, refusals, and with --log each request
```

`ox --version` and `ox --help` print to standard output and exit 0, as package scripts expect.

## Docker

```bash
docker build -t oxrefine .
docker run -d -p 3030:3030 -v oxrefine-data:/data -e OXREFINE_TOKEN=<secret> oxrefine
```

The container runs `ox serve --host 0.0.0.0` with projects in `/data`. Without `OXREFINE_TOKEN` the log prints a link with a new token. CI builds this image and uses it on each change.

## Health checks and logs

- **`/healthz`** answers `ok` to anyone, token or not, and says nothing else: for load balancers, Docker and Kubernetes. `HEAD` works there and on every page and file.
- **`--log`** prints each request on stderr: method, path, status and time. Never the query, so never the token.
- **Refusals** are printed once each on stderr, with what to change: a host name to allow, or the link with the token.
- **The token in the page address** reaches your proxy's access log with the first request, as it does in Jupyter. OxRefine sends `Referrer-Policy: no-referrer`, so it doesn't spread further, in the log's referrer field or to other sites. To keep it out of the log altogether, log the path without the query string, or rely on `--allow-host` and the proxy's own login.

## When something goes wrong

- **A note instead of the app.** OxRefine refused the request, and the note says why: the link without its token, or a host name it doesn't answer. stderr says the same, with the flag to add.
- **502, 503 or 504 from the proxy.** The proxy didn't get an answer from OxRefine. Check it runs and listens where the proxy looks: `curl http://127.0.0.1:3030/healthz` on the proxy's machine answers `ok`. Some proxies (Pound, Traefik) keep a backend marked as down for a while after a failed connection. A 504 on a long import means the proxy's timeout is too short: give it minutes.
- **413 on an upload.** The proxy caps request bodies: `client_max_body_size 0` in nginx, `LimitRequestBody 0` in Apache.
- **A blank page.** The page loaded but its scripts didn't. Look at the proxy's log for the page's files under `/ox/assets/`: a 404 there means the proxy sends OxRefine a path it doesn't serve; check the path mapping above.
