//! The app drawer: what it shows for an app, which of its color popovers is open, and how it is drawn.

use cosmic::iced::widget::container as container_style;
use cosmic::iced::widget::text::{Style as TextStyle, Wrapping};
use cosmic::iced::{Background, Border, Color, Length, Point, mouse};
use cosmic::widget::{self, button, column, container, popover, row, settings, text};
use cosmic::{Element, theme};

use super::AppRow;
use super::pages::chip;
use super::view::muted;
use super::window::Message;
use crate::colors::{self, ColorStore, Overrides, PALETTE};
use crate::fl;
use crate::icons::{self, Icon};
use crate::picker::keycap;
use crate::profiles::Rgb;

/// The width COSMIC's own drawers use, which keeps the roomy inset.
const DRAWER_WIDTH: f32 = 392.0;
const GRID_COLUMNS: usize = 8;
const DOT: f32 = 20.0;
const RING: f32 = 2.0;
const RING_GAP: f32 = 2.0;
const OUTLINE: f32 = 1.0;
/// A dot with room for its ring, so a ringed dot and a plain one lay out alike.
const DISK: f32 = DOT + 2.0 * (RING + RING_GAP);
const CELL_PADDING: f32 = 2.0;
const CELL: f32 = DISK + 2.0 * CELL_PADDING;
/// The colour palette's popup.
pub(super) static PALETTE_POPUP: std::sync::LazyLock<widget::Id> =
    std::sync::LazyLock::new(|| widget::Id::new("signpost-palette"));

/// The open drawer. It names its app by id, so a refresh re-resolves the app instead of holding a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Drawer {
    pub(super) app: String,
    /// The key of the profile whose color popover is open.
    pub(super) swatch: Option<String>,
    /// Why the last color change was not saved.
    error: Option<String>,
}

/// What a profile swatch or its popover asks for. `Close` names the popover that closed itself, `Dismiss`
/// is Esc and closes whichever is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Swatch {
    Open(String),
    Close(String),
    Choose(Option<Rgb>),
    Dismiss,
}

pub(super) struct ProfileLine<'a> {
    pub(super) key: &'a str,
    pub(super) label: &'a str,
    pub(super) color: Option<Rgb>,
    pub(super) keycap: Option<u8>,
}

pub(super) struct Content<'a> {
    pub(super) caption: String,
    pub(super) profiles: Vec<ProfileLine<'a>>,
    pub(super) menu: Vec<(String, String)>,
}

/// What the drawer shows for `app`: each profile with the color it is shown in, and the entries its tiles
/// offer in the picker menu.
pub(super) fn content<'a>(app: &'a AppRow, overrides: &Overrides) -> Content<'a> {
    let kind = if app.flatpak {
        fl!("app-flatpak")
    } else {
        fl!("app-native")
    };
    let private = app
        .private
        .then(|| (fl!("menu-private"), fl!("menu-available")));
    let actions = app
        .desktop_actions
        .iter()
        .map(|name| (name.clone(), fl!("menu-desktop-action")));
    Content {
        caption: fl!(
            "drawer-caption",
            id = format!("{}.desktop", app.id),
            kind = kind
        ),
        profiles: app
            .profiles
            .iter()
            .map(|profile| {
                let chosen = overrides.get(&colors::override_key(&app.id, &profile.key));
                ProfileLine {
                    key: &profile.key,
                    label: &profile.label,
                    color: colors::effective(profile.seed, chosen),
                    keycap: profile.tile.and_then(|tile| keycap(tile - 1)),
                }
            })
            .collect(),
        menu: private.into_iter().chain(actions).collect(),
    }
}

impl Drawer {
    pub(super) fn open(app: String) -> Self {
        Self {
            app,
            swatch: None,
            error: None,
        }
    }

    /// The drawer against a refreshed `inventory`: gone with its app, and its popover gone with its profile.
    pub(super) fn follow(mut self, inventory: &[AppRow]) -> Option<Self> {
        let app = inventory.iter().find(|app| app.id == self.app)?;
        self.swatch
            .take_if(|open| !app.profiles.iter().any(|profile| profile.key == *open));
        Some(self)
    }

    pub(super) fn update(
        &mut self,
        swatch: Swatch,
        store: Option<&ColorStore>,
        overrides: &mut Overrides,
    ) {
        match swatch {
            Swatch::Open(profile) => self.swatch = Some(profile),
            // Pressing swatch B while A is open reports "open B" and "close A" in widget order; a close
            // counts only for the popover that is open, so B stays open in either order.
            Swatch::Close(profile) => {
                self.swatch.take_if(|open| *open == profile);
            }
            Swatch::Dismiss => self.swatch = None,
            Swatch::Choose(color) => self.choose(color, store, overrides),
        }
    }

    fn choose(
        &mut self,
        color: Option<Rgb>,
        store: Option<&ColorStore>,
        overrides: &mut Overrides,
    ) {
        let Some(profile) = self.swatch.take() else {
            return;
        };
        let key = colors::override_key(&self.app, &profile);
        self.error = save(store, overrides, &key, color)
            .err()
            .map(|reason| fl!("color-save-failed", reason = reason));
    }
}

fn save(
    store: Option<&ColorStore>,
    overrides: &mut Overrides,
    key: &str,
    color: Option<Rgb>,
) -> Result<(), String> {
    let store = store.ok_or_else(|| fl!("color-store-missing"))?;
    overrides
        .set(store, key, color)
        .map_err(|error| error.to_string())
}

/// The surface a dot sits on, which decides whether it needs an outline.
type Surface = fn(&cosmic::Theme) -> Color;

fn list_surface(theme: &cosmic::Theme) -> Color {
    theme.current_container().component.base.into()
}

fn popup_surface(theme: &cosmic::Theme) -> Color {
    theme.cosmic().primary(false).base.into()
}

fn paint(color: Rgb) -> Color {
    Color::from_rgb8(color.r, color.g, color.b)
}

fn rgb_of(color: Color) -> Rgb {
    let [r, g, b, _] = color.into_rgba8();
    Rgb { r, g, b }
}

fn hex(color: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn warning(theme: &cosmic::Theme) -> TextStyle {
    TextStyle {
        color: Some(theme.cosmic().warning_text_color().into()),
        ..Default::default()
    }
}

/// A profile color as a filled circle, outlined in the text color when it would melt into `surface`;
/// no color is an empty ring. `ring` circles it in that color, for the color in use.
fn disk<'a>(color: Option<Rgb>, ring: Option<Rgb>, surface: Surface) -> Element<'a, Message> {
    let dot = container(widget::Space::new())
        .width(Length::Fixed(DOT))
        .height(Length::Fixed(DOT))
        .class(theme::Container::custom(move |theme| {
            dot_style(color, surface(theme), theme)
        }));
    container(dot)
        .padding(RING + RING_GAP)
        .class(theme::Container::custom(move |theme| {
            ring_style(ring, surface(theme), theme)
        }))
        .into()
}

fn dot_style(color: Option<Rgb>, surface: Color, theme: &cosmic::Theme) -> container_style::Style {
    let outlined = color.is_none_or(|color| colors::needs_outline(color, rgb_of(surface)));
    container_style::Style {
        background: color.map(|color| Background::Color(paint(color))),
        border: Border {
            color: theme.current_container().component.on.into(),
            width: if outlined { OUTLINE } else { 0.0 },
            radius: (DOT / 2.0).into(),
        },
        ..Default::default()
    }
}

fn ring_style(ring: Option<Rgb>, surface: Color, theme: &cosmic::Theme) -> container_style::Style {
    let ink: Color = theme.current_container().component.on.into();
    let visible = ring.map(|color| {
        if colors::needs_outline(color, rgb_of(surface)) {
            ink
        } else {
            paint(color)
        }
    });
    container_style::Style {
        border: Border {
            color: visible.unwrap_or(Color::TRANSPARENT),
            width: RING,
            radius: (DISK / 2.0).into(),
        },
        ..Default::default()
    }
}

/// A dot you press: it opens the profile's color popover, which shows while `open` is its key.
fn swatch<'a>(line: &ProfileLine<'a>, open: Option<&str>) -> Element<'a, Message> {
    let key = line.key.to_owned();
    let pressable = button::custom(disk(line.color, None, list_surface))
        .padding(CELL_PADDING)
        .class(theme::Button::Icon)
        .name(fl!("color-of", profile = line.label))
        .on_press(Message::Swatch(Swatch::Open(key.clone())));
    let cell = popover(pressable)
        .position(popover::Position::Point(Point::new(0.0, CELL)))
        .on_close(Message::Swatch(Swatch::Close(key)));
    if open != Some(line.key) {
        return cell.into();
    }
    cell.popup(grid(line.color)).into()
}

fn choice<'a>(color: Rgb, current: Option<Rgb>) -> Element<'a, Message> {
    let ring = current.filter(|current| *current == color);
    button::custom(disk(Some(color), ring, popup_surface))
        .padding(CELL_PADDING)
        .class(theme::Button::Icon)
        .name(hex(color))
        .on_press(Message::Swatch(Swatch::Choose(Some(color))))
        .into()
}

/// The palette with the color in use ringed, over a full-width Reset. Its `mouse_area` captures a press
/// anywhere on the popup, so one on its padding never reads as a press outside the popover.
fn grid<'a>(current: Option<Rgb>) -> Element<'a, Message> {
    let spacing = theme::spacing();
    let mut rows = column::with_capacity(PALETTE.len() / GRID_COLUMNS);
    for line in PALETTE.chunks(GRID_COLUMNS) {
        rows = rows.push(row::with_children(
            line.iter().map(|color| choice(*color, current)),
        ));
    }
    let reset = button::standard(fl!("reset-color"))
        .leading_icon(icons::handle(Icon::ArrowCounterClockwise))
        .width(Length::Fill)
        .on_press(Message::Swatch(Swatch::Choose(None)));
    let popup = container(
        column::with_capacity(2)
            .spacing(spacing.space_xs)
            .push(rows)
            .push(reset),
    )
    .id(PALETTE_POPUP.clone())
    .padding(spacing.space_xs)
    .class(theme::Container::Dialog(true));
    widget::mouse_area(popup)
        .interaction(mouse::Interaction::Idle)
        .into()
}

fn profile_row<'a>(line: &ProfileLine<'a>, open: Option<&str>) -> Element<'a, Message> {
    let keycap = line.keycap.map(|digit| chip(digit.to_string()));
    settings::item::builder(line.label)
        .description(line.key)
        .icon(swatch(line, open))
        .control(row::with_capacity(1).push_maybe(keycap))
        .into()
}

fn profiles_group<'a>(drawer: &Drawer, lines: &[ProfileLine<'a>]) -> Option<Element<'a, Message>> {
    if lines.is_empty() {
        return None;
    }
    let open = drawer.swatch.as_deref();
    Some(
        settings::section()
            .title(fl!("drawer-profiles"))
            .extend(lines.iter().map(|line| profile_row(line, open)))
            .into(),
    )
}

fn menu_group<'a>(entries: Vec<(String, String)>) -> Option<Element<'a, Message>> {
    if entries.is_empty() {
        return None;
    }
    Some(
        settings::section()
            .title(fl!("drawer-menu"))
            .extend(entries.into_iter().map(|(title, status)| {
                settings::item(title, text::body(status).class(theme::Text::Custom(muted)))
            }))
            .into(),
    )
}

fn failure(reason: &str) -> Element<'_, Message> {
    text::body(reason)
        .class(theme::Text::Custom(warning))
        .wrapping(Wrapping::Word)
        .into()
}

/// `page` with `app`'s drawer over it: a non-modal overlay, so the page behind stays usable.
pub(super) fn view<'a>(
    page: Element<'a, Message>,
    drawer: &'a Drawer,
    app: &'a AppRow,
    overrides: &Overrides,
) -> Element<'a, Message> {
    let Content {
        caption,
        profiles,
        menu,
    } = content(app, overrides);
    let body = column::with_capacity(3)
        .spacing(theme::spacing().space_m)
        .push_maybe(profiles_group(drawer, &profiles))
        .push_maybe(drawer.error.as_deref().map(failure))
        .push_maybe(menu_group(menu));
    widget::context_drawer(
        Some(app.name.as_str().into()),
        None,
        Some(text::caption(caption).width(Length::Fill).into()),
        None,
        Message::CloseAppDrawer,
        page,
        body,
        DRAWER_WIDTH,
    )
    .into()
}

#[cfg(test)]
mod tests;
