#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

failures=0

forbidden_files="$(
  git ls-files \
    '*.py' \
    'pyproject.toml' \
    'uv.lock' \
    '.python-version' \
    'src/tidas_tools/**' \
    '.github/workflows/ci.yml' \
    '.github/workflows/python-package-deploy.yml'
)"
if [[ -n "$forbidden_files" ]]; then
  echo "error: active Python-era files remain tracked:" >&2
  printf '%s\n' "$forbidden_files" >&2
  failures=1
fi

active_files=()
while IFS= read -r -d '' path; do
  if [[ "$path" == "scripts/audit-rust-only.sh" ]]; then
    continue
  fi
  active_files+=("$path")
done < <(
  git ls-files -z \
    AGENTS.md \
    README.md \
    README_CN.md \
    .docpact \
    .github \
    .githooks \
    docs \
    scripts
)

forbidden_guidance='src/tidas_tools|pyproject\.toml|python-package-deploy|(^|[[:space:]])uv run([[:space:]]|$)|(^|[[:space:]])pip3? install([[:space:]]|$)|(^|[[:space:]])pytest([[:space:]]|$)|(^|[[:space:]])python(3|[0-9.]*)?([[:space:]]+-m|[[:space:]]+[^[:space:]]+\.py|[[:space:]]+scripts/)'
if (( ${#active_files[@]} > 0 )); then
  matches="$(grep -EnI "$forbidden_guidance" "${active_files[@]}" || true)"
  if [[ -n "$matches" ]]; then
    echo "error: active guidance or automation contains a removed implementation/install/invocation path:" >&2
    printf '%s\n' "$matches" >&2
    failures=1
  fi
fi

all_files=()
while IFS= read -r -d '' path; do
  if [[ -f "$path" && "$path" != "scripts/audit-rust-only.sh" ]]; then
    all_files+=("$path")
  fi
done < <(git ls-files -z --cached --others --exclude-standard)

runtime_invocation='(^#!.*python)|(^|[[:space:]])uv run([[:space:]]|$)|(^|[[:space:]])pip3? install([[:space:]]|$)|(^|[[:space:]])pytest([[:space:]]|$)|(^|[[:space:]])python(3|[0-9.]*)?([[:space:]]+(-[cm]|[^[:space:]]+\.py|scripts/|src/)|[[:space:]]+-([[:space:]]|$))|Command::new\(["'\'']python'
if (( ${#all_files[@]} > 0 )); then
  matches="$(grep -EnI "$runtime_invocation" "${all_files[@]}" || true)"
  matches="$(
    printf '%s\n' "$matches" |
      grep -v '^migration/python-to-rust-owners.md:' || true
  )"
  if [[ -n "$matches" ]]; then
    echo "error: tracked source contains an executable Python install or invocation path:" >&2
    printf '%s\n' "$matches" >&2
    failures=1
  fi
fi

allowlist="migration/rust-only-audit-allowlist.txt"
if [[ ! -f "$allowlist" ]]; then
  echo "error: missing reviewed terminology allowlist: $allowlist" >&2
  failures=1
else
  mention_files=()
  while IFS= read -r -d '' path; do
    if [[ "$path" != "scripts/audit-rust-only.sh" &&
      "$path" != "$allowlist" ]] &&
      grep -Eqi 'python' "$path"; then
      mention_files+=("$path")
    fi
  done < <(git ls-files -z --cached --others --exclude-standard)

  for path in "${mention_files[@]}"; do
    allowed=false
    while IFS= read -r pattern; do
      if [[ -z "$pattern" || "$pattern" == \#* ]]; then
        continue
      fi
      if [[ "$path" == $pattern ]]; then
        allowed=true
        break
      fi
    done < "$allowlist"
    if [[ "$allowed" != "true" ]]; then
      echo "error: unreviewed Python-era terminology remains in $path" >&2
      failures=1
    fi
  done
fi

if (( failures != 0 )); then
  exit 1
fi

echo "Rust-only audit passed: no active Python source, packaging, CI, or invocation path is tracked."
