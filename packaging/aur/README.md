# AUR packages

Two packages, both maintained here and pushed to the AUR with the
`aur` SSH key (`ssh://aur@aur.archlinux.org/<pkg>.git`):

- `localfly-linux-git` - builds from git main (always fresh).
- `localfly-linux-bin` - prebuilt release tarball + GUI files pinned to the tag.

## New release checklist

1. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`
   (GitHub builds the tarballs; `-bin` needs them to exist).
2. `-bin`: bump `pkgver`, refresh `sha256sums` for the two tarballs and the
   six pinned `raw.githubusercontent.com` files, rebuild `.SRCINFO` with
   `makepkg --printsrcinfo`, test with `makepkg`, commit, push to AUR.
3. `-git`: nothing to change (version is dynamic via `git describe`).
   Push to AUR only if the PKGBUILD itself changed.
4. Verify on the AUR site that both packages show the new version.

## Local checks

```bash
cd packaging/aur/localfly-linux-bin
makepkg --printsrcinfo > .SRCINFO
namcap PKGBUILD
makepkg   # full build in a clean checkout copy
```
