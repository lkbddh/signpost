# Signpost

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Latest release](https://img.shields.io/github/v/release/lkbddh/signpost?sort=semver)](https://github.com/lkbddh/signpost/releases/latest)
[![Built for COSMIC](https://img.shields.io/badge/built%20for-COSMIC-6d28d9)](https://github.com/pop-os/cosmic-epoch)

![Signpost, the link chooser for COSMIC](.github/cover.png)

A link chooser for the COSMIC™ desktop. Make Signpost your default handler for web links, and every link you open from
any app shows a small picker of your browsers, each browser profile, and the other apps that can open it. Pick one and
the link opens there.

- Every profile of Chrome, Chromium, Brave, Edge, Vivaldi, Helium, Firefox, LibreWolf, Zen and Floorp as a tile of its own.
- Keyboard first: arrows and Enter, number keys for the first nine tiles, Ctrl+Enter to keep the picker open, Ctrl+C to
  copy the link, Esc to close it.
- A tile's menu (right-click, Menu key, or touch and hold) opens a private window or one of the app's own actions.
- Links that arrive while the picker is open wait their turn, and a link sent twice at once opens once.
- Signpost stays running, so the picker appears at once. It follows your COSMIC theme, frosted windows included.
- It never changes your default browser until you press **Use Signpost**, and **Restore defaults** puts back the one
  you had.

## Install

Download the latest release from the [Releases page](https://github.com/lkbddh/signpost/releases) and check it against
`SHA256SUMS`:

```sh
sha256sum --check --ignore-missing SHA256SUMS
```

**Pop!_OS, Ubuntu and other Debian-based systems:**

```sh
sudo apt install ./signpost_0.1.0_amd64.deb
```

**Flatpak,** on any distribution. Its runtime comes from Flathub:

```sh
flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user ./signpost-0.1.0.flatpak
```

Install one or the other, not both. Each keeps its own record of the browser it replaced, so press **Restore defaults**
in the one you set up before you switch to the other or uninstall it.

## First use

1. Open **Signpost** from the app library. Its settings window opens.
2. Press **Use Signpost**. Signpost becomes the default handler for `http` and `https` links in your user
   `mimeapps.list`, and records the previous ones so they can be restored.
3. Open a link in any app. The picker appears.

To go back, open the same window and press **Restore defaults**. It puts back the `http` and `https` defaults Signpost
recorded. Restore before you uninstall, because uninstalling removes the window that has the button.

## The Flatpak

Signpost has to see your whole desktop to do its job, so its Flatpak is not a tight sandbox. It can:

- **read all your files,** so the picker shows the apps, default apps and browser profiles you have, including the
  profiles of these browsers installed as Flatpaks: Chrome, Chromium, Brave, Brave Origin, Vivaldi, Edge, Firefox,
  LibreWolf, Zen and Floorp;
- **write only to `~/.config`** (besides its own data), where **Use Signpost** and **Restore defaults** edit your
  `mimeapps.list`;
- **use the session bus,** to start when a link arrives and to open apps that start that way;
- **start apps outside the sandbox,** each in a systemd scope of its own when your system makes them, so an app you
  open keeps running after Signpost stops.

Some limits:

- A `mimeapps.list` that links into a folder outside `~/.config` cannot be changed until you let the Flatpak write
  there (see [Troubleshooting](#troubleshooting)).
- Signpost notices an app that fails within 3 seconds of starting. In the Flatpak, those 3 seconds include starting it
  outside the sandbox.
- An app whose icon comes only from a Flatpak installed for all users, or only from `~/.local/share/icons`, shows the
  generic app icon.

## Build from source

You need Rust 1.93 or newer, [`just`](https://github.com/casey/just), and the Wayland and XKB development packages
(`libwayland-dev`, `libxkbcommon-dev` and `pkg-config` on Pop!_OS and Ubuntu).

```sh
just build              # release build, as yourself
sudo just install       # copies the files under /usr; never runs cargo
sudo just uninstall     # removes everything install put in place
```

`just install` only copies files, so root never needs your Rust toolchain. It refuses to install a binary that was not
built from the checkout's current commit, or a checkout with uncommitted changes. To see what it installs without
touching your system:

```sh
just build
stage=$(mktemp -d)
just DESTDIR="$stage" prefix=/usr install
find "$stage" -type f
rm -rf "$stage"
```

`just deb` packages the build as `target/deb/signpost_<version>_<arch>.deb`, and needs `dpkg-dev`. Like `just install`,
it refuses a binary that was not built from the checkout's current commit. `just flatpak` builds
`target/flatpak/signpost-<version>.flatpak` with `flatpak-builder`, and needs the Freedesktop 26.08 SDK and its Rust
extension:

```sh
flatpak install --user flathub org.freedesktop.Sdk//26.08 org.freedesktop.Sdk.Extension.rust-stable//26.08
```

The Flatpak builds offline from `cargo-sources.json`. After changing `Cargo.lock`, regenerate it with
[`flatpak-cargo-generator.py`](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo):
`python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json`. `just validate` runs
formatting, pedantic Clippy, the tests, the desktop and AppStream checks, the staged install and the package, and also
needs `desktop-file-utils`, `appstream` and `dbus`.

## Troubleshooting

**The picker never appears and the log says "WAYLAND_DISPLAY is not set".** D-Bus started Signpost without your
session's display. Run `dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP` (COSMIC
normally does this at login), then open a link again.

**Signpost still behaves like the old version after an update.** It stays running between links. Stop it with
`kill -TERM $(busctl --user status com.lkbddh.signpost | sed -n 's/^PID=//p')`; the next link starts the new one.

**The Flatpak cannot set itself as default, and says it cannot write your `mimeapps.list`.** Your
`~/.config/mimeapps.list` is a link to a file in a folder the Flatpak can read but not write, such as a dotfiles
checkout. The failed attempt changed nothing. Let the Flatpak write to that folder, then press **Use Signpost** again:

```sh
flatpak override --user --filesystem=/path/to/that/folder com.lkbddh.signpost
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Report security problems as described in [SECURITY.md](SECURITY.md).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE). Third-party notices are in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and
their license texts in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

COSMIC is a trademark of System76, Inc. Signpost is an independent project, not affiliated with or endorsed by System76.
