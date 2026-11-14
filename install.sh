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
  rm -f "$BIN/localfly" "$BIN/localflyd" "$BIN/localfly-gui"
  rm -rf "$SHARE/localfly"
  rm -f "$SHARE/applications/com.localfly.Localfly.desktop" "$SHARE/icons/hicolor/256x256/apps/localfly.png" "$SHARE/icons/hicolor/256x256/apps/localfly-live.png"
  rm -f "$SHARE/metainfo/com.localfly.Localfly.metainfo.xml"
  rm -f "$SHARE/bash-completion/completions/localfly" "$SHARE/bash-completion/completions/localflyd"
  rm -f "$SHARE/zsh/site-functions/_localfly" "$SHARE/zsh/site-functions/_localflyd"
  rm -f "$HOME/.config/fish/completions/localfly.fish" "$HOME/.config/fish/completions/localflyd.fish"
  rm -f "$SHARE/man/man1/localfly.1" "$SHARE/man/man1/localflyd.1"
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
as_root install -Dm755 "$PKGDIR"/localflyd "$BIN/localflyd"

# GUI (needs system python3-gi + gtk4 + libadwaita; see README).
# Prefers the release tarball (0.1.1+ bundles the GUI), falls back to
# version-pinned raw files so older tarballs keep working.
echo "Installing GUI ..."
if [ -f "$PKGDIR/gui/localfly_gui.py" ]; then
  as_root install -Dm644 "$PKGDIR/gui/localfly_backend.py" "$SHARE/localfly/gui/localfly_backend.py"
  as_root install -Dm644 "$PKGDIR/gui/localfly_gui.py" "$SHARE/localfly/gui/localfly_gui.py"
  as_root install -Dm644 "$PKGDIR/gui/sni.py" "$SHARE/localfly/gui/sni.py"
else
  for f in gui/localfly_backend.py gui/localfly_gui.py gui/sni.py; do
    curl -fsSL "https://raw.githubusercontent.com/${REPO}/${TAG}/$f" -o "$TMP/g.py" \
      || { echo "Could not fetch $f." >&2; exit 1; }
    as_root install -Dm644 "$TMP/g.py" "$SHARE/localfly/$f"
  done
fi
printf '#!/usr/bin/env sh\nexec python3 "%s/localfly/gui/localfly_gui.py" "$@"\n' "$SHARE" > "$TMP/localfly-gui"
as_root install -Dm755 "$TMP/localfly-gui" "$BIN/localfly-gui"
if [ -f "$PKGDIR/com.localfly.Localfly.desktop" ]; then
  as_root install -Dm644 "$PKGDIR/com.localfly.Localfly.desktop" "$SHARE/applications/com.localfly.Localfly.desktop"
else
  curl -fsSL "https://raw.githubusercontent.com/${REPO}/${TAG}/com.localfly.Localfly.desktop" -o "$TMP/desktop" \
    && as_root install -Dm644 "$TMP/desktop" "$SHARE/applications/com.localfly.Localfly.desktop" || true
fi
for _icon in localfly localfly-live; do
  if [ -f "$PKGDIR/${_icon}.png" ]; then
    as_root install -Dm644 "$PKGDIR/${_icon}.png" "$SHARE/icons/hicolor/256x256/apps/${_icon}.png"
  else
    curl -fsSL "https://raw.githubusercontent.com/${REPO}/${TAG}/packaging/icons/${_icon}.png" -o "$TMP/icon.png" 2>/dev/null \
      && as_root install -Dm644 "$TMP/icon.png" "$SHARE/icons/hicolor/256x256/apps/${_icon}.png" || true
  fi
done
if [ -f "$PKGDIR/com.localfly.Localfly.metainfo.xml" ]; then
  as_root install -Dm644 "$PKGDIR/com.localfly.Localfly.metainfo.xml" "$SHARE/metainfo/com.localfly.Localfly.metainfo.xml"
fi
if [ -d "$PKGDIR/completions" ]; then
  as_root install -Dm644 "$PKGDIR/completions/localfly.bash" "$SHARE/bash-completion/completions/localfly" || true
  as_root install -Dm644 "$PKGDIR/completions/localflyd.bash" "$SHARE/bash-completion/completions/localflyd" || true
  as_root install -Dm644 "$PKGDIR/completions/_localfly" "$SHARE/zsh/site-functions/_localfly" || true
  as_root install -Dm644 "$PKGDIR/completions/_localflyd" "$SHARE/zsh/site-functions/_localflyd" || true
  as_root install -Dm644 "$PKGDIR/completions/localfly.fish" "$HOME/.config/fish/completions/localfly.fish" || true
  as_root install -Dm644 "$PKGDIR/completions/localflyd.fish" "$HOME/.config/fish/completions/localflyd.fish" || true
fi
if [ -d "$PKGDIR/man" ]; then
  as_root install -Dm644 "$PKGDIR/man/localfly.1" "$SHARE/man/man1/localfly.1" || true
  as_root install -Dm644 "$PKGDIR/man/localflyd.1" "$SHARE/man/man1/localflyd.1" || true
fi
echo "GUI installed (requires: python3-gi gir1.2-gtk-4.0 gir1.2-adw-1)."

echo "Done. Make sure $BIN is on your PATH, then run: localfly --help"
