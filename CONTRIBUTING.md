# Contributing to Signpost

Thanks for helping. Signpost is a small link chooser for the COSMIC™ desktop: it shows a picker when a link opens and
gets out of the way. It takes changes that keep it small, fast and predictable.

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md). Report security problems privately as described
in [SECURITY.md](SECURITY.md), not in an issue.

## Reporting a bug

Open an issue with:

- the Signpost version (`signpost --version`, or About in its settings window) and how you installed it: `.deb`,
  Flatpak, or from source;
- your COSMIC and distribution versions;
- what you did, what you expected, and what happened instead;
- for wrong behaviour, a debug log. Signpost stays running between links, so stop it first:
  `kill -TERM $(busctl --user status com.lkbddh.signpost | sed -n 's/^PID=//p')`. Then start it from a terminal with
  `RUST_LOG=signpost=debug signpost --service` (Flatpak:
  `flatpak run --env=RUST_LOG=signpost=debug com.lkbddh.signpost --service`) and open a link to reproduce the problem.

## Suggesting a feature

Open an issue first and describe the problem rather than the solution. Signpost has no accounts, telemetry or network
features, and it never changes your default browser without you pressing a button; changes that alter that will not be
merged.

## Setting up

You need Rust 1.93 or newer, [`just`](https://github.com/casey/just), and the build packages listed under
[Build from source](README.md#build-from-source). Running it needs a COSMIC session on Wayland.

```bash
git clone https://github.com/lkbddh/signpost.git
cd signpost
just run https://example.org   # debug build with debug logs
just validate                  # what every pull request must pass
```

## Pull requests

`main` is protected: every change lands through a pull request.

1. Fork, branch from `main`, and keep one change per pull request.
2. Run `just validate`. It checks formatting, pedantic Clippy, the tests, the desktop and AppStream metadata, the
   staged install and the `.deb`, and needs `desktop-file-utils`, `appstream`, `dbus` and `dpkg-dev` installed. CI runs
   the same command.
3. Tests never touch your real `~/.config`, `~/.local` or `~/.cache`: the ones that need a home run in a scratch one.
   Keep it that way.
4. For changes to the picker, the settings window, launching or setup, try them in a COSMIC session and attach a
   screenshot for anything visible.
5. In the description, say what was wrong, what you changed, and how you checked it.

Some files need extra steps:

- **Dependencies.** When `Cargo.lock` changes, regenerate `cargo-sources.json` with
  [`flatpak-cargo-generator.py`](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo) and the notices with
  `scripts/gen-third-party-notices.py`. Avoid new dependencies unless the change cannot be done without one.
- **Text.** User-facing strings live in `i18n/en/signpost.ftl`. To add a translation, copy that file to
  `i18n/<language>/signpost.ftl` and translate the values.
- **Icons.** UI icons come from [Phosphor Icons](https://phosphoricons.com/). Credit any new icon source in
  `THIRD_PARTY_NOTICES.md`.
- **`vendor/iced_winit`** is a patched copy of libcosmic's `iced/winit`. Keep changes there small, say why in
  `THIRD_PARTY_NOTICES.md`, and cover them with a test in that crate.

## AI-assisted contributions

You may use AI tools. The person who opens the pull request is responsible for it: you have read every line, run
`just validate` yourself, and can answer review questions without asking the tool. Pull requests opened by an agent with
no person behind them will be closed.

## License

Signpost is licensed under [GPL-3.0-or-later](LICENSE). By contributing, you agree that your contribution is licensed
the same way.
