#!/bin/sh
set -eu
umask 022
project=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cargo build --manifest-path "$project/Cargo.toml" --target-dir "$project/target" --release --locked
binary="$project/target/release/ssh-manager"
version=$("$binary" --version | cut -d ' ' -f 3)
arch=$(dpkg --print-architecture)
glibc=$(LC_ALL=C readelf --version-info "$binary" | sed -n 's/.*Name: GLIBC_\([0-9.]*\) .*/\1/p' | sort -V | tail -1)
test -n "$glibc"
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT
chmod 755 "$stage"
install -d "$stage/DEBIAN" "$stage/usr/bin" "$stage/usr/share/applications" "$project/dist"
sed -e "s/@VERSION@/$version/g" -e "s/@ARCH@/$arch/g" -e "s/@GLIBC@/$glibc/g" "$project/packaging/control" > "$stage/DEBIAN/control"
install -m 644 "$project/packaging/org.sshmanager.Desktop.desktop" "$stage/usr/share/applications"
for image in "$project"/icons/hicolor/*/apps/org.sshmanager.Desktop.png; do
    install -Dm 644 "$image" "$stage/usr/share/${image#"$project"/}"
done
install -m 755 "$binary" "$stage/usr/bin/ssh-manager"
dpkg-deb --root-owner-group --build "$stage" "$project/dist/"
