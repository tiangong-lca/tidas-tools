#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

usage() {
  echo "usage: scripts/validate-release-request.sh <request.json> <release-head>" >&2
  exit 64
}

request_path="${1:-}"
release_head="${2:-}"
if [[ -z "$request_path" || -z "$release_head" || $# -ne 2 ]]; then
  usage
fi
if [[ ! -f "$request_path" || -L "$request_path" ]]; then
  echo "release request must be a regular non-symlink file: $request_path" >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required to validate a release request" >&2
  exit 1
fi

request="$(
  jq -ce '
    if type != "object"
      or keys != ["request_schema", "target", "version"]
      or .request_schema != "tidas.release-request.v1"
      or (.version | type) != "string"
      or (.target | type) != "string"
    then error("expected exactly request_schema, version, and target")
    else .
    end
  ' "$request_path"
)" || {
  echo "invalid release request structure: $request_path" >&2
  exit 1
}

version="$(jq -r '.version' <<<"$request")"
target="$(jq -r '.target' <<<"$request")"
tag="v$version"

if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$ ]]; then
  echo "release request version is not a supported semantic version: $version" >&2
  exit 1
fi
if [[ "$(basename "$request_path")" != "$tag.json" ]]; then
  echo "release request filename must be $tag.json" >&2
  exit 1
fi
if [[ ! "$target" =~ ^[0-9a-f]{40}$ ]]; then
  echo "release request target must be a full lowercase commit SHA" >&2
  exit 1
fi
if ! git cat-file -e "$target^{commit}" 2>/dev/null; then
  echo "release request target is not available as a commit: $target" >&2
  exit 1
fi
if ! git cat-file -e "$release_head^{commit}" 2>/dev/null; then
  echo "release head is not available as a commit: $release_head" >&2
  exit 1
fi
if ! git merge-base --is-ancestor "$target" "$release_head"; then
  echo "release request target $target is not an ancestor of $release_head" >&2
  exit 1
fi

workspace_version="$(
  git show "$target:Cargo.toml" |
    awk '
      /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
      /^\[/ { in_workspace_package = 0 }
      in_workspace_package && /^version[[:space:]]*=/ {
        value = $0
        sub(/^[^"]*"/, "", value)
        sub(/".*$/, "", value)
        print value
        exit
      }
    '
)"
if [[ -z "$workspace_version" ]]; then
  echo "could not resolve [workspace.package] version at $target" >&2
  exit 1
fi
if [[ "$version" != "$workspace_version" ]]; then
  echo "release request version $version does not match Cargo version $workspace_version at $target" >&2
  exit 1
fi

jq -cn \
  --arg schema "tidas.release-context.v1" \
  --arg version "$version" \
  --arg tag "$tag" \
  --arg target "$target" \
  '{schema: $schema, version: $version, tag: $tag, target: $target}'
