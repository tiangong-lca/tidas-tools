#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

request=".github/releases/v0.1.0.json"
head="$(git rev-parse HEAD)"
expected_target="9053f3bcdd1aa692c3ae56fbcda8566d373fccdc"

context="$(./scripts/validate-release-request.sh "$request" "$head")"
jq -e \
  --arg target "$expected_target" \
  '.schema == "tidas.release-context.v1"
    and .version == "0.1.0"
    and .tag == "v0.1.0"
    and .target == $target' \
  <<<"$context" >/dev/null

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

jq '.version = "0.1.1"' "$request" >"$tmp_dir/v0.1.1.json"
if ./scripts/validate-release-request.sh "$tmp_dir/v0.1.1.json" "$head" >/dev/null 2>&1; then
  echo "tampered version unexpectedly passed" >&2
  exit 1
fi

jq '.target = "0000000000000000000000000000000000000000"' "$request" >"$tmp_dir/v0.1.0.json"
if ./scripts/validate-release-request.sh "$tmp_dir/v0.1.0.json" "$head" >/dev/null 2>&1; then
  echo "unknown target unexpectedly passed" >&2
  exit 1
fi

jq '.unexpected = true' "$request" >"$tmp_dir/extra-v0.1.0.json"
if ./scripts/validate-release-request.sh "$tmp_dir/extra-v0.1.0.json" "$head" >/dev/null 2>&1; then
  echo "extra request field unexpectedly passed" >&2
  exit 1
fi

echo "release request validation passed"
