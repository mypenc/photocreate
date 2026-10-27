#!/usr/bin/env bash
# Fetch winit 0.30.13 (the version Cargo.lock pins) into android/vendor/winit and apply the
# Android input patch: mouse hover/buttons/wheel, keyboard modifiers, stable IME visibility, and
# soft-keyboard text/cursor updates.
# The stock Android backend drops all of those (`_ => None // TODO mouse events`).
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
dest="$here/vendor/winit"
version="0.30.13"
if [ -e "$dest" ]; then echo "$dest exists; remove it to re-vendor"; exit 0; fi
mkdir -p "$here/vendor"
rev="a517b3846ee8457d84ee4975110bfda6797c44dc"
# PhotoCraft builds against storytold's winit fork (egui needs its IME-reset events), so the
# Android input patch goes on top of that fork, not on stock crates.io winit.
tmp="$(mktemp -d)"
git clone -q https://github.com/storytold/winit.git "$tmp/winit"
git -C "$tmp/winit" checkout -q "$rev"
rm -rf "$tmp/winit/.git"
mv "$tmp/winit" "$dest"
patch -p1 -d "$dest" < "$here/patches/winit-$version-android-input.patch"
rm -rf "$tmp"
echo "winit $version (storytold fork $rev) patched in $dest"
