//! Which of the user's frosted-glass settings each surface follows.
//!
//! libcosmic sets one translucency for the whole app, from the windows setting. Signpost has two kinds of
//! surface: the settings window, a window, and the picker, a layer-shell overlay that COSMIC treats as
//! system interface. Each gets its own theme and its own compositor blur.

use cosmic::app::Core;
use cosmic::core::Auto;
use cosmic::cosmic_theme;
use cosmic::iced::widget::themer;
use cosmic::surface::action::LiveSettings;
use cosmic::{Element, Theme};

/// The user's three frosted-glass options in COSMIC Settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
struct Options {
    windows: bool,
    system_interface: bool,
    maximized_apps: bool,
}

/// What decides whether a surface is frosted: whether the compositor can blur, and the user's options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Frost {
    blur_supported: bool,
    options: Options,
}

impl Frost {
    /// This, with the options of `theme`.
    #[must_use]
    pub fn following(self, theme: &cosmic_theme::Theme) -> Self {
        Self {
            options: Options {
                windows: theme.frosted_windows,
                system_interface: theme.frosted_system_interface,
                maximized_apps: theme.frosted_maximized_apps,
            },
            ..self
        }
    }

    /// This, once the compositor has said it can blur.
    #[must_use]
    pub fn supported(self) -> Self {
        Self {
            blur_supported: true,
            ..self
        }
    }

    /// The picker is a system-interface overlay, so the system-interface option decides.
    #[must_use]
    pub fn picker(self) -> bool {
        self.blur_supported && self.options.system_interface
    }

    /// The settings window follows the windows option, except while maximized, when it follows the
    /// maximized-apps option too: libcosmic tracks that for a main window, which Signpost has none of.
    #[must_use]
    pub fn settings(self, maximized: bool) -> bool {
        let Options {
            windows,
            maximized_apps,
            ..
        } = self.options;
        self.blur_supported && windows && (maximized_apps || !maximized)
    }
}

/// Leaves the blur of layer-shell surfaces to the app: libcosmic would blur them by the system-interface
/// option alone, and its updates would overwrite the picker's.
pub fn own_layer_blur(core: &mut Core) {
    core.set_auto_blur(Auto::Window | Auto::Popup);
}

/// A surface's live settings: its compositor blur is on or off as `frosted` says, whatever libcosmic's
/// own rules would give.
#[must_use]
pub fn live_settings(frosted: bool) -> LiveSettings {
    LiveSettings {
        blur: Some(frosted),
        ..LiveSettings::default()
    }
}

/// `content` drawn under `theme`, translucent as `frosted` says rather than as the app-wide theme has it.
/// Every widget in it, popovers and tooltips included, reads this theme.
// A known limit: iced's `themer` forwards no `a11y_nodes`, so the content has none. Nothing at this pin sends
// widget trees to accesskit (vendor/iced_winit never calls them); a wrapper that forwards them has to name
// `iced_accessibility::A11yTree`, which needs that crate as a direct dependency.
pub fn scoped<'a, Message: 'a>(
    mut theme: Theme,
    frosted: bool,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    theme.transparent = frosted;
    themer(Some(theme), content).into()
}

#[cfg(test)]
mod tests {
    use cosmic::iced::widget::{Space, container};
    use cosmic::iced::{Length, Point, Size};
    use cosmic::theme;

    use super::*;
    use crate::test_support::frosted::{settings, translucent};
    use crate::test_support::headless::{self, BACKDROP, Frames, is_rounding_apart, over};

    const SCRATCH_HOME: &str = "SIGNPOST_FROST_SCRATCH_HOME";
    const SURFACE: Size = Size::new(40.0, 40.0);
    const FROSTED_ALPHA: f32 = 0.1;

    /// Every combination of the compositor's support and the three settings.
    fn every_frost() -> impl Iterator<Item = Frost> {
        (0..16_u8).map(|bits| Frost {
            blur_supported: bits & 1 != 0,
            options: Options {
                windows: bits & 2 != 0,
                system_interface: bits & 4 != 0,
                maximized_apps: bits & 8 != 0,
            },
        })
    }

    fn supported() -> impl Iterator<Item = Frost> {
        every_frost().filter(|frost| frost.blur_supported)
    }

    #[test]
    fn nothing_frosts_while_the_compositor_cannot_blur() {
        for frost in every_frost().filter(|frost| !frost.blur_supported) {
            assert!(!frost.picker(), "{frost:?}");
            assert!(!frost.settings(false), "{frost:?}");
            assert!(!frost.settings(true), "{frost:?}");
        }
    }

    #[test]
    fn the_picker_follows_the_system_interface_setting_alone() {
        for frost in supported() {
            assert_eq!(frost.picker(), frost.options.system_interface, "{frost:?}");
        }
    }

    #[test]
    fn the_settings_window_follows_the_windows_setting() {
        for frost in supported() {
            assert_eq!(frost.settings(false), frost.options.windows, "{frost:?}");
        }
    }

    #[test]
    fn a_maximized_settings_window_frosts_only_where_maximized_apps_do() {
        for frost in supported() {
            assert_eq!(
                frost.settings(true),
                frost.options.windows && frost.options.maximized_apps,
                "{frost:?}"
            );
        }
    }

    #[test]
    fn following_a_theme_takes_its_three_options_and_keeps_the_compositor_support() {
        for frost in every_frost() {
            for (windows, system_interface, maximized_apps) in [
                (true, false, false),
                (false, true, false),
                (false, false, true),
                (false, false, false),
            ] {
                let followed =
                    frost.following(&settings(windows, system_interface, maximized_apps));
                assert_eq!(
                    followed,
                    Frost {
                        blur_supported: frost.blur_supported,
                        options: Options {
                            windows,
                            system_interface,
                            maximized_apps
                        }
                    }
                );
            }
        }
    }

    #[test]
    fn the_compositor_announcing_blur_keeps_the_options() {
        let followed = Frost::default().following(&settings(true, false, true));
        assert_eq!(
            followed.supported(),
            Frost {
                blur_supported: true,
                ..followed
            }
        );
    }

    /// A filled `Container::Background` across the surface, in the theme of the scope around it.
    fn background<'a>() -> Element<'a, ()> {
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .class(theme::Container::Background)
            .into()
    }

    #[test]
    fn a_scope_decides_the_translucency_of_its_surfaces_whatever_theme_is_drawn() {
        if !headless::in_scratch_home(
            SCRATCH_HOME,
            "frost::tests::a_scope_decides_the_translucency_of_its_surfaces_whatever_theme_is_drawn",
        ) {
            return;
        }
        let middle = Point::new(SURFACE.width / 2.0, SURFACE.height / 2.0);
        let mut frames = Frames::new(SURFACE);
        for dark in [true, false] {
            let theme = translucent(dark, FROSTED_ALPHA);
            let draw_time = if dark { Theme::light() } else { Theme::dark() };
            for frosted in [true, false] {
                let drawn = frames.drawn(scoped(theme.clone(), frosted, background()), &draw_time);
                let fill = theme.cosmic().background(frosted).base.into();
                let expected = over(fill, BACKDROP);
                let seen = drawn.at(middle);
                assert!(
                    is_rounding_apart(seen, expected),
                    "dark={dark}, frosted={frosted}: drawn {seen:?}, the scope's theme gives {expected:?}"
                );
            }
        }
    }
}
