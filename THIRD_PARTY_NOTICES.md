# Third-Party Notices

This file records third-party open-source software, embedded fonts, and assets used by Signpost, and how the distributed binary is licensed.

Signpost's own source is GPL-3.0-or-later (`LICENSE`). For source installs, the Rust dependency set is reproducible from `Cargo.lock`.
For binary distribution, ship `LICENSE`, this file, and `THIRD_PARTY_LICENSES.md` (the license, copyright, and NOTICE texts of every dependency and embedded font) together.
`just install` puts all three in `$prefix/share/doc/signpost/`.

## Licensing Of The Distributed Binary

The distributed binary is covered by **GPL-3.0**. libcosmic, which every build links, contains a file marked `GPL-3.0-only` (`src/localize.rs`, see Mixed Licensing below), so the combined work can only be distributed under version 3 of the GPL. Signpost's own source remains GPL-3.0-or-later.

Every other dependency license in the inventory (MIT, Apache-2.0 with LLVM exception, MPL-2.0, BSD-2-Clause, BSD-3-Clause, ISC, 0BSD, Zlib, CC0-1.0, Unicode-3.0, Unlicense, OFL-1.1 for fonts) is compatible with GPL-3.0. MPL-2.0 permits the combination through its section 3.3. Where a package offers a choice, Signpost uses the Apache-2.0 or MIT option; for example `self_cell` is `Apache-2.0 OR GPL-2.0-only` and is used under Apache-2.0.

## Embedded Data And Assets

| Project | Used for | License | Source |
|---|---|---|---|
| Open Sans 1.10 (Light, Regular, Semibold, Bold, ExtraBold) | Fonts that libcosmic embeds in every build (`src/app/mod.rs`) | Apache-2.0, per the font metadata; OFL-1.1, per libcosmic's `res/open-sans/LICENSE` (see the note below) | https://github.com/pop-os/libcosmic/tree/87ab8179e1bd9880239c340855ae8862034bd0e8/res/open-sans |
| Noto Sans Mono (Regular, Bold) | Fonts that libcosmic embeds in every build | OFL-1.1, Copyright 2015 Google LLC | https://github.com/pop-os/libcosmic/tree/87ab8179e1bd9880239c340855ae8862034bd0e8/res/noto |
| `Iced-Icons.ttf` | Icon font that iced_graphics embeds in every build | MIT (iced's `LICENSE`); generated with Fontello, which ships no separate glyph licenses | https://github.com/pop-os/libcosmic/tree/87ab8179e1bd9880239c340855ae8862034bd0e8/iced/graphics/fonts |
| `iced_winit` (vendored, six local fixes) | Patched copy of libcosmic's `iced/winit` in `vendor/iced_winit/`, with its own `LICENSE`; the fixes retire completed runtime futures, drop a surface's accessibility adapter when it closes, expire unanswered activation-token requests, let a popup request a token with the serial of its latest press, grab with the serial of the input that opened a popup, and forget the cursor when the pointer leaves a surface | MIT, with one MPL-2.0 file (see Mixed Licensing) | https://github.com/iced-rs/iced and https://github.com/pop-os/libcosmic/tree/87ab8179e1bd9880239c340855ae8862034bd0e8/iced/winit |
| Phosphor Icons, Fill weight (`app-window`, `arrow-counter-clockwise`, `arrow-square-out`, `caret-right`, `check`, `copy`, `cursor-click`, `detective`, `lock-simple`, `lock-simple-open`, `push-pin`, `stack`, `warning-circle`) | The 13 SVG icons Signpost bundles in `resources/icons/phosphor/` and compiles in; their `LICENSE` is there too, and its text is in `THIRD_PARTY_LICENSES.md` | MIT, Copyright (c) 2023 Phosphor Icons | https://github.com/phosphor-icons/core |

Open Sans: libcosmic's repository carries an OFL-1.1 text (Copyright 2020 The Open Sans Project Authors, https://github.com/googlefonts/opensans), but the five embedded TTFs are the 2010-2011 Google/Ascender version 1.10 builds, and each one states "Licensed under the Apache License, Version 2.0" in its name table. The Apache-2.0 license is the one that describes the embedded files, and it is the one this project relies on. The OFL text is shipped too, because libcosmic ships it with the fonts. The fonts come with no NOTICE file. Both texts are in `THIRD_PARTY_LICENSES.md`.

Not part of the Linux binary: Fira Sans (iced's `fira-sans` feature is off) and libcosmic's `cosmic-icons` (bundled only on non-Unix targets).

## Mixed Licensing

### libcosmic at `87ab8179e1bd9880239c340855ae8862034bd0e8`

The repository `LICENSE` is MPL-2.0 and most files carry `SPDX-License-Identifier: MPL-2.0`. File-level exceptions in the crate Signpost links:

| Files | License |
|---|---|
| `src/localize.rs` | GPL-3.0-only |
| `src/widget/dropdown/mod.rs`, `widget.rs`, `operation.rs`, `menu/mod.rs`, `menu/appearance.rs`, `multi/mod.rs`, `multi/widget.rs` | MPL-2.0 AND MIT (MIT part: Copyright 2019 Héctor Ramón, Iced contributors) |
| `src/widget/button/widget.rs`, `src/widget/text_input/{cursor,editor,input,mod,style,value}.rs`, `src/widget/wayland/tooltip/widget.rs` | MIT |

The MIT text is iced's `LICENSE`, included in `THIRD_PARTY_LICENSES.md`.

### Vendored `iced_winit`

`vendor/iced_winit/` is libcosmic's `iced/winit` at the revision above with six changes (MIT): the one in `src/lib.rs` is described by `Cargo.toml` `local-fix`, the accessibility adapter cleanup and the cursor forgotten when the pointer leaves a surface are in `src/platform_specific/wayland/sctk_event.rs` (with `src/window/state.rs`), and the activation-token expiry, the token request of a popup and the serial a popup grabs with are in `src/platform_specific/wayland/event_loop/state.rs`. `src/platform_specific/wayland/handlers/compositor.rs` is marked `MPL-2.0-only` (not a valid SPDX id, read as MPL-2.0), is unmodified, and is not reflected in the crate's `license = "MIT"`. `vendor/iced_winit/LICENSE` is the MIT notice; the MPL-2.0 text is in `THIRD_PARTY_LICENSES.md`. Its `Cargo.toml` also allows the crate's own compiler warnings, which cargo hides for the crates it downloads.

### MPL-2.0 source availability

MPL-2.0 code (libcosmic and the `cosmic-*` crates, `iced_winit`'s `compositor.rs`, `freedesktop-desktop-entry`, `option-ext`, `cosmic-mime-apps`) is used without modification, apart from the vendored `iced_winit` whose complete source is in this repository under `vendor/iced_winit/`. The Source Code Form of the rest is available at the exact revisions pinned in `Cargo.lock`:

- libcosmic, `cosmic-config`, `cosmic-config-derive`, `cosmic-theme`, `iced_accessibility`, `iced/winit`: https://github.com/pop-os/libcosmic/tree/87ab8179e1bd9880239c340855ae8862034bd0e8
- `cosmic-settings-config`: https://github.com/pop-os/cosmic-settings-daemon/tree/7ec3e3016b68c5e57f27cb18a4b0e6c413443a37
- `cosmic-settings-daemon`: https://github.com/pop-os/dbus-settings-bindings/tree/eed01dd3609e90e3c8cd043656734c500956c793
- `cosmic-mime-apps`: https://github.com/pop-os/cosmic-mime-apps/tree/8fc81e8857e46c018ea21ab985aaf30ac853c007
- `freedesktop-desktop-entry` and `option-ext`: the exact versions below on https://crates.io

## Rust Dependencies

One row per crate in the x86_64 Linux build (`cargo tree --locked --target x86_64-unknown-linux-gnu -e normal,build`: normal and build dependencies, no dev-dependencies), without Signpost itself. Crates that only other platforms compile (macOS, Windows, Android) are not listed. Some upstream packages do not publish a Cargo `license` field; for those the generator uses the license in the upstream repository's LICENSE file or SPDX headers. The source is the package `repository`, else `homepage`, else its Cargo `source`.

`THIRD_PARTY_LICENSES.md` carries the license text of every row. It collects every LICENSE, COPYING, NOTICE, COPYRIGHT and UNLICENSE file in each crate's source. A crate whose source ships none uses the file from its upstream repository at the released revision, saved once in `licenses/overrides/<crate>-<version>.txt` (first line `Source: <url>`), or the standard Apache-2.0, MPL-2.0, CC0-1.0 or BSL-1.0 text when its license is or includes that choice. The generator works offline and fails, listing the crates, if one has no text.

Regenerate this table and `THIRD_PARTY_LICENSES.md` after any `Cargo.lock` change with `python3 scripts/gen-third-party-notices.py`; the script rewrites only the marked block.

<!-- BEGIN GENERATED: rust-dependencies -->
| Package | Version | License | Source |
|---|---:|---|---|
| `ab_glyph` | `0.2.32` | Apache-2.0 | https://github.com/alexheretic/ab-glyph |
| `ab_glyph_rasterizer` | `0.1.10` | Apache-2.0 | https://github.com/alexheretic/ab-glyph |
| `accesskit` | `0.22.0` | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit |
| `accesskit_atspi_common` | `0.15.0` | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit |
| `accesskit_consumer` | `0.32.0` | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit |
| `accesskit_unix` | `0.18.0` | MIT OR Apache-2.0 | https://github.com/AccessKit/accesskit |
| `accesskit_winit` | `0.30.0` | Apache-2.0 | https://github.com/AccessKit/accesskit |
| `adler2` | `2.0.1` | 0BSD OR MIT OR Apache-2.0 | https://github.com/oyvindln/adler2 |
| `ahash` | `0.8.12` | MIT OR Apache-2.0 | https://github.com/tkaitchuck/ahash |
| `aho-corasick` | `1.1.5` | Unlicense OR MIT | https://github.com/BurntSushi/aho-corasick |
| `aliasable` | `0.1.3` | MIT | https://github.com/avitex/rust-aliasable |
| `allocator-api2` | `0.2.21` | MIT OR Apache-2.0 | https://github.com/zakarumych/allocator-api2 |
| `almost` | `0.2.0` | CC0-1.0 | https://github.com/thomcc/almost |
| `apply` | `0.3.0` | Unlicense | https://github.com/burtonageo/apply |
| `approx` | `0.5.1` | Apache-2.0 | https://github.com/brendanzab/approx |
| `arc-swap` | `1.9.2` | MIT OR Apache-2.0 | https://github.com/vorner/arc-swap |
| `arrayref` | `0.3.9` | BSD-2-Clause | https://github.com/droundy/arrayref |
| `arrayvec` | `0.7.8` | MIT OR Apache-2.0 | https://github.com/bluss/arrayvec |
| `as-raw-xcb-connection` | `1.0.1` | MIT OR Apache-2.0 | https://github.com/psychon/as-raw-xcb-connection |
| `ash` | `0.38.0+1.3.281` | MIT OR Apache-2.0 | https://github.com/ash-rs/ash |
| `async-broadcast` | `0.7.2` | MIT OR Apache-2.0 | https://github.com/smol-rs/async-broadcast |
| `async-channel` | `2.5.0` | Apache-2.0 OR MIT | https://github.com/smol-rs/async-channel |
| `async-executor` | `1.14.0` | Apache-2.0 OR MIT | https://github.com/smol-rs/async-executor |
| `async-io` | `2.6.0` | Apache-2.0 OR MIT | https://github.com/smol-rs/async-io |
| `async-lock` | `3.4.2` | Apache-2.0 OR MIT | https://github.com/smol-rs/async-lock |
| `async-process` | `2.5.0` | Apache-2.0 OR MIT | https://github.com/smol-rs/async-process |
| `async-recursion` | `1.1.1` | MIT OR Apache-2.0 | https://github.com/dcchut/async-recursion |
| `async-signal` | `0.2.14` | Apache-2.0 OR MIT | https://github.com/smol-rs/async-signal |
| `async-task` | `4.7.1` | Apache-2.0 OR MIT | https://github.com/smol-rs/async-task |
| `async-trait` | `0.1.92` | MIT OR Apache-2.0 | https://github.com/dtolnay/async-trait |
| `atomic-waker` | `1.1.2` | Apache-2.0 OR MIT | https://github.com/smol-rs/atomic-waker |
| `atomicwrites` | `0.4.2` | MIT | https://github.com/untitaker/rust-atomicwrites |
| `atspi` | `0.29.0` | Apache-2.0 OR MIT | https://github.com/odilia-app/atspi |
| `atspi-common` | `0.13.0` | Apache-2.0 OR MIT | https://github.com/odilia-app/atspi |
| `atspi-proxies` | `0.13.0` | Apache-2.0 OR MIT | https://github.com/odilia-app/atspi |
| `auto_enums` | `0.8.10` | Apache-2.0 OR MIT | https://github.com/taiki-e/auto_enums |
| `autocfg` | `1.5.1` | Apache-2.0 OR MIT | https://github.com/cuviper/autocfg |
| `base64` | `0.22.1` | MIT OR Apache-2.0 | https://github.com/marshallpierce/rust-base64 |
| `basic-toml` | `0.1.10` | MIT OR Apache-2.0 | https://github.com/dtolnay/basic-toml |
| `bit-set` | `0.8.0` | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-set |
| `bit-vec` | `0.8.0` | Apache-2.0 OR MIT | https://github.com/contain-rs/bit-vec |
| `bitflags` | `1.3.2` | MIT/Apache-2.0 | https://github.com/bitflags/bitflags |
| `bitflags` | `2.13.2` | MIT OR Apache-2.0 | https://github.com/bitflags/bitflags |
| `block-buffer` | `0.12.1` | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| `blocking` | `1.7.0` | Apache-2.0 OR MIT | https://github.com/smol-rs/blocking |
| `bstr` | `1.13.1` | MIT OR Apache-2.0 | https://github.com/BurntSushi/bstr |
| `btoi` | `0.5.0` | MIT OR Apache-2.0 | https://github.com/niklasf/rust-btoi |
| `build_helpers` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `by_address` | `1.2.1` | MIT OR Apache-2.0 | https://github.com/mbrubeck/by_address |
| `bytemuck` | `1.25.2` | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| `bytemuck_derive` | `1.12.1` | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| `byteorder-lite` | `0.1.0` | Unlicense OR MIT | https://github.com/image-rs/byteorder-lite |
| `bytes` | `1.12.1` | MIT | https://github.com/tokio-rs/bytes |
| `calloop` | `0.14.4` | MIT | https://github.com/Smithay/calloop |
| `calloop-wayland-source` | `0.4.1` | MIT | https://github.com/smithay/calloop-wayland-source |
| `cc` | `1.5.1` | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs |
| `cfg-if` | `1.0.5` | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if |
| `cfg_aliases` | `0.2.2` | MIT | https://github.com/katharostech/cfg_aliases |
| `clipboard_wayland` | `0.2.2` | Apache-2.0 | https://github.com/hecrj/window_clipboard |
| `clipboard_x11` | `0.4.2` | MIT | https://github.com/hecrj/window_clipboard |
| `codespan-reporting` | `0.12.0` | Apache-2.0 | https://github.com/brendanzab/codespan |
| `color_quant` | `1.1.0` | MIT | https://github.com/image-rs/color_quant.git |
| `concurrent-queue` | `2.5.0` | Apache-2.0 OR MIT | https://github.com/smol-rs/concurrent-queue |
| `configparser` | `3.3.0` | MIT OR LGPL-3.0-or-later | https://github.com/QEDK/configparser-rs |
| `const-oid` | `0.10.2` | Apache-2.0 OR MIT | https://github.com/RustCrypto/formats |
| `core_maths` | `0.1.1` | MIT | https://github.com/robertbastian/core_maths |
| `cosmic-client-toolkit` | `0.2.0` | MIT | https://github.com/pop-os/cosmic-protocols |
| `cosmic-config` | `1.0.0` | MPL-2.0 | git+https://github.com/pop-os/libcosmic#87ab8179e1bd9880239c340855ae8862034bd0e8 |
| `cosmic-config-derive` | `1.0.0` | MPL-2.0 | git+https://github.com/pop-os/libcosmic#87ab8179e1bd9880239c340855ae8862034bd0e8 |
| `cosmic-freedesktop-icons` | `0.4.0` | MIT | https://github.com/pop-os/freedesktop-icons |
| `cosmic-mime-apps` | `0.2.0` | MPL-2.0 | git+https://github.com/pop-os/cosmic-mime-apps.git?rev=8fc81e8857e46c018ea21ab985aaf30ac853c007#8fc81e8857e46c018ea21ab985aaf30ac853c007 |
| `cosmic-protocols` | `0.2.0` | MIT | https://github.com/pop-os/cosmic-protocols |
| `cosmic-settings-config` | `1.9.0` | MPL-2.0 | git+https://github.com/pop-os/cosmic-settings-daemon#7ec3e3016b68c5e57f27cb18a4b0e6c413443a37 |
| `cosmic-settings-daemon` | `0.1.0` | MPL-2.0 | git+https://github.com/pop-os/dbus-settings-bindings#eed01dd3609e90e3c8cd043656734c500956c793 |
| `cosmic-text` | `0.19.0` | MIT OR Apache-2.0 | https://github.com/pop-os/cosmic-text |
| `cosmic-theme` | `1.0.0` | MPL-2.0 | git+https://github.com/pop-os/libcosmic#87ab8179e1bd9880239c340855ae8862034bd0e8 |
| `cpufeatures` | `0.3.1` | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| `crc32fast` | `1.5.2` | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast |
| `crossbeam-utils` | `0.8.23` | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| `cryoglyph` | `0.1.0` | MIT OR Apache-2.0 OR Zlib | https://github.com/iced-rs/cryoglyph |
| `crypto-common` | `0.2.2` | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| `css-color` | `0.2.8` | MIT OR Apache-2.0 | https://github.com/kalcutter/rust-css-color |
| `csscolorparser` | `0.8.4` | MIT OR Apache-2.0 | https://github.com/mazznoer/csscolorparser-rs |
| `ctor` | `0.10.1` | Apache-2.0 OR MIT | https://github.com/mmastrac/rust-ctor |
| `cursor-icon` | `1.2.0` | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/cursor-icon |
| `darling` | `0.21.3` | MIT | https://github.com/TedDriggs/darling |
| `darling` | `0.24.1` | MIT | https://github.com/TedDriggs/darling |
| `darling_core` | `0.21.3` | MIT | https://github.com/TedDriggs/darling |
| `darling_core` | `0.24.1` | MIT | https://github.com/TedDriggs/darling |
| `darling_macro` | `0.21.3` | MIT | https://github.com/TedDriggs/darling |
| `darling_macro` | `0.24.1` | MIT | https://github.com/TedDriggs/darling |
| `data-url` | `0.3.2` | MIT OR Apache-2.0 | https://github.com/servo/rust-url |
| `derive_setters` | `0.1.9` | MIT/Apache-2.0 | https://github.com/Lymia/derive_setters |
| `derive_utils` | `0.16.0` | Apache-2.0 OR MIT | https://github.com/taiki-e/derive_utils |
| `digest` | `0.11.3` | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| `dirs` | `6.0.0` | MIT OR Apache-2.0 | https://github.com/soc/dirs-rs |
| `dirs-sys` | `0.5.0` | MIT OR Apache-2.0 | https://github.com/dirs-dev/dirs-sys-rs |
| `displaydoc` | `0.2.7` | MIT OR Apache-2.0 | https://github.com/yaahc/displaydoc |
| `dlib` | `0.5.3` | MIT | https://github.com/elinorbgr/dlib |
| `dnd` | `0.1.0` | MIT | git+https://github.com/pop-os/window_clipboard.git?tag=sctk-0.20#f68595ee0e62fbd6589f4709b5aaa5c3c7ea5f6c |
| `document-features` | `0.2.12` | MIT OR Apache-2.0 | https://github.com/slint-ui/document-features |
| `downcast-rs` | `1.2.1` | MIT/Apache-2.0 | https://github.com/marcianx/downcast-rs |
| `dpi` | `0.1.2` | Apache-2.0 AND MIT | https://github.com/rust-windowing/winit |
| `drm` | `0.11.1` | MIT | https://github.com/Smithay/drm-rs |
| `drm-ffi` | `0.7.1` | MIT | https://github.com/Smithay/drm-rs |
| `drm-fourcc` | `2.2.0` | MIT | https://github.com/danielzfranklin/drm-fourcc-rs |
| `drm-sys` | `0.6.1` | MIT | https://github.com/Smithay/drm-rs |
| `endi` | `1.1.1` | MIT | https://github.com/zeenix/endi |
| `enumflags2` | `0.7.12` | MIT OR Apache-2.0 | https://github.com/meithecatte/enumflags2 |
| `enumflags2_derive` | `0.7.12` | MIT OR Apache-2.0 | https://github.com/meithecatte/enumflags2 |
| `equivalent` | `1.0.2` | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent |
| `errno` | `0.3.14` | MIT OR Apache-2.0 | https://github.com/lambda-fairy/rust-errno |
| `etagere` | `0.2.15` | MIT/Apache-2.0 | https://github.com/nical/etagere |
| `euclid` | `0.22.14` | MIT OR Apache-2.0 | https://github.com/servo/euclid |
| `event-listener` | `5.4.2` | Apache-2.0 OR MIT | https://github.com/smol-rs/event-listener |
| `event-listener-strategy` | `0.5.4` | Apache-2.0 OR MIT | https://github.com/smol-rs/event-listener-strategy |
| `fastrand` | `2.5.0` | Apache-2.0 OR MIT | https://github.com/smol-rs/fastrand |
| `fdeflate` | `0.3.7` | MIT OR Apache-2.0 | https://github.com/image-rs/fdeflate |
| `find-crate` | `0.6.3` | Apache-2.0 OR MIT | https://github.com/taiki-e/find-crate |
| `find-msvc-tools` | `0.1.14` | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs |
| `flate2` | `1.1.10` | MIT OR Apache-2.0 | https://github.com/rust-lang/flate2-rs |
| `float-cmp` | `0.9.0` | MIT | https://github.com/mikedilger/float-cmp |
| `float-cmp` | `0.10.0` | MIT | https://github.com/mikedilger/float-cmp |
| `float_next_after` | `1.0.0` | MIT | https://gitlab.com/bronsonbdevost/next_afterf |
| `fluent` | `0.17.0` | Apache-2.0 OR MIT | https://github.com/projectfluent/fluent-rs |
| `fluent-bundle` | `0.16.0` | Apache-2.0 OR MIT | https://github.com/projectfluent/fluent-rs |
| `fluent-langneg` | `0.13.1` | Apache-2.0 OR MIT | https://github.com/projectfluent/fluent-langneg-rs |
| `fluent-syntax` | `0.12.0` | Apache-2.0 OR MIT | https://github.com/projectfluent/fluent-rs |
| `fnv` | `1.0.7` | Apache-2.0 / MIT | https://github.com/servo/rust-fnv |
| `foldhash` | `0.1.5` | Zlib | https://github.com/orlp/foldhash |
| `foldhash` | `0.2.0` | Zlib | https://github.com/orlp/foldhash |
| `font-types` | `0.11.3` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| `font-types` | `0.12.5` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| `fontconfig-parser` | `0.5.8` | MIT | https://github.com/Riey/fontconfig-parser |
| `fontdb` | `0.23.0` | MIT | https://github.com/RazrFalcon/fontdb |
| `form_urlencoded` | `1.2.2` | MIT OR Apache-2.0 | https://github.com/servo/rust-url |
| `freedesktop-desktop-entry` | `0.8.3` | MPL-2.0 | https://github.com/pop-os/freedesktop-desktop-entry |
| `futures` | `0.3.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-channel` | `0.3.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-core` | `0.3.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-executor` | `0.3.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-io` | `0.3.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-lite` | `2.6.1` | Apache-2.0 OR MIT | https://github.com/smol-rs/futures-lite |
| `futures-macro` | `0.3.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-sink` | `0.3.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-task` | `0.3.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `futures-util` | `0.3.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| `gethostname` | `1.1.0` | Apache-2.0 | https://codeberg.org/swsnr/gethostname.rs.git |
| `getrandom` | `0.3.4` | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom |
| `getrandom` | `0.4.3` | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom |
| `gettext-rs` | `0.8.0` | MIT | https://github.com/gettext-rs/gettext-rs |
| `gettext-sys` | `0.27.0` | MIT | https://github.com/gettext-rs/gettext-rs |
| `gif` | `0.13.3` | MIT OR Apache-2.0 | https://github.com/image-rs/image-gif |
| `glam` | `0.25.0` | MIT OR Apache-2.0 | https://github.com/bitshifter/glam-rs |
| `glow` | `0.16.0` | MIT OR Apache-2.0 OR Zlib | https://github.com/grovesNL/glow |
| `gpu-allocator` | `0.28.0` | MIT OR Apache-2.0 | https://github.com/Traverse-Research/gpu-allocator |
| `gpu-descriptor` | `0.3.2` | MIT OR Apache-2.0 | https://github.com/zakarumych/gpu-descriptor |
| `gpu-descriptor-types` | `0.2.0` | MIT OR Apache-2.0 | https://github.com/zakarumych/gpu-descriptor |
| `grid` | `1.0.1` | MIT | https://github.com/becheran/grid |
| `guillotiere` | `0.6.2` | MIT/Apache-2.0 | https://github.com/nical/guillotiere |
| `half` | `2.7.1` | MIT OR Apache-2.0 | https://github.com/VoidStarKat/half-rs |
| `harfrust` | `0.5.2` | MIT | https://github.com/harfbuzz/harfrust |
| `hashbrown` | `0.15.5` | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| `hashbrown` | `0.16.1` | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| `hashbrown` | `0.17.1` | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| `heck` | `0.4.1` | MIT OR Apache-2.0 | https://github.com/withoutboats/heck |
| `hex` | `0.4.3` | MIT OR Apache-2.0 | https://github.com/KokaKiwi/rust-hex |
| `hex_color` | `3.0.0` | MIT OR Apache-2.0 | https://github.com/seancroach/hex_color |
| `hexf-parse` | `0.2.1` | CC0-1.0 | https://github.com/lifthrasiir/hexf |
| `hybrid-array` | `0.4.15` | MIT OR Apache-2.0 | https://github.com/RustCrypto/hybrid-array |
| `i18n-config` | `0.4.8` | MIT | https://github.com/kellpossible/cargo-i18n/tree/master/i18n-config |
| `i18n-embed` | `0.16.0` | MIT | https://github.com/kellpossible/cargo-i18n/tree/master/i18n-embed |
| `i18n-embed-fl` | `0.10.1` | MIT | https://github.com/kellpossible/cargo-i18n/tree/master/i18n-embed-fl |
| `i18n-embed-impl` | `0.8.4` | MIT | https://github.com/kellpossible/cargo-i18n/tree/master/i18n-embed |
| `iced` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `iced_accessibility` | `0.1.0` | MIT | git+https://github.com/pop-os/libcosmic#87ab8179e1bd9880239c340855ae8862034bd0e8 |
| `iced_core` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `iced_debug` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `iced_futures` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `iced_graphics` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `iced_program` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `iced_renderer` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `iced_runtime` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `iced_tiny_skia` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `iced_wgpu` | `0.14.0` | MIT | https://github.com/iced-rs/iced |
| `iced_widget` | `0.14.2` | MIT | https://github.com/iced-rs/iced |
| `iced_winit` | `0.14.0` | MIT, with one MPL-2.0 file | https://github.com/iced-rs/iced |
| `icu_collections` | `2.3.0` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_locale_core` | `2.3.0` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_normalizer` | `2.3.0` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_normalizer_data` | `2.3.0` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_properties` | `2.3.0` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_properties_data` | `2.3.0` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `icu_provider` | `2.3.1` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `ident_case` | `1.0.1` | MIT/Apache-2.0 | https://github.com/TedDriggs/ident_case |
| `idna` | `1.1.0` | MIT OR Apache-2.0 | https://github.com/servo/rust-url/ |
| `idna_adapter` | `1.2.2` | Apache-2.0 OR MIT | https://github.com/hsivonen/idna_adapter |
| `image` | `0.25.10` | MIT OR Apache-2.0 | https://github.com/image-rs/image |
| `image-extras` | `0.1.1` | MIT OR Apache-2.0 | https://github.com/image-rs/image-extras |
| `image-webp` | `0.2.4` | MIT OR Apache-2.0 | https://github.com/image-rs/image-webp |
| `imagesize` | `0.13.0` | MIT | https://github.com/Roughsketch/imagesize |
| `indexmap` | `2.14.2` | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap |
| `inotify` | `0.11.5` | ISC | https://github.com/hannobraun/inotify-rs |
| `inotify-sys` | `0.1.8` | ISC | https://github.com/hannobraun/inotify-sys |
| `intl-memoizer` | `0.5.3` | Apache-2.0 OR MIT | https://github.com/projectfluent/fluent-rs |
| `intl_pluralrules` | `7.0.2` | Apache-2.0/MIT | https://github.com/zbraniecki/pluralrules |
| `itoa` | `1.0.18` | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa |
| `jiff` | `0.2.37` | Unlicense OR MIT | https://github.com/BurntSushi/jiff |
| `jiff-core` | `0.1.1` | Unlicense OR MIT | https://github.com/BurntSushi/jiff |
| `kamadak-exif` | `0.6.1` | BSD-2-Clause | https://github.com/kamadak/exif-rs |
| `keyboard-types` | `0.8.3` | MIT OR Apache-2.0 | https://github.com/rust-windowing/keyboard-types |
| `khronos-egl` | `6.0.0` | MIT/Apache-2.0 | https://github.com/timothee-haudebourg/khronos-egl |
| `kurbo` | `0.10.4` | MIT OR Apache-2.0 | https://github.com/linebender/kurbo |
| `kurbo` | `0.11.3` | Apache-2.0 OR MIT | https://github.com/linebender/kurbo |
| `lazy_static` | `1.5.1` | MIT OR Apache-2.0 | https://github.com/rust-lang-nursery/lazy-static.rs |
| `libc` | `0.2.189` | MIT OR Apache-2.0 | https://github.com/rust-lang/libc |
| `libcosmic` | `1.0.0` | MPL-2.0, with file-level GPL-3.0-only and MIT files | git+https://github.com/pop-os/libcosmic#87ab8179e1bd9880239c340855ae8862034bd0e8 |
| `libloading` | `0.8.9` | ISC | https://github.com/nagisa/rust_libloading/ |
| `libm` | `0.2.16` | MIT | https://github.com/rust-lang/compiler-builtins |
| `lilt` | `0.8.2` | MIT | https://github.com/cyypherus/lilt |
| `linebender_resource_handle` | `0.1.1` | Apache-2.0 OR MIT | https://github.com/linebender/raw_resource_handle |
| `linux-raw-sys` | `0.4.15` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/sunfishcode/linux-raw-sys |
| `linux-raw-sys` | `0.6.5` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/sunfishcode/linux-raw-sys |
| `linux-raw-sys` | `0.12.1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/sunfishcode/linux-raw-sys |
| `litemap` | `0.8.3` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `litrs` | `1.0.0` | MIT OR Apache-2.0 | https://github.com/LukasKalbertodt/litrs |
| `locale_config` | `0.3.0` | MIT | https://github.com/rust-locale/locale_config/ |
| `lock_api` | `0.4.14` | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| `log` | `0.4.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/log |
| `lru` | `0.16.4` | MIT | https://github.com/jeromefroe/lru-rs.git |
| `lyon` | `1.0.19` | MIT OR Apache-2.0 | https://github.com/nical/lyon |
| `lyon_algorithms` | `1.0.21` | MIT OR Apache-2.0 | https://github.com/nical/lyon |
| `lyon_geom` | `1.0.19` | MIT OR Apache-2.0 | https://github.com/nical/lyon |
| `lyon_path` | `1.0.19` | MIT OR Apache-2.0 | https://github.com/nical/lyon |
| `lyon_tessellation` | `1.0.22` | MIT OR Apache-2.0 | https://github.com/nical/lyon |
| `matchers` | `0.2.0` | MIT | https://github.com/hawkw/matchers |
| `memchr` | `2.8.3` | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| `memmap2` | `0.8.0` | MIT OR Apache-2.0 | https://github.com/RazrFalcon/memmap2-rs |
| `memmap2` | `0.9.11` | MIT OR Apache-2.0 | https://github.com/RazrFalcon/memmap2-rs |
| `mime` | `0.1.0` | MIT | git+https://github.com/pop-os/window_clipboard.git?tag=sctk-0.20#f68595ee0e62fbd6589f4709b5aaa5c3c7ea5f6c |
| `mime` | `0.3.17` | MIT OR Apache-2.0 | https://github.com/hyperium/mime |
| `mime_guess` | `2.0.5` | MIT | https://github.com/abonander/mime_guess |
| `miniz_oxide` | `0.8.9` | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| `miniz_oxide` | `0.9.1` | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| `mio` | `1.2.3` | MIT | https://github.com/tokio-rs/mio |
| `moxcms` | `0.8.1` | BSD-3-Clause OR Apache-2.0 | https://github.com/awxkee/moxcms.git |
| `mutate_once` | `0.1.2` | BSD-2-Clause | https://github.com/kamadak/mutate_once-rs |
| `naga` | `28.0.0` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `notify` | `8.2.0` | CC0-1.0 | https://github.com/notify-rs/notify.git |
| `notify-types` | `2.1.0` | MIT OR Apache-2.0 | https://github.com/notify-rs/notify.git |
| `nu-ansi-term` | `0.50.3` | MIT | https://github.com/nushell/nu-ansi-term |
| `num-traits` | `0.2.19` | MIT OR Apache-2.0 | https://github.com/rust-num/num-traits |
| `once_cell` | `1.21.4` | MIT OR Apache-2.0 | https://github.com/matklad/once_cell |
| `option-ext` | `0.2.0` | MPL-2.0 | https://github.com/soc/option-ext.git |
| `ordered-float` | `5.5.0` | MIT | https://github.com/reem/rust-ordered-float |
| `ordered-stream` | `0.2.0` | MIT OR Apache-2.0 | https://github.com/danieldg/ordered-stream |
| `ouroboros` | `0.18.5` | MIT OR Apache-2.0 | https://github.com/someguynamedjosh/ouroboros |
| `ouroboros_macro` | `0.18.5` | MIT OR Apache-2.0 | https://github.com/someguynamedjosh/ouroboros |
| `owned_ttf_parser` | `0.25.1` | Apache-2.0 | https://github.com/alexheretic/owned-ttf-parser |
| `palette` | `0.7.7` | MIT OR Apache-2.0 | https://github.com/Ogeon/palette |
| `palette_derive` | `0.7.7` | MIT OR Apache-2.0 | https://github.com/Ogeon/palette |
| `palette_math` | `0.7.7` | MIT OR Apache-2.0 | https://github.com/Ogeon/palette |
| `parking` | `2.2.1` | Apache-2.0 OR MIT | https://github.com/smol-rs/parking |
| `parking_lot` | `0.12.5` | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| `parking_lot_core` | `0.9.12` | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| `percent-encoding` | `2.3.2` | MIT OR Apache-2.0 | https://github.com/servo/rust-url/ |
| `phf` | `0.13.1` | MIT | https://github.com/rust-phf/rust-phf |
| `phf_generator` | `0.13.1` | MIT | https://github.com/rust-phf/rust-phf |
| `phf_macros` | `0.13.1` | MIT | https://github.com/rust-phf/rust-phf |
| `phf_shared` | `0.13.1` | MIT | https://github.com/rust-phf/rust-phf |
| `pico-args` | `0.5.0` | MIT | https://github.com/RazrFalcon/pico-args |
| `pin-project-lite` | `0.2.17` | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project-lite |
| `piper` | `0.2.5` | MIT OR Apache-2.0 | https://github.com/smol-rs/piper |
| `pkg-config` | `0.3.34` | MIT OR Apache-2.0 | https://github.com/rust-lang/pkg-config-rs |
| `png` | `0.17.16` | MIT OR Apache-2.0 | https://github.com/image-rs/image-png |
| `png` | `0.18.1` | MIT OR Apache-2.0 | https://github.com/image-rs/image-png |
| `polling` | `3.11.0` | Apache-2.0 OR MIT | https://github.com/smol-rs/polling |
| `potential_utf` | `0.1.6` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `presser` | `0.3.1` | MIT OR Apache-2.0 | https://github.com/EmbarkStudios/presser |
| `proc-macro-crate` | `3.5.0` | MIT OR Apache-2.0 | https://github.com/bkchr/proc-macro-crate |
| `proc-macro-error-attr3` | `3.1.1` | MIT OR Apache-2.0 | https://github.com/gamma0987/proc-macro-error3 |
| `proc-macro-error3` | `3.1.1` | MIT OR Apache-2.0 | https://github.com/gamma0987/proc-macro-error3 |
| `proc-macro2` | `1.0.107` | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 |
| `proc-macro2-diagnostics` | `0.10.1` | MIT/Apache-2.0 | https://github.com/SergioBenitez/proc-macro2-diagnostics |
| `profiling` | `1.0.18` | MIT OR Apache-2.0 | https://github.com/aclysma/profiling |
| `pxfm` | `0.1.30` | BSD-3-Clause OR Apache-2.0 | https://github.com/awxkee/pxfm |
| `quick-error` | `2.0.1` | MIT/Apache-2.0 | http://github.com/tailhook/quick-error |
| `quick-xml` | `0.38.4` | MIT | https://github.com/tafia/quick-xml |
| `quick-xml` | `0.41.0` | MIT | https://github.com/tafia/quick-xml |
| `quote` | `1.0.47` | MIT OR Apache-2.0 | https://github.com/dtolnay/quote |
| `rangemap` | `1.8.0` | MIT/Apache-2.0 | https://github.com/jeffparsons/rangemap |
| `raw-window-handle` | `0.6.2` | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/raw-window-handle |
| `read-fonts` | `0.37.0` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| `read-fonts` | `0.41.0` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| `regex` | `1.13.1` | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| `regex-automata` | `0.4.18` | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| `regex-syntax` | `0.8.11` | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| `renderdoc-sys` | `1.1.0` | MIT OR Apache-2.0 | https://github.com/ebkalderon/renderdoc-rs |
| `resvg` | `0.45.1` | Apache-2.0 OR MIT | https://github.com/linebender/resvg |
| `rgb` | `0.8.53` | MIT | https://github.com/kornelski/rust-rgb |
| `ron` | `0.11.0` | MIT OR Apache-2.0 | https://github.com/ron-rs/ron |
| `ron` | `0.12.2` | MIT OR Apache-2.0 | https://github.com/ron-rs/ron |
| `roxmltree` | `0.20.0` | MIT OR Apache-2.0 | https://github.com/RazrFalcon/roxmltree |
| `rust-embed` | `8.12.0` | MIT | https://pyrossh.dev/repos/rust-embed |
| `rust-embed-impl` | `8.12.0` | MIT | https://pyrossh.dev/repos/rust-embed |
| `rust-embed-utils` | `8.12.0` | MIT | https://pyrossh.dev/repos/rust-embed |
| `rustc-hash` | `1.1.0` | Apache-2.0/MIT | https://github.com/rust-lang-nursery/rustc-hash |
| `rustc-hash` | `2.1.3` | Apache-2.0 OR MIT | https://github.com/rust-lang/rustc-hash |
| `rustix` | `0.38.44` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/rustix |
| `rustix` | `1.1.5` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | https://github.com/bytecodealliance/rustix |
| `rustversion` | `1.0.23` | MIT OR Apache-2.0 | https://github.com/dtolnay/rustversion |
| `rustybuzz` | `0.20.1` | MIT | https://github.com/harfbuzz/rustybuzz |
| `same-file` | `1.0.6` | Unlicense/MIT | https://github.com/BurntSushi/same-file |
| `scoped-tls` | `1.0.1` | MIT/Apache-2.0 | https://github.com/alexcrichton/scoped-tls |
| `scopeguard` | `1.2.0` | MIT OR Apache-2.0 | https://github.com/bluss/scopeguard |
| `sctk-adwaita` | `0.11.1` | MIT | https://github.com/PolyMeilex/sctk-adwaita |
| `self_cell` | `1.3.0` | Apache-2.0 OR GPL-2.0-only | https://github.com/Voultapher/self_cell |
| `serde` | `1.0.229` | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| `serde_core` | `1.0.229` | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| `serde_derive` | `1.0.229` | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| `serde_json` | `1.0.151` | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| `serde_repr` | `0.1.21` | MIT OR Apache-2.0 | https://github.com/dtolnay/serde-repr |
| `serde_with` | `3.24.0` | MIT OR Apache-2.0 | https://github.com/jonasbb/serde_with/ |
| `serde_with_macros` | `3.24.0` | MIT OR Apache-2.0 | https://github.com/jonasbb/serde_with/ |
| `sha2` | `0.11.0` | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| `sharded-slab` | `0.1.7` | MIT | https://github.com/hawkw/sharded-slab |
| `shlex` | `1.3.0` | MIT OR Apache-2.0 | https://github.com/comex/rust-shlex |
| `shlex` | `2.0.1` | MIT OR Apache-2.0 | https://github.com/comex/rust-shlex |
| `signal-hook-registry` | `1.4.8` | MIT OR Apache-2.0 | https://github.com/vorner/signal-hook |
| `simd-adler32` | `0.3.10` | MIT | https://github.com/mcountryman/simd-adler32 |
| `simplecss` | `0.2.2` | Apache-2.0 OR MIT | https://github.com/linebender/simplecss |
| `siphasher` | `1.0.4` | MIT OR Apache-2.0 | https://github.com/jedisct1/rust-siphash |
| `skrifa` | `0.40.0` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| `skrifa` | `0.44.0` | MIT OR Apache-2.0 | https://github.com/googlefonts/fontations |
| `slab` | `0.4.12` | MIT | https://github.com/tokio-rs/slab |
| `slotmap` | `1.1.1` | Zlib | https://github.com/orlp/slotmap |
| `smallvec` | `1.16.2` | MIT OR Apache-2.0 | https://github.com/servo/rust-smallvec |
| `smithay-client-toolkit` | `0.20.0` | MIT | https://github.com/smithay/client-toolkit |
| `smithay-clipboard` | `0.8.0` | MIT | https://github.com/smithay/smithay-clipboard |
| `smol_str` | `0.3.6` | MIT OR Apache-2.0 | https://github.com/rust-lang/rust-analyzer/tree/master/lib/smol_str |
| `socket2` | `0.6.5` | MIT OR Apache-2.0 | https://github.com/rust-lang/socket2 |
| `softbuffer` | `0.4.1` | MIT OR Apache-2.0 | https://github.com/rust-windowing/softbuffer |
| `spirv` | `0.3.0+sdk-1.3.268.0` | Apache-2.0 | https://github.com/gfx-rs/rspirv |
| `stable_deref_trait` | `1.2.1` | MIT OR Apache-2.0 | https://github.com/storyyeller/stable_deref_trait |
| `static_assertions` | `1.1.0` | MIT OR Apache-2.0 | https://github.com/nvzqz/static-assertions-rs |
| `strict-num` | `0.1.1` | MIT | https://github.com/RazrFalcon/strict-num |
| `strsim` | `0.11.1` | MIT | https://github.com/rapidfuzz/strsim-rs |
| `svg_fmt` | `0.4.5` | MIT/Apache-2.0 | https://github.com/nical/rust_debug |
| `svgtypes` | `0.15.3` | Apache-2.0 OR MIT | https://github.com/linebender/svgtypes |
| `swash` | `0.2.10` | Apache-2.0 OR MIT | https://github.com/dfrg/swash |
| `syn` | `2.0.119` | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| `syn` | `3.0.6` | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| `synstructure` | `0.14.0` | MIT | https://github.com/mystor/synstructure |
| `sys-locale` | `0.3.2` | MIT OR Apache-2.0 | https://github.com/1Password/sys-locale |
| `taffy` | `0.9.2` | MIT | https://github.com/DioxusLabs/taffy |
| `temp-dir` | `0.1.16` | Apache-2.0 | https://gitlab.com/leonhard-llc/ops |
| `tempfile` | `3.27.0` | MIT OR Apache-2.0 | https://github.com/Stebalien/tempfile |
| `thiserror` | `1.0.69` | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| `thiserror` | `2.0.21` | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| `thiserror-impl` | `1.0.69` | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| `thiserror-impl` | `2.0.21` | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| `thread_local` | `1.1.10` | MIT OR Apache-2.0 | https://github.com/Amanieu/thread_local-rs |
| `tiny-skia` | `0.11.4` | BSD-3-Clause | https://github.com/RazrFalcon/tiny-skia |
| `tiny-skia` | `0.12.0` | BSD-3-Clause | https://github.com/linebender/tiny-skia |
| `tiny-skia-path` | `0.11.4` | BSD-3-Clause | https://github.com/RazrFalcon/tiny-skia/tree/master/path |
| `tiny-skia-path` | `0.12.0` | BSD-3-Clause | https://github.com/linebender/tiny-skia/tree/master/path |
| `tiny-xlib` | `0.2.5` | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/tiny-xlib |
| `tinystr` | `0.8.4` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `tinyvec` | `1.13.3` | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/tinyvec |
| `tokio` | `1.53.1` | MIT | https://github.com/tokio-rs/tokio |
| `tokio-macros` | `2.7.2` | MIT | https://github.com/tokio-rs/tokio |
| `tokio-stream` | `0.1.19` | MIT | https://github.com/tokio-rs/tokio |
| `toml` | `0.5.11` | MIT/Apache-2.0 | https://github.com/toml-rs/toml |
| `toml_datetime` | `1.1.1+spec-1.1.0` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| `toml_edit` | `0.25.15+spec-1.1.0` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| `toml_parser` | `1.1.3+spec-1.1.0` | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| `tracing` | `0.1.44` | MIT | https://github.com/tokio-rs/tracing |
| `tracing-attributes` | `0.1.31` | MIT | https://github.com/tokio-rs/tracing |
| `tracing-core` | `0.1.36` | MIT | https://github.com/tokio-rs/tracing |
| `tracing-log` | `0.2.0` | MIT | https://github.com/tokio-rs/tracing |
| `tracing-subscriber` | `0.3.23` | MIT | https://github.com/tokio-rs/tracing |
| `ttf-parser` | `0.25.1` | MIT OR Apache-2.0 | https://github.com/harfbuzz/ttf-parser |
| `type-map` | `0.5.1` | MIT/Apache-2.0 | https://github.com/kardeiz/type-map |
| `typeid` | `1.0.3` | MIT OR Apache-2.0 | https://github.com/dtolnay/typeid |
| `typenum` | `1.20.1` | MIT OR Apache-2.0 | https://github.com/paholg/typenum |
| `uncased` | `0.9.10` | MIT OR Apache-2.0 | https://github.com/SergioBenitez/uncased |
| `unic-langid` | `0.9.6` | MIT OR Apache-2.0 | https://github.com/zbraniecki/unic-locale |
| `unic-langid-impl` | `0.9.6` | MIT OR Apache-2.0 | https://github.com/zbraniecki/unic-locale |
| `unicase` | `2.9.0` | MIT OR Apache-2.0 | https://github.com/seanmonstar/unicase |
| `unicode-bidi` | `0.3.18` | MIT OR Apache-2.0 | https://github.com/servo/unicode-bidi |
| `unicode-bidi-mirroring` | `0.4.0` | MIT/Apache-2.0 | https://github.com/RazrFalcon/unicode-bidi-mirroring |
| `unicode-ccc` | `0.4.0` | MIT/Apache-2.0 | https://github.com/RazrFalcon/unicode-ccc |
| `unicode-ident` | `1.0.26` | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| `unicode-linebreak` | `0.1.5` | Apache-2.0 | https://github.com/axelf4/unicode-linebreak |
| `unicode-properties` | `0.1.4` | MIT/Apache-2.0 | https://github.com/unicode-rs/unicode-properties |
| `unicode-script` | `0.5.8` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-script |
| `unicode-segmentation` | `1.13.3` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-segmentation |
| `unicode-vo` | `0.1.0` | MIT/Apache-2.0 | https://github.com/RazrFalcon/unicode-vo |
| `unicode-width` | `0.2.2` | MIT OR Apache-2.0 | https://github.com/unicode-rs/unicode-width |
| `url` | `2.5.8` | MIT OR Apache-2.0 | https://github.com/servo/rust-url |
| `usvg` | `0.45.1` | Apache-2.0 OR MIT | https://github.com/linebender/resvg |
| `utf8_iter` | `1.0.4` | Apache-2.0 OR MIT | https://github.com/hsivonen/utf8_iter |
| `uuid` | `1.26.1` | Apache-2.0 OR MIT | https://github.com/uuid-rs/uuid |
| `version_check` | `0.9.5` | MIT/Apache-2.0 | https://github.com/SergioBenitez/version_check |
| `walkdir` | `2.5.0` | Unlicense/MIT | https://github.com/BurntSushi/walkdir |
| `wayland-backend` | `0.3.17` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-client` | `0.31.15` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-csd-frame` | `0.3.0` | MIT | https://github.com/rust-windowing/wayland-csd-frame |
| `wayland-cursor` | `0.31.14` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols` | `0.32.13` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols-experimental` | `20250721.0.1` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols-misc` | `0.3.12` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols-plasma` | `0.3.12` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-protocols-wlr` | `0.3.12` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-scanner` | `0.31.11` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-server` | `0.31.14` | MIT | https://github.com/smithay/wayland-rs |
| `wayland-sys` | `0.31.11` | MIT | https://github.com/smithay/wayland-rs |
| `web-time` | `1.1.0` | MIT OR Apache-2.0 | https://github.com/daxpedda/web-time |
| `weezl` | `0.1.12` | MIT OR Apache-2.0 | https://github.com/image-rs/weezl |
| `wgpu` | `28.0.0` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-core` | `28.0.1` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-core-deps-windows-linux-android` | `28.0.0` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-hal` | `28.0.1` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `wgpu-types` | `28.0.0` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| `window_clipboard` | `0.4.1` | MIT | https://github.com/hecrj/window_clipboard |
| `winit` | `0.31.0-beta.2` | Apache-2.0 | https://github.com/rust-windowing/winit |
| `winit-common` | `0.31.0-beta.2` | Apache-2.0 | https://github.com/rust-windowing/winit |
| `winit-core` | `0.31.0-beta.2` | Apache-2.0 | https://github.com/rust-windowing/winit |
| `winit-wayland` | `0.31.0-beta.2` | Apache-2.0 | https://github.com/rust-windowing/winit |
| `winit-x11` | `0.31.0-beta.2` | Apache-2.0 | https://github.com/rust-windowing/winit |
| `winnow` | `1.0.4` | MIT | https://github.com/winnow-rs/winnow |
| `writeable` | `0.6.4` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `x11-dl` | `2.21.0` | MIT | https://github.com/AltF02/x11-rs.git |
| `x11rb` | `0.13.2` | MIT OR Apache-2.0 | https://github.com/psychon/x11rb |
| `x11rb-protocol` | `0.13.2` | MIT OR Apache-2.0 | https://github.com/psychon/x11rb |
| `xcursor` | `0.3.11` | MIT | https://github.com/esposm03/xcursor-rs |
| `xdg` | `3.0.0` | Apache-2.0 OR MIT | https://github.com/whitequark/rust-xdg |
| `xkbcommon` | `0.7.0` | MIT | https://github.com/rust-x-bindings/xkbcommon-rs |
| `xkbcommon` | `0.8.0` | MIT | https://github.com/rust-x-bindings/xkbcommon-rs |
| `xkbcommon` | `0.9.0` | MIT | https://github.com/rust-x-bindings/xkbcommon-rs |
| `xkbcommon-dl` | `0.4.2` | MIT | https://github.com/rust-windowing/xkbcommon-dl |
| `xkeysym` | `0.2.1` | MIT OR Apache-2.0 OR Zlib | https://github.com/notgull/xkeysym |
| `xmlwriter` | `0.1.0` | MIT | https://github.com/RazrFalcon/xmlwriter |
| `yansi` | `1.0.1` | MIT OR Apache-2.0 | https://github.com/SergioBenitez/yansi |
| `yazi` | `0.2.1` | Apache-2.0 OR MIT | https://github.com/dfrg/yazi |
| `yoke` | `0.8.3` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `yoke-derive` | `0.8.4` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zbus` | `5.19.0` | MIT | https://github.com/z-galaxy/zbus/ |
| `zbus-lockstep` | `0.5.2` | MIT | https://github.com/luukvanderduim/zbus-lockstep |
| `zbus-lockstep-macros` | `0.5.2` | MIT | https://github.com/luukvanderduim/zbus-lockstep |
| `zbus_macros` | `5.19.0` | MIT | https://github.com/z-galaxy/zbus/ |
| `zbus_names` | `4.3.4` | MIT | https://github.com/z-galaxy/zbus/ |
| `zbus_xml` | `5.2.1` | MIT | https://github.com/z-galaxy/zbus/ |
| `zcheapstr` | `1.1.0` | MIT | https://github.com/z-galaxy/zcheapstr/ |
| `zeno` | `0.3.3` | Apache-2.0 OR MIT | https://github.com/dfrg/zeno |
| `zerocopy` | `0.8.59` | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy |
| `zerocopy-derive` | `0.8.59` | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy |
| `zerofrom` | `0.1.8` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zerofrom-derive` | `0.1.8` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zerotrie` | `0.2.5` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zerovec` | `0.11.8` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zerovec-derive` | `0.11.6` | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| `zmij` | `1.0.23` | MIT | https://github.com/dtolnay/zmij |
| `zune-core` | `0.4.12` | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image/tree/dev/zune-core |
| `zune-core` | `0.5.3` | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image |
| `zune-jpeg` | `0.4.21` | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg |
| `zune-jpeg` | `0.5.15` | MIT OR Apache-2.0 OR Zlib | https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg |
| `zvariant` | `5.15.0` | MIT | https://github.com/z-galaxy/zbus/ |
| `zvariant_derive` | `5.15.0` | MIT | https://github.com/z-galaxy/zbus/ |
| `zvariant_utils` | `4.2.0` | MIT | https://github.com/z-galaxy/zbus/ |
<!-- END GENERATED: rust-dependencies -->
