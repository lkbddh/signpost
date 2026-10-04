//! Themes as COSMIC builds them when the user has frosted glass on.

use std::sync::Arc;

use cosmic::cosmic_theme::{AlphaMap, BlurStrength, ThemeBuilder};
use cosmic::iced::widget::container::Catalog;
use cosmic::iced::{Background, Color};

fn themed(dark: bool, strength: BlurStrength, alpha_map: AlphaMap) -> cosmic::Theme {
    let builder = if dark {
        ThemeBuilder::dark()
    } else {
        ThemeBuilder::light()
    };
    let theme = ThemeBuilder {
        frosted: strength,
        frosted_windows: true,
        frosted_system_interface: true,
        alpha_map,
        ..builder
    }
    .build();
    cosmic::Theme::system(Arc::new(theme))
}

/// A COSMIC theme of the given tone with every surface kind frosted at `strength`. It is not translucent
/// until a test sets `transparent`, as the app does when the compositor can blur.
pub fn frosted(dark: bool, strength: BlurStrength) -> cosmic::Theme {
    themed(dark, strength, AlphaMap::default())
}

/// A translucent theme of the given tone whose background surface is `alpha` opaque; a low `alpha` shows
/// whatever is drawn behind a surface plainly.
pub fn translucent(dark: bool, alpha: f32) -> cosmic::Theme {
    let alpha_map = AlphaMap {
        medium: alpha,
        ..AlphaMap::default()
    };
    let mut theme = themed(dark, BlurStrength::Medium, alpha_map);
    theme.transparent = true;
    theme
}

/// The user's frosted-glass settings as COSMIC stores them in a theme: for windows, for system interface
/// and for maximized apps.
pub fn settings(
    windows: bool,
    system_interface: bool,
    maximized_apps: bool,
) -> cosmic::cosmic_theme::Theme {
    ThemeBuilder {
        frosted_windows: windows,
        frosted_system_interface: system_interface,
        frosted_maximized_apps: maximized_apps,
        ..ThemeBuilder::dark()
    }
    .build()
}

/// The color `theme` fills a container of `class` with.
pub fn fill(theme: &cosmic::Theme, class: &cosmic::theme::Container<'_>) -> Color {
    let Some(Background::Color(color)) = theme.style(class).background else {
        panic!("the container class is filled with a color");
    };
    color
}
