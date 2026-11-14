#!/usr/bin/env sh
# Bump localfly-linux-bin to a new tag: refresh tarball checksums, rebuild
# .SRCINFO, and print the AUR push commands.
# Usage: packaging/aur/bump-bin.sh vX.Y.Z   (after the GitHub release exists)
set -eu

VER="${1:?usage: bump-bin.sh vX.Y.Z (e.g. v0.1.1)}"
VER_NUM="$(printf '%s' "$VER" | sed 's/^v//')"
DIR="$(dirname "$(readlink -f "$0")")/localfly-linux-bin"
cd "$DIR"

python3 - "$VER" "$VER_NUM" <<'PY'
import hashlib
import pathlib
import re
import subprocess
import sys

tag, ver = sys.argv[1], sys.argv[2]
targets = {
    "x86_64": "x86_64-unknown-linux-gnu",
    "aarch64": "aarch64-unknown-linux-gnu",
}
sums = {}
for arch, target in targets.items():
    url = (
        "https://github.com/9M2PJU/localfly-linux/releases/download/"
        f"{tag}/localfly-{tag}-{target}.tar.gz"
    )
    print(f"Fetching {url} ...", flush=True)
    out = subprocess.run(["curl", "-fsSL", url], capture_output=True, check=True).stdout

    sums[arch] = (url, hashlib.sha256(out).hexdigest())
    print(f"  {sums[arch][1]}")

p = pathlib.Path("PKGBUILD")
s = p.read_text()
for arch, (url, sha) in sums.items():
    fname = url.rsplit("/", 1)[1]
    s = re.sub(
        rf'^source_{arch}=\(.*\)$',
        f'source_{arch}=("{fname}::{url}")',
        s,
        count=1,
        flags=re.M,
    )
    s = re.sub(
        rf"^sha256sums_{arch}=\(.*\)$",
        f"sha256sums_{arch}=('{sha}')",
        s,
        count=1,
        flags=re.M,
    )
s = re.sub(r"^pkgver=.*$", f"pkgver={ver}", s, count=1, flags=re.M)
s = re.sub(r"^pkgrel=.*$", "pkgrel=1", s, count=1, flags=re.M)
p.write_text(s)
print("PKGBUILD updated.")
PY

makepkg --printsrcinfo > .SRCINFO
namcap PKGBUILD || true
echo "---"
echo "Review the diff, then:"
echo "  cd $DIR && git add PKGBUILD .SRCINFO && git commit -m 'Update to $VER' && git push origin master"
