#!/usr/bin/env bash
set -euo pipefail

marker="${1:-migration/final-python-line.json}"
reachable_from="${2:-HEAD}"
expected_tag="python-final-v0.1.1"
expected_target="f7a56243cfc6d38114dac396893889e748c68c88"

if [[ ! -f "$marker" ]]; then
  echo "error: missing final Python line marker: $marker" >&2
  exit 1
fi

if ! jq -e '
  type == "object" and
  (keys == ["schema_version", "tag", "target"]) and
  .schema_version == "tidas.final-python-line.v1" and
  (.tag | type == "string") and
  (.target | type == "string")
' "$marker" >/dev/null; then
  echo "error: invalid final Python line marker schema" >&2
  exit 1
fi

tag="$(jq -r '.tag' "$marker")"
target="$(jq -r '.target' "$marker")"
if [[ "$tag" != "$expected_tag" || "$target" != "$expected_target" ]]; then
  echo "error: final Python line marker must preserve $expected_tag at $expected_target" >&2
  exit 1
fi

resolved_target="$(git rev-parse "$target^{commit}")"
if [[ "$resolved_target" != "$expected_target" ]]; then
  echo "error: final Python target does not resolve to the reviewed commit" >&2
  exit 1
fi
if ! git merge-base --is-ancestor "$target" "$reachable_from"; then
  echo "error: final Python target is not an ancestor of $reachable_from" >&2
  exit 1
fi

remote_target="$(
  git ls-remote --refs --tags origin "refs/tags/$tag" |
    awk 'NR == 1 { print $1 }'
)"
if [[ -n "$remote_target" ]]; then
  git fetch --force origin "refs/tags/$tag:refs/tags/$tag" >/dev/null
  resolved_remote="$(git rev-parse "refs/tags/$tag^{commit}")"
  if [[ "$resolved_remote" != "$expected_target" ]]; then
    echo "error: existing $tag resolves to $resolved_remote, expected $expected_target" >&2
    exit 1
  fi
fi

jq -n \
  --arg tag "$tag" \
  --arg target "$target" \
  '{tag: $tag, target: $target}'
