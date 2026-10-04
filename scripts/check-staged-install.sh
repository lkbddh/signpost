#!/usr/bin/env bash
# Staged install must produce exactly the expected file layout, and runtime paths must not contain DESTDIR.
# `just install` only copies files, so build first. Uninstall must remove everything, and a failed install must write nothing.
set -euo pipefail
stage=$(mktemp -d)
failed=$(mktemp -d)
trap 'rm -rf "$stage" "$failed"' EXIT
just build >/dev/null
just -q DESTDIR="$stage" prefix=/usr install
expect=(
  usr/bin/signpost
  usr/share/applications/com.lkbddh.signpost.desktop
  usr/share/metainfo/com.lkbddh.signpost.metainfo.xml
  usr/share/dbus-1/services/com.lkbddh.signpost.service
  usr/share/icons/hicolor/scalable/apps/com.lkbddh.signpost.svg
  usr/share/icons/Cosmic/scalable/apps/com.lkbddh.signpost.svg
  usr/share/icons/hicolor/symbolic/apps/com.lkbddh.signpost-symbolic.svg
  usr/share/icons/Cosmic/scalable/apps/com.lkbddh.signpost-symbolic.svg
  usr/share/doc/signpost/LICENSE
  usr/share/doc/signpost/THIRD_PARTY_NOTICES.md
  usr/share/doc/signpost/THIRD_PARTY_LICENSES.md
)
files() { find "$1" \( -type f -o -type l \) -printf '%P\n' | sort; }
[ -z "$(find "$stage" -name '*.portal')" ] || { echo "the link chooser must not install a .portal file"; exit 1; }
diff <(printf '%s\n' "${expect[@]}" | sort) <(files "$stage") || { echo "installed manifest differs from the expected layout (< expected, > installed)"; exit 1; }
grep -qx 'Exec=/usr/bin/signpost --service' "$stage/usr/share/dbus-1/services/com.lkbddh.signpost.service" || { echo "bad service Exec"; exit 1; }
! grep -rq "$stage" "$stage/usr/share" || { echo "DESTDIR leaked into installed files"; exit 1; }
desktop-file-validate "$stage/usr/share/applications/com.lkbddh.signpost.desktop"
appstreamcli validate --no-net "$stage/usr/share/metainfo/com.lkbddh.signpost.metainfo.xml" >/dev/null
just -q DESTDIR="$stage" prefix=/usr uninstall
[ -z "$(files "$stage")" ] || { echo "uninstall left files behind:"; files "$stage"; exit 1; }
# The .deb holds exactly what the install puts in place, owned by root, under a control file naming its version.
version=$(cargo pkgid | sed 's/.*[#@]//')
deb="target/deb/signpost_${version}_$(dpkg --print-architecture).deb"
rm -f "$deb"
# This runs before a commit too, when the checkout has changes: the package's own check is tested below.
just guard=no deb >/dev/null
[ -f "$deb" ] || { echo "just deb did not build $deb"; exit 1; }
diff <(printf '%s\n' "${expect[@]}" | sort) <(dpkg-deb --fsys-tarfile "$deb" | tar -t | grep -v '/$' | sed 's#^\./##' | sort) \
  || { echo "the .deb's files differ from the install (< expected, > packaged)"; exit 1; }
[ -z "$(dpkg-deb --fsys-tarfile "$deb" | tar -tv | awk '$2 != "root/root"')" ] || { echo "the .deb has files not owned by root"; exit 1; }
for field in "Package: signpost" "Version: $version" "Maintainer: lkbddh <204492431+lkbddh@users.noreply.github.com>"; do
  dpkg-deb --field "$deb" | grep -qxF "$field" || { echo "the .deb's control lacks '$field'"; exit 1; }
done
dpkg-deb --field "$deb" Depends | grep -q 'libc6' || { echo "the .deb does not depend on the libraries it links"; exit 1; }
# A missing service template or binary must fail the install, for that reason, without writing anything.
if out=$(just DESTDIR="$failed" prefix=/usr service_in=/nonexistent/service.in install 2>&1); then
  echo "install succeeded without a service template"; exit 1
fi
grep -q "/nonexistent/service.in" <<<"$out" || { echo "template failure not reported:"; echo "$out"; exit 1; }
[ -z "$(files "$failed")" ] || { echo "a failed install wrote files:"; files "$failed"; exit 1; }
if out=$(just DESTDIR="$failed" prefix=/usr bin=/nonexistent/signpost install 2>&1); then
  echo "install succeeded without a built binary"; exit 1
fi
grep -q "just build" <<<"$out" || { echo "missing-binary error must tell the user to run 'just build':"; echo "$out"; exit 1; }
[ -z "$(files "$failed")" ] || { echo "a failed install wrote files:"; files "$failed"; exit 1; }
# A checked install (a real one, or guard=yes) installs only the checkout's own build: each case runs in a
# throwaway repository holding what the recipe installs, so it holds whatever state this checkout is in.
fixture=$(mktemp -d)
trap 'rm -rf "$stage" "$failed" "$fixture"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
repo="$fixture/repo"
mkdir -p "$repo/data/icons"
cp justfile LICENSE THIRD_PARTY_NOTICES.md THIRD_PARTY_LICENSES.md "$repo/"
cp data/com.lkbddh.signpost.desktop data/com.lkbddh.signpost.metainfo.xml data/com.lkbddh.signpost.service.in "$repo/data/"
cp data/icons/com.lkbddh.signpost.svg data/icons/com.lkbddh.signpost-symbolic.svg "$repo/data/icons/"
git -C "$repo" init --quiet
git -C "$repo" add -A
git -C "$repo" -c user.name=fixture -c user.email=fixture@example.invalid commit --quiet -m fixture
built_from() { printf '#!/bin/sh\necho "signpost 0.1.0 (%s)"\n' "$1" > "$fixture/$2"; chmod +x "$fixture/$2"; }
built_from 000000000000 other
built_from "$(git -C "$repo" rev-parse --short=12 HEAD)" head
checked() { (cd "$1" && just DESTDIR="$fixture/out" prefix=/usr guard=yes bin="$fixture/$2" install 2>&1); }
refused() {
  if out=$(checked "$1" "$2"); then echo "$3: installed"; exit 1; fi
  grep -q "$4" <<<"$out" || { echo "$3: refused, but not because $4:"; echo "$out"; exit 1; }
  [ ! -e "$fixture/out" ] || { echo "$3: a refused install wrote files"; exit 1; }
}
refused "$repo" other "a binary built from another commit" "was built from 000000000000"
checked "$repo" head >/dev/null || { echo "the checkout's own build was refused"; exit 1; }
[ -x "$fixture/out/usr/bin/signpost" ] || { echo "the checkout's own build was not installed"; exit 1; }
rm -rf "$fixture/out"
# A package is what a release ships: `just deb` checks its binary as a real install does, and one it refuses leaves no
# older package behind to be installed instead. `cargo` only names the version here.
mkdir "$fixture/bin"
printf '#!/bin/sh\necho "path+file:///fixture#signpost@0.1.0"\n' > "$fixture/bin/cargo"
chmod +x "$fixture/bin/cargo"
package="$repo/target/deb/signpost_0.1.0_$(dpkg --print-architecture).deb"
packaged() { (cd "$repo" && PATH="$fixture/bin:$PATH" just bin="$fixture/$1" deb 2>&1); }
unpackaged() {
  mkdir -p "$(dirname "$package")" && echo "an older package" > "$package"
  if out=$(packaged "$1"); then echo "$2: packaged"; exit 1; fi
  grep -q "$3" <<<"$out" || { echo "$2: refused, but not because $3:"; echo "$out"; exit 1; }
  [ ! -e "$package" ] || { echo "$2: an older package was left to be installed"; exit 1; }
}
unpackaged other "a package of a binary built from another commit" "was built from 000000000000"
unpackaged missing "a package with no binary" "just build"
out=$(packaged head) || { echo "the checkout's own build was not packaged:"; echo "$out"; exit 1; }
[ "$(dpkg-deb --field "$package" Version)" = 0.1.0 ] || { echo "the checkout's own build was not packaged"; exit 1; }
echo "an edit" >> "$repo/LICENSE"
refused "$repo" head "a checkout with uncommitted changes" "uncommitted changes"
unpackaged head "a package of a checkout with uncommitted changes" "uncommitted changes"
rm -rf "$repo/.git"
out=$(checked "$repo" other) || { echo "outside git the install failed:"; echo "$out"; exit 1; }
grep -q "not a git checkout" <<<"$out" || { echo "outside git the unchecked commit went unsaid:"; echo "$out"; exit 1; }
echo "staged install OK"
