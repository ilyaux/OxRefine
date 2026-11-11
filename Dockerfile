# OxRefine in a container: the app in a browser, for servers (like running OpenRefine remotely).
#   docker build -t oxrefine .
#   docker run -p 3030:3030 -v oxrefine-data:/data oxrefine
# The log prints the link with its access token; pass `--token <secret>` after the image name to
# choose one. Projects live in /data.

FROM node:22-slim AS ui
WORKDIR /src/ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci
COPY ui/ ./
RUN npm run build

FROM rust:1-slim-bookworm AS ox
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# a workspace member; only its manifest and sources are needed to resolve the workspace
COPY ui/src-tauri ui/src-tauri
COPY --from=ui /src/ui/dist ui/dist
RUN cargo build --release -p ox-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=ox /src/target/release/ox /usr/local/bin/ox
VOLUME /data
EXPOSE 3030
ENTRYPOINT ["ox", "serve", "--host", "0.0.0.0", "--port", "3030", "--data", "/data"]
