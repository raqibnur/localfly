#!/usr/bin/env sh
# Localfly Linux - one-liner install (no root needed).
#
#   curl -fsSL https://raw.githubusercontent.com/9M2PJU/localfly-linux/main/install.sh | sh
#
# Installs the latest release to ~/.local/bin + ~/.local/share, or with
# --system to /usr/local (needs sudo). Use --uninstall to remove.
set -eu

# Run a command with sudo only when needed (PREFIX not writable).
as_root() {
  if [ "$(id -u)" -eq 0 ] || [ -w "$PREFIX" ]; then
    "$@"
  else
    sudo "$@"
  fi
}

REPO="9M2PJU/localfly-linux"
PREFIX="${HOME}/.local"
SYSTEM=0
UNINSTALL=0

for arg in "$@"; do
  case "$arg" in
    --system) SYSTEM=1 ;;
    --uninstall|--remove) UNINSTALL=1 ;;
    --prefix=*) PREFIX="${arg#--prefix=}" ;;
    -h|--help)
      echo "Usage: install.sh [--system] [--uninstall] [--prefix=DIR]"
      exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

if [ "$SYSTEM" -eq 1 ]; then PREFIX="/usr/local"; fi
BIN="$PREFIX/bin"
SHARE="$PREFIX/share"

uninstall() {
  rm -f "$BIN/localfly" "$BIN/localfly-gui"
  rm -rf "$SHARE/localfly"
  rm -f "$SHARE/applications/com.localfly.Localfly.desktop" "$SHARE/icons/hicolor/256x256/apps/localfly.png"
  echo "Localfly Linux removed from $PREFIX."
}

if [ "$UNINSTALL" -eq 1 ]; then uninstall; exit 0; fi

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
  aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
  *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

need() { command -v "$1" >/dev/null 2>&1 || { echo "Missing required tool: $1" >&2; exit 1; }; }
need curl; need tar

TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep -oE '"tag_name": *"[^"]+"' | head -n 1 | cut -d'"' -f4)"
[ -n "$TAG" ] || { echo "Could not find latest release." >&2; exit 1; }
echo "Installing localfly $TAG for $TARGET to $PREFIX ..."

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT INT TERM
mkdir -p "$PREFIX" 2>/dev/null || true
curl -fsSL "https://github.com/${REPO}/releases/download/${TAG}/localfly-${TAG}-${TARGET}.tar.gz" -o "$TMP/pkg.tar.gz"
tar -xzf "$TMP/pkg.tar.gz" -C "$TMP"
PKGDIR=$(echo "$TMP"/localfly-*)

as_root install -Dm755 "$PKGDIR"/localfly "$BIN/localfly"

# GUI (needs system python3-gi + gtk4 + libadwaita; see README).
# Fetched version-pinned from this release tag so CLI and GUI match.
echo "Installing GUI ..."
for f in gui/localfly_backend.py gui/localfly_gui.py gui/sni.py; do
  curl -fsSL "https://raw.githubusercontent.com/${REPO}/${TAG}/$f" -o "$TMP/g.py" \
    || { echo "Could not fetch $f." >&2; exit 1; }
  # shellcheck disable=SC2086
  as_root install -Dm644 "$TMP/g.py" "$SHARE/localfly/$f"
done
printf '#!/usr/bin/env sh\nexec python3 "%s/localfly/gui/localfly_gui.py" "$@"\n' "$SHARE" > "$TMP/localfly-gui"
as_root install -Dm755 "$TMP/localfly-gui" "$BIN/localfly-gui"
curl -fsSL "https://raw.githubusercontent.com/${REPO}/${TAG}/com.localfly.Localfly.desktop" -o "$TMP/desktop" \
  && as_root install -Dm644 "$TMP/desktop" "$SHARE/applications/com.localfly.Localfly.desktop" || true
curl -fsSL "https://raw.githubusercontent.com/${REPO}/${TAG}/packaging/icons/localfly.png" -o "$TMP/icon.png" 2>/dev/null \
  && as_root install -Dm644 "$TMP/icon.png" "$SHARE/icons/hicolor/256x256/apps/localfly.png" || true
echo "GUI installed (requires: python3-gi gir1.2-gtk-4.0 gir1.2-adw-1)."

echo "Done. Make sure $BIN is on your PATH, then run: localfly --help"
