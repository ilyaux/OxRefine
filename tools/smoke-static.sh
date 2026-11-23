#!/usr/bin/env bash
# Run a static (musl) `ox` on distributions the glibc build can't start on: Alpine (musl) and
# Alma Linux 8 (glibc 2.28). Each replays a recipe with a GREL and a Python step on a small CSV.
#   tools/smoke-static.sh target/x86_64-unknown-linux-musl/release/ox
set -euo pipefail
ox=$(realpath "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
printf 'name,city\nAnn, oslo\nBob,rome\n' > "$work/in.csv"
cat > "$work/recipe.json" <<'EOF'
[
  {"op": "core/text-transform", "engineConfig": {"facets": [], "mode": "row-based"}, "columnName": "city",
   "expression": "grel:value.trim()", "onError": "keep-original", "repeat": false, "repeatCount": 10},
  {"op": "core/text-transform", "engineConfig": {"facets": [], "mode": "row-based"}, "columnName": "city",
   "expression": "jython:return value.upper()", "onError": "keep-original", "repeat": false, "repeatCount": 10}
]
EOF
printf 'name,city\nAnn,OSLO\nBob,ROME\n' > "$work/expected.csv"
file "$ox" | tee /dev/stderr | grep -Eq "statically linked|static-pie linked"
for image in alpine:3.22 almalinux:8; do
  rm -f "$work/out.csv"
  docker run --rm --user "$(id -u):$(id -g)" -e HOME=/work -v "$ox:/usr/local/bin/ox:ro" -v "$work:/work" "$image" ox apply /work/recipe.json /work/in.csv -o /work/out.csv
  diff <(tr -d '\r' < "$work/out.csv") "$work/expected.csv"
  echo "$image: ok"
done
