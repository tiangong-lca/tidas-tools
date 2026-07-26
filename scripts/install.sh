#!/bin/sh
set -eu

REPOSITORY="${TIDAS_INSTALL_REPOSITORY:-tiangong-lca/tidas-tools}"
PREFIX="${TIDAS_INSTALL_PREFIX:-/usr/local}"
VERSION=""

usage() {
  echo "usage: install.sh --version <VERSION> [--prefix <DIR>]" >&2
  echo "example: install.sh --version 0.1.0 --prefix \"\$HOME/.local\"" >&2
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || {
        usage
        exit 2
      }
      VERSION="${2#v}"
      shift 2
      ;;
    --prefix)
      [ "$#" -ge 2 ] || {
        usage
        exit 2
      }
      PREFIX="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [ -z "$VERSION" ]; then
  echo "error: --version is required; installers never resolve an implicit mutable latest release" >&2
  usage
  exit 2
fi

case "$(uname -s)" in
  Linux) OS="unknown-linux-gnu" ;;
  Darwin) OS="apple-darwin" ;;
  *)
    echo "error: unsupported operating system: $(uname -s)" >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64|amd64) ARCH="x86_64" ;;
  arm64|aarch64) ARCH="aarch64" ;;
  *)
    echo "error: unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

TARGET="${ARCH}-${OS}"
ARCHIVE="tidas-v${VERSION}-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPOSITORY}/releases/download/v${VERSION}"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/tidas-install.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT HUP INT TERM

curl --fail --location --proto '=https' --tlsv1.2 \
  --output "$TEMP_DIR/$ARCHIVE" "$BASE_URL/$ARCHIVE"
curl --fail --location --proto '=https' --tlsv1.2 \
  --output "$TEMP_DIR/$ARCHIVE.sha256" "$BASE_URL/$ARCHIVE.sha256"

EXPECTED="$(awk -v file="$ARCHIVE" '$2 == file { print $1 }' "$TEMP_DIR/$ARCHIVE.sha256")"
if [ -z "$EXPECTED" ]; then
  echo "error: checksum file does not name $ARCHIVE" >&2
  exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL="$(sha256sum "$TEMP_DIR/$ARCHIVE" | awk '{ print $1 }')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL="$(shasum -a 256 "$TEMP_DIR/$ARCHIVE" | awk '{ print $1 }')"
else
  echo "error: sha256sum or shasum is required to verify the release" >&2
  exit 1
fi
if [ "$ACTUAL" != "$EXPECTED" ]; then
  echo "error: SHA-256 mismatch for $ARCHIVE" >&2
  exit 1
fi

tar -xzf "$TEMP_DIR/$ARCHIVE" -C "$TEMP_DIR"
SOURCE="$TEMP_DIR/tidas-v${VERSION}-${TARGET}/bin/tidas"
if [ ! -x "$SOURCE" ]; then
  echo "error: verified archive does not contain bin/tidas" >&2
  exit 1
fi

mkdir -p "$PREFIX/bin"
install -m 0755 "$SOURCE" "$PREFIX/bin/tidas"
"$PREFIX/bin/tidas" --version
echo "installed verified tidas v${VERSION} to $PREFIX/bin/tidas"
