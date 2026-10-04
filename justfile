set positional-arguments

prefix := "/usr"
DESTDIR := ""
bindir := prefix + "/bin"
datadir := prefix + "/share"
docdir := datadir + "/doc/signpost"
bin := "target/release/signpost"
service_in := "data/com.lkbddh.signpost.service.in"
# A real install (no DESTDIR) and a package check that the binary is the checkout's own build; `guard=yes` checks a
# staged install too, and `guard=no` skips the check.
guard := if DESTDIR == "" { "yes" } else { "no" }

default: validate

build:
    cargo build --release --locked

run *ARGS:
    RUST_LOG=warn,signpost=debug cargo run -- "$@"

test:
    cargo test --locked

# Copies files only (never runs cargo, so `sudo just install` needs no toolchain): run `just build` first.
install:
    #!/usr/bin/env bash
    set -euo pipefail
    [ -x "{{bin}}" ] || { echo "{{bin}} not found: run 'just build' first" >&2; exit 1; }
    # Under sudo git distrusts a checkout the user owns, so it is told to trust it.
    git_() { git -c safe.directory='*' "$@"; }
    if [ "{{guard}}" = yes ] && git_ rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        [ -z "$(git_ status --porcelain --untracked-files=no)" ] || { echo "the checkout has uncommitted changes: commit them and run 'just build' before installing" >&2; exit 1; }
        head=$(git_ rev-parse --short=12 HEAD)
        built=$("{{bin}}" --version | sed -n 's/^signpost .* (\(.*\))$/\1/p')
        [ "$built" = "$head" ] || { echo "{{bin}} was built from ${built:-an unknown commit}, not $head: run 'just build' first" >&2; exit 1; }
    elif [ "{{guard}}" = yes ]; then
        echo "not a git checkout: the binary's commit is not checked"
    fi
    service=$(mktemp)
    trap 'rm -f "$service"' EXIT
    sed 's|@bindir@|{{bindir}}|' "{{service_in}}" > "$service"
    install -Dm0755 "{{bin}}" "{{DESTDIR}}{{bindir}}/signpost"
    install -Dm0644 data/com.lkbddh.signpost.desktop "{{DESTDIR}}{{datadir}}/applications/com.lkbddh.signpost.desktop"
    install -Dm0644 data/com.lkbddh.signpost.metainfo.xml "{{DESTDIR}}{{datadir}}/metainfo/com.lkbddh.signpost.metainfo.xml"
    install -Dm0644 "$service" "{{DESTDIR}}{{datadir}}/dbus-1/services/com.lkbddh.signpost.service"
    install -Dm0644 data/icons/com.lkbddh.signpost.svg "{{DESTDIR}}{{datadir}}/icons/hicolor/scalable/apps/com.lkbddh.signpost.svg"
    install -Dm0644 data/icons/com.lkbddh.signpost.svg "{{DESTDIR}}{{datadir}}/icons/Cosmic/scalable/apps/com.lkbddh.signpost.svg"
    install -Dm0644 data/icons/com.lkbddh.signpost-symbolic.svg "{{DESTDIR}}{{datadir}}/icons/hicolor/symbolic/apps/com.lkbddh.signpost-symbolic.svg"
    install -Dm0644 data/icons/com.lkbddh.signpost-symbolic.svg "{{DESTDIR}}{{datadir}}/icons/Cosmic/scalable/apps/com.lkbddh.signpost-symbolic.svg"
    install -Dm0644 LICENSE "{{DESTDIR}}{{docdir}}/LICENSE"
    install -Dm0644 THIRD_PARTY_NOTICES.md "{{DESTDIR}}{{docdir}}/THIRD_PARTY_NOTICES.md"
    install -Dm0644 THIRD_PARTY_LICENSES.md "{{DESTDIR}}{{docdir}}/THIRD_PARTY_LICENSES.md"
    [ -z "{{DESTDIR}}" ] || exit 0
    # The daemon keeps running the replaced binary until it is stopped; it runs on the user's own bus.
    as_user=()
    if [ -n "${SUDO_USER:-}" ]; then
        uid=$(id -u "$SUDO_USER")
        as_user=(sudo -u "$SUDO_USER" env XDG_RUNTIME_DIR="/run/user/$uid" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus")
    fi
    if ! names=$("${as_user[@]}" busctl --user list --no-legend --no-pager 2>/dev/null); then
        echo "If Signpost is running, it still runs the old build. Stop it with:"
        echo "  kill -TERM \$(busctl --user status com.lkbddh.signpost | sed -n 's/^PID=//p')"
        exit 0
    fi
    pid=$(awk '$1 == "com.lkbddh.signpost" && $2 ~ /^[0-9]+$/ { print $2 }' <<<"$names")
    [ -z "$pid" ] || echo "Signpost $pid still runs the old build. Stop it with: kill -TERM $pid"

uninstall:
    rm -f "{{DESTDIR}}{{bindir}}/signpost"
    rm -f "{{DESTDIR}}{{datadir}}/applications/com.lkbddh.signpost.desktop"
    rm -f "{{DESTDIR}}{{datadir}}/metainfo/com.lkbddh.signpost.metainfo.xml"
    rm -f "{{DESTDIR}}{{datadir}}/dbus-1/services/com.lkbddh.signpost.service"
    rm -f "{{DESTDIR}}{{datadir}}/icons/hicolor/scalable/apps/com.lkbddh.signpost.svg"
    rm -f "{{DESTDIR}}{{datadir}}/icons/Cosmic/scalable/apps/com.lkbddh.signpost.svg"
    rm -f "{{DESTDIR}}{{datadir}}/icons/hicolor/symbolic/apps/com.lkbddh.signpost-symbolic.svg"
    rm -f "{{DESTDIR}}{{datadir}}/icons/Cosmic/scalable/apps/com.lkbddh.signpost-symbolic.svg"
    rm -f "{{DESTDIR}}{{docdir}}/LICENSE"
    rm -f "{{DESTDIR}}{{docdir}}/THIRD_PARTY_NOTICES.md"
    rm -f "{{DESTDIR}}{{docdir}}/THIRD_PARTY_LICENSES.md"

# Builds a .deb in target/deb from the release binary, which must be the checkout's own build (run `just build` first).
deb:
    #!/usr/bin/env bash
    set -euo pipefail
    version=$(cargo pkgid | sed 's/.*[#@]//')
    arch=$(dpkg --print-architecture)
    pkg="target/deb/signpost_${version}_${arch}"
    # No older package is left to be installed in place of one this refuses.
    rm -rf "$pkg" "$pkg.deb"
    [ -x "{{bin}}" ] || { echo "{{bin}} not found: run 'just build' first" >&2; exit 1; }
    just DESTDIR="$pkg" prefix=/usr guard={{guard}} bin="{{bin}}" install
    # dpkg-shlibdeps only runs next to a debian/control, so it gets a throwaway one.
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    mkdir "$tmp/debian"
    printf 'Source: signpost\n\nPackage: signpost\nArchitecture: any\n' > "$tmp/debian/control"
    shlibs=$(cd "$tmp" && dpkg-shlibdeps -O "$OLDPWD/$pkg/usr/bin/signpost" | sed 's/^shlibs:Depends=//')
    mkdir -p "$pkg/DEBIAN"
    cat > "$pkg/DEBIAN/control" <<EOF
    Package: signpost
    Version: ${version}
    Architecture: ${arch}
    Maintainer: lkbddh <204492431+lkbddh@users.noreply.github.com>
    Installed-Size: $(du -sk "$pkg/usr" | cut -f1)
    Depends: ${shlibs:+$shlibs, }dbus-user-session | default-dbus-session-bus
    Recommends: libvulkan1 | libegl1
    Section: web
    Priority: optional
    Homepage: https://github.com/lkbddh/signpost
    Description: Link chooser for the COSMIC desktop
     Make Signpost your default handler for web links, and every link you open
     shows a small picker of your browsers, each browser profile, and the other
     apps that can open it.
    EOF
    dpkg-deb --root-owner-group --build "$pkg" target/deb/ >/dev/null

# Builds and checks target/flatpak/signpost-<version>.flatpak (needs flatpak-builder, the freedesktop SDK 26.08 and rust-stable).
flatpak:
    #!/usr/bin/env bash
    set -euo pipefail
    version=$(cargo pkgid | sed 's/.*[#@]//')
    out=target/flatpak
    flatpak-builder --force-clean --state-dir="$out/state" --repo="$out/repo" "$out/build" com.lkbddh.signpost.yml
    desktop-file-validate "$out/build/export/share/applications/com.lkbddh.signpost.desktop"
    appstreamcli validate --no-net "$out/build/files/share/metainfo/com.lkbddh.signpost.metainfo.xml"
    flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo "$out/repo" "$out/signpost-$version.flatpak" com.lkbddh.signpost
    echo "$out/signpost-$version.flatpak"

validate:
    cargo fmt --check
    cargo clippy --all-targets --locked -- -D warnings -W clippy::pedantic
    cargo test --locked
    desktop-file-validate data/com.lkbddh.signpost.desktop
    appstreamcli validate --no-net data/com.lkbddh.signpost.metainfo.xml
    bash scripts/check-staged-install.sh
