//! Phosphor icons (Fill weight, MIT, see `resources/icons/phosphor/LICENSE`), bundled so every
//! icon the app chooses looks the same on any icon theme, plus the app logo and the two
//! illustrations. Window chrome drawn by libcosmic itself (window controls, drawer Close) stays on
//! the system theme, and Back buttons use the system `go-previous-symbolic` like COSMIC Settings.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex, PoisonError};

use cosmic::iced::gradient::{ColorStop, Linear};
use cosmic::iced::widget::container as container_style;
use cosmic::iced::{Background, Color, Gradient, Length, Radians, Size};
use cosmic::widget::icon as widget;
use cosmic::widget::{Space, container, svg};

macro_rules! icons {
    ($($variant:ident => $file:literal,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Icon {
            $($variant,)*
        }

        impl Icon {
            fn svg(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => include_bytes!(concat!(
                        "../resources/icons/phosphor/",
                        $file,
                        ".svg"
                    )),)*
                }
            }

            #[cfg(test)]
            const FILES: &[(Self, &str)] = &[$((Self::$variant, $file),)*];
        }
    };
}

icons! {
    AppWindow => "app-window",
    ArrowCounterClockwise => "arrow-counter-clockwise",
    ArrowSquareOut => "arrow-square-out",
    CaretRight => "caret-right",
    Check => "check",
    Copy => "copy",
    CursorClick => "cursor-click",
    Detective => "detective",
    LockSimple => "lock-simple",
    LockSimpleOpen => "lock-simple-open",
    PushPin => "push-pin",
    Stack => "stack",
    WarningCircle => "warning-circle",
}

/// How an app's icon is drawn, from the value a tile or row holds.
#[derive(Debug, PartialEq, Eq)]
pub enum AppIcon<'a> {
    /// A theme icon of this name.
    Named(&'a str),
    /// The image at this absolute path.
    File(&'a Path),
    /// An absolute path with no file there: the generic app icon instead.
    Missing,
}

/// How to draw an app's icon held as `value`: a name is looked up in the icon theme, an absolute path is a file, drawn
/// only when it can be read.
#[must_use]
pub fn app_icon_of(value: &str) -> AppIcon<'_> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return AppIcon::Named(value);
    }
    let readable = crate::index::open_regular(path).is_ok();
    if readable {
        return AppIcon::File(path);
    }
    AppIcon::Missing
}

/// A theme-recolored handle, for widgets that take one (buttons, menus).
pub fn handle(icon: Icon) -> widget::Handle {
    widget::from_svg_bytes(icon.svg()).symbolic(true)
}

/// A sized icon element.
pub fn icon(icon: Icon, size: u16) -> widget::Icon {
    handle(icon).icon().size(size)
}

const LOGO: &[u8] = include_bytes!("../data/icons/com.lkbddh.signpost.svg");

/// The app icon in full color, never recolored.
pub fn logo() -> widget::Handle {
    widget::from_svg_bytes(LOGO)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Illustration {
    AboutHero,
    NoApps,
}

/// Each illustration's `[light, dark]` file, in `Illustration` order. Every pixel is opaque: the GPU renderer
/// blends an SVG's partly transparent pixels as if they were not premultiplied, darker than they are, so the
/// window fades each one in above its top with [`sky_fade`].
const ILLUSTRATIONS: [[&[u8]; 2]; 2] = [
    [
        include_bytes!("../resources/about-hero-light.svg"),
        include_bytes!("../resources/about-hero.svg"),
    ],
    [
        include_bytes!("../resources/no-apps-light.svg"),
        include_bytes!("../resources/no-apps.svg"),
    ],
];

/// Built once: `Handle::from_memory` hashes the whole file, which a view drawn every frame must not
/// repeat.
static HANDLES: LazyLock<[[svg::Handle; 2]; 2]> =
    LazyLock::new(|| ILLUSTRATIONS.map(|files| files.map(svg::Handle::from_memory)));

/// The illustration drawn for a dark or a light theme; every request shares the one handle.
#[must_use]
pub fn illustration(which: Illustration, dark: bool) -> svg::Handle {
    HANDLES[which as usize][usize::from(dark)].clone()
}

/// The no-apps illustration's size, as its files draw it: logical pixels, where it is shown.
pub const NO_APPS_SIZE: Size = Size::new(476.0, 180.0);
/// How much of the no-apps illustration's top [`no_apps_rounded`] leaves to the window's fade: its files' sky
/// starts there.
pub const NO_APPS_FADE: f32 = 65.0;

/// Each illustration's `[light, dark]` sky where its file begins it, in `Illustration` order.
const SKIES: [[Color; 2]; 2] = [
    [
        Color::from_rgb8(0x7D, 0xBD, 0xEB),
        Color::from_rgb8(0x15, 0x1A, 0x2C),
    ],
    [
        Color::from_rgb8(0x74, 0xB9, 0xEA),
        Color::from_rgb8(0x1A, 0x18, 0x30),
    ],
];

/// The color of the illustration's sky at its top, for the dark or the light theme.
#[must_use]
pub fn sky(which: Illustration, dark: bool) -> Color {
    SKIES[which as usize][usize::from(dark)]
}

/// A band `height` tall of `sky`, clear at its top and solid at its bottom, eased so that neither end shows: it
/// fades an illustration whose top is that sky in from the window above it.
#[must_use]
pub fn sky_fade<'a, Message: 'a>(sky: Color, height: f32) -> cosmic::Element<'a, Message> {
    // Smootherstep, which starts and ends flat, at the eight stops a gradient takes.
    let stops = (0..8u8).map(|stop| {
        let t = f32::from(stop) / 7.0;
        let alpha = t * t * t * (t * (6.0 * t - 15.0) + 10.0);
        ColorStop {
            offset: t,
            color: Color { a: alpha, ..sky },
        }
    });
    // A half turn runs the gradient from the top down.
    let fade = Gradient::Linear(Linear::new(Radians::PI).add_stops(stops));
    container(Space::new())
        .width(Length::Fill)
        .height(height)
        .style(move |_| container_style::Style {
            background: Some(Background::Gradient(fade)),
            ..container_style::Style::default()
        })
        .into()
}

/// A tone, and the bits of the bottom right and bottom left radii.
type Rounding = (bool, [u32; 2]);

/// [`no_apps_rounded`]'s pictures, built when a theme first asks for them: a theme rounds its corners
/// one of a few ways, so this holds a few.
static ROUNDED: LazyLock<Mutex<HashMap<Rounding, svg::Handle>>> = LazyLock::new(Mutex::default);

/// The no-apps illustration for a dark or a light theme, below its top [`NO_APPS_FADE`], its bottom corners
/// rounded to follow a card with the theme's corner `radii` (`radius_m`); every request shares one handle per
/// tone and radii.
#[must_use]
pub fn no_apps_rounded(dark: bool, [.., right, left]: [f32; 4]) -> svg::Handle {
    let file = ILLUSTRATIONS[Illustration::NoApps as usize][usize::from(dark)];
    ROUNDED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry((dark, [right, left].map(f32::to_bits)))
        .or_insert_with(|| svg::Handle::from_memory(clipped(file, right, left)))
        .clone()
}

/// `file` below its top [`NO_APPS_FADE`], inside a clip rounded at its bottom corners. The renderers draw an SVG
/// through resvg and ignore the `border_radius` an `Svg` carries, so the rounding has to be in the picture.
fn clipped(file: &[u8], right: f32, left: f32) -> Vec<u8> {
    let Size { width, height } = NO_APPS_SIZE;
    let top = NO_APPS_FADE;
    let shown = height - top;
    let outline = format!(
        "M0 {top}H{width}V{} A{right} {right} 0 0 1 {} {height}H{left} A{left} {left} 0 0 1 0 {}Z",
        height - right,
        width - right,
        height - left,
    );
    let head = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{shown}" viewBox="0 {top} {width} {shown}"><clipPath id="rounded"><path d="{outline}"/></clipPath><g clip-path="url(#rounded)">"#
    );
    [head.as_bytes(), file, b"</g></svg>"].concat()
}

#[cfg(test)]
mod tests {
    use cosmic::iced::advanced::svg::Data;

    use super::*;

    fn assert_parses(svg: &[u8], what: &str) {
        usvg::Tree::from_data(svg, &usvg::Options::default())
            .unwrap_or_else(|e| panic!("{what} does not parse: {e}"));
    }

    const RESOURCES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/resources");
    /// The sky color the illustrations used to be drawn against and then had swapped for the window's.
    const RETIRED_SKY: &str = "#141515";

    /// Each illustration's file in `resources/`, by the tone of theme it is drawn for.
    const ILLUSTRATION_FILES: [(Illustration, bool, &str); 4] = [
        (Illustration::AboutHero, true, "about-hero.svg"),
        (Illustration::AboutHero, false, "about-hero-light.svg"),
        (Illustration::NoApps, true, "no-apps.svg"),
        (Illustration::NoApps, false, "no-apps-light.svg"),
    ];

    fn illustration_file(name: &str) -> Vec<u8> {
        std::fs::read(format!("{RESOURCES}/{name}")).unwrap()
    }

    fn bytes_of(handle: &svg::Handle) -> &[u8] {
        let Data::Bytes(bytes) = handle.data() else {
            panic!("an illustration is held in memory");
        };
        bytes
    }

    #[test]
    fn enum_covers_exactly_the_bundled_files() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/icons/phosphor");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "svg"))
            .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        on_disk.sort();
        let mut in_enum: Vec<&str> = Icon::FILES.iter().map(|(_, file)| *file).collect();
        in_enum.sort_unstable();
        assert_eq!(on_disk, in_enum);
    }

    #[test]
    fn every_icon_parses() {
        for (icon, file) in Icon::FILES {
            assert_parses(icon.svg(), file);
        }
    }

    #[test]
    fn logo_parses() {
        assert_parses(LOGO, "logo");
    }

    #[test]
    fn resources_hold_exactly_the_four_illustrations() {
        let mut on_disk: Vec<String> = std::fs::read_dir(RESOURCES)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "svg"))
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        on_disk.sort();
        let mut named: Vec<&str> = ILLUSTRATION_FILES
            .iter()
            .map(|(_, _, name)| *name)
            .collect();
        named.sort_unstable();
        assert_eq!(on_disk, named);
    }

    #[test]
    fn every_illustration_hazes_its_top_into_the_sky_the_window_fades_it_in_with() {
        for (which, dark, name) in ILLUSTRATION_FILES {
            let svg = illustration_file(name);
            let tree = usvg::Tree::from_data(&svg, &usvg::Options::default())
                .unwrap_or_else(|e| panic!("{name} does not parse: {e}"));
            let haze = tree
                .linear_gradients()
                .iter()
                .find(|gradient| gradient.id() == "haze")
                .unwrap_or_else(|| panic!("{name} has no haze at its top"));
            let [r, g, b, _] = sky(which, dark).into_rgba8();
            let [from, .., to] = haze.stops() else {
                panic!("{name}'s haze has too few stops");
            };
            assert_eq!(
                (from.color(), from.opacity()),
                (usvg::Color::new_rgb(r, g, b), usvg::Opacity::ONE),
                "{name}'s haze does not start at the window's sky"
            );
            assert_eq!(
                to.opacity(),
                usvg::Opacity::ZERO,
                "{name}'s haze does not clear"
            );
            let text = String::from_utf8(svg).unwrap();
            assert!(
                !text.contains(RETIRED_SKY),
                "{name} still has {RETIRED_SKY}"
            );
            assert!(!text.contains("mask"), "{name} fades itself in");
        }
    }

    #[test]
    fn the_theme_picks_the_file_drawn_for_its_tone() {
        for (which, dark, name) in ILLUSTRATION_FILES {
            assert!(
                bytes_of(&illustration(which, dark)) == illustration_file(name),
                "{which:?} dark={dark} is not {name}"
            );
        }
    }

    #[test]
    fn the_no_apps_size_is_that_of_its_files_and_their_rounded_copies_leave_the_fade_out() {
        for dark in [true, false] {
            let file = illustration(Illustration::NoApps, dark);
            let tree = usvg::Tree::from_data(bytes_of(&file), &usvg::Options::default())
                .unwrap_or_else(|e| panic!("dark={dark} does not parse: {e}"));
            assert_eq!(
                (tree.size().width(), tree.size().height()),
                (NO_APPS_SIZE.width, NO_APPS_SIZE.height),
                "dark={dark}"
            );
            // The rounded copy leaves its top to the window's fade.
            let rounded = no_apps_rounded(dark, [16.0; 4]);
            let tree = usvg::Tree::from_data(bytes_of(&rounded), &usvg::Options::default())
                .unwrap_or_else(|e| panic!("dark={dark} does not parse: {e}"));
            assert_eq!(
                (tree.size().width(), tree.size().height()),
                (NO_APPS_SIZE.width, NO_APPS_SIZE.height - NO_APPS_FADE),
                "dark={dark}, rounded"
            );
        }
    }

    #[test]
    fn every_request_for_a_rounded_illustration_shares_one_handle_per_tone_and_radii() {
        let (round, less_round) = ([16.0; 4], [8.0; 4]);
        for dark in [true, false] {
            let (first, second) = (no_apps_rounded(dark, round), no_apps_rounded(dark, round));
            assert!(
                std::ptr::eq(first.data(), second.data()),
                "dark={dark} was built again"
            );
            assert!(
                !std::ptr::eq(first.data(), no_apps_rounded(dark, less_round).data()),
                "dark={dark} drew the same picture for another radius"
            );
        }
        assert!(
            !std::ptr::eq(
                no_apps_rounded(true, round).data(),
                no_apps_rounded(false, round).data()
            ),
            "both tones drew the same picture"
        );
    }

    #[test]
    fn every_request_for_an_illustration_shares_one_handle() {
        for (which, dark, _) in ILLUSTRATION_FILES {
            let (first, second) = (illustration(which, dark), illustration(which, dark));
            assert!(
                std::ptr::eq(first.data(), second.data()),
                "{which:?} dark={dark} was built again"
            );
        }
    }

    #[test]
    fn an_apps_icon_is_drawn_by_theme_name_or_from_its_file_or_not_at_all() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("app.png");
        std::fs::write(&file, ONE_PIXEL_PNG).unwrap();
        assert_eq!(app_icon_of("firefox"), AppIcon::Named("firefox"));
        let shown = file.display().to_string();
        assert_eq!(app_icon_of(&shown), AppIcon::File(file.as_path()));
        let missing = root.path().join("gone.png").display().to_string();
        assert_eq!(app_icon_of(&missing), AppIcon::Missing);
    }

    #[test]
    fn an_icon_file_that_cannot_be_read_is_drawn_as_the_generic_icon() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("locked.png");
        std::fs::write(&file, ONE_PIXEL_PNG).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(&file).is_ok() {
            // A process that reads past permissions, such as root's, cannot show this.
            return;
        }
        assert_eq!(app_icon_of(&file.display().to_string()), AppIcon::Missing);
    }

    #[test]
    fn an_icon_path_to_a_pipe_is_drawn_as_the_generic_icon_at_once() {
        let root = tempfile::tempdir().unwrap();
        let pipe = root.path().join("icon.png");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &pipe,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        // Nothing writes to the pipe, so a plain open of it never returns.
        let (sender, answer) = std::sync::mpsc::channel();
        let value = pipe.display().to_string();
        std::thread::spawn(move || {
            let _ = sender.send(app_icon_of(&value) == AppIcon::Missing);
        });
        assert_eq!(
            answer.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(true)
        );
    }

    /// A PNG of one transparent pixel.
    const ONE_PIXEL_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
}
