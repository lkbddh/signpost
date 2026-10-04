//! The picker's view: header, tiles, actions menu, failure banner, no-apps state and the queue row.

use cosmic::iced::core::text::Wrapping;
use cosmic::iced::id::Id;
use cosmic::iced::widget::{Stack, canvas, container as container_style};
use cosmic::iced::{Alignment, Border, Color, Length, Padding, Size, alignment, mouse, window};
use cosmic::widget::{self, button, column, container, row, text};
use cosmic::{Element, theme};

use crate::app::{MENU_AUTOSIZE, MENU_SCROLL, Message, PICKER_AUTOSIZE, PICKER_SCROLL};
use crate::colors::{self, Overrides};
use crate::fl;
use crate::icons::{self, Icon};
use crate::picker::{
    Input, LinkQueue, MenuItem, Picker, QueuedLink, Target, grid_position, keycap, rows,
};
use crate::profiles::{Rgb, Tile, TileAction};

use self::parts::{
    BORDER_WIDTH, DashedOutline, PressShield, Rows, TabCatcher, TileLook, accent_tint, badge_disk,
    badge_style, button_class, card_surface, centered_line, chip, ellipsized, header_button_class,
    hover_color, menu_style, muted, one_line, pill, row_style, sheet_style, shell_edge, shell_fill,
    shell_ink, shell_style, tooltip_below, warning_icon, warning_text,
};

mod parts;

const MAX_WIDTH: f32 = 720.0;
/// The narrowest card whose header still shows a host beside its two buttons.
const MIN_CARD_WIDTH: f32 = 360.0;
const MAX_HEIGHT: f32 = 560.0;
const CARD_PADDING: f32 = 24.0;
const SECTION_GAP: f32 = 16.0;
/// [`SECTION_GAP`] for the grid, which takes whole pixels.
const GRID_GAP: u16 = 16;
const HEADER_HEIGHT: f32 = 42.0;
const QUEUE_ROW_HEIGHT: f32 = 44.0;
/// Fixed rather than from the theme's density, so the grid can reserve the banner's height exactly.
const BANNER_PADDING: f32 = 12.0;
const BANNER_GAP: f32 = 4.0;
/// The failure banner without its details: its padding, title, hint and details toggle. A layout test
/// holds it to the banner as laid out.
const BANNER_HEIGHT: f32 = 94.0;
/// The most height the error text takes inside the banner; it scrolls past this.
const DETAILS_MAX_HEIGHT: f32 = 96.0;
/// What the card keeps above and below the tiles: its padding, the header and the gap under it.
const CARD_CHROME: f32 = 2.0 * CARD_PADDING + HEADER_HEIGHT + SECTION_GAP;
const TILE_WIDTH: f32 = 132.0;
/// Every tile has this height, so grid rows are equal and scroll-to-focus is exact.
pub const TILE_HEIGHT: f32 = 124.0;
/// The space between a tile and its menu.
pub(crate) const MENU_GAP: f32 = SECTION_GAP;
const MENU_PADDING: f32 = 8.0;
const APP_ICON_SIZE: u16 = 48;
const FALLBACK_APP_ICON: &str = "application-x-executable";
const ICON_SIZE: u16 = 16;
/// How far the badge's halo reaches past the icon's right and bottom edges.
const BADGE_OVERHANG: f32 = 6.0;
const KEYCAP_INSET: f32 = 8.0;
const HEADER_BADGE_SIZE: f32 = 36.0;
const TINT_ALPHA: f32 = 0.18;
const BANNER_OUTLINE_ALPHA: f32 = 0.4;
/// A waiting link shows as a band below the card, so at most this many.
const MAX_SHEETS: usize = 2;
/// How tall each band is, and how far it is inset at each side from the one above it (the card, for the first).
const SHEET_STEP: f32 = 16.0;
/// The no-apps illustration is drawn at this size; the card is as wide, so it fits exactly.
const ILLUSTRATION_SIZE: Size = icons::NO_APPS_SIZE;
const NO_APPS_CARD_HEIGHT: f32 = 212.0;
/// Card left below the no-apps panel and its section gap for the illustration's ground.
const NO_APPS_SCENE: f32 = 96.0;
/// A small count as a float, for height arithmetic.
fn to_f32(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

/// What the header shows of a link.
#[derive(Debug, PartialEq, Eq)]
pub struct LinkParts {
    pub secure: bool,
    /// The host, with the port unless it is the scheme's default; never userinfo.
    pub host: String,
    /// Path, query and fragment; empty for a bare `/`.
    pub rest: String,
}

#[must_use]
pub fn link_parts(uri: &str) -> LinkParts {
    let Ok(url) = url::Url::parse(uri) else {
        return LinkParts {
            secure: false,
            host: uri.to_owned(),
            rest: String::new(),
        };
    };
    let host = url.host_str().unwrap_or_default();
    let rest = &url[url::Position::BeforePath..];
    LinkParts {
        secure: url.scheme() == "https",
        host: url
            .port()
            .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}")),
        rest: if rest == "/" {
            String::new()
        } else {
            rest.to_owned()
        },
    }
}

/// The queue row's text for the next link.
#[must_use]
pub fn host_and_path(uri: &str) -> String {
    let LinkParts { host, rest, .. } = link_parts(uri);
    format!("{host}{rest}")
}

/// The color a profile tile shows: the user's choice, else the browser's own.
#[must_use]
pub fn effective_color(tile: &Tile, overrides: &Overrides) -> Option<Rgb> {
    let profile = tile.profile.as_ref()?;
    let chosen = overrides.get(&colors::override_key(&tile.app_id, &profile.key));
    colors::effective(profile.color, chosen)
}

/// A badge tells the profiles of one app apart, so it needs at least two of them.
const MIN_BADGED_PROFILES: usize = 2;

/// The color of tile `index`'s badge: only a profile of an app with two or more profile tiles, and
/// only when it has a color.
#[must_use]
pub fn badge_color(tiles: &[Tile], index: usize, overrides: &Overrides) -> Option<Rgb> {
    let tile = tiles.get(index)?;
    let siblings = tiles
        .iter()
        .filter(|other| other.profile.is_some() && other.app_id == tile.app_id)
        .count();
    if siblings < MIN_BADGED_PROFILES {
        return None;
    }
    effective_color(tile, overrides)
}

/// The tint at the foot of a hovered or focused tile: the profile's color, else a neutral.
#[must_use]
pub fn tint(color: Option<Rgb>, neutral: Color) -> Color {
    color.map_or(neutral, |Rgb { r, g, b }| {
        Color::from_rgba8(r, g, b, TINT_ALPHA)
    })
}

#[must_use]
pub fn waiting_label(count: usize) -> String {
    fl!("links-waiting", count = count)
}

#[must_use]
pub fn failure_title(failed: Option<&Target>) -> String {
    failed.map_or_else(
        || fl!("failure-title-unnamed"),
        |target| fl!("failure-title", tile = target.label.clone()),
    )
}

/// The card is as wide as its widest row of tiles, but never narrower than its header needs; with no
/// tiles it is as wide as the illustration behind its message. Without a fixed width the header's
/// `Fill` title would take all of [`MAX_WIDTH`].
#[must_use]
pub fn card_width(picker: &Picker) -> f32 {
    let Some(columns) = rows(&picker.tiles).iter().map(Vec::len).max() else {
        return ILLUSTRATION_SIZE.width;
    };
    let grid = to_f32(columns) * TILE_WIDTH + to_f32(columns.saturating_sub(1)) * SECTION_GAP;
    (grid + 2.0 * CARD_PADDING).max(MIN_CARD_WIDTH)
}

/// The tile's container, which the app asks where it is to anchor the actions menu.
pub(crate) fn tile_id(index: usize) -> Id {
    Id::new(format!("signpost-picker-tile-{index}"))
}

/// The most height the failure banner takes, if there is one: its parts, and the cap of the details
/// while they are open.
fn banner_height(picker: &Picker) -> Option<f32> {
    let details = if picker.details_open {
        BANNER_GAP + DETAILS_MAX_HEIGHT
    } else {
        0.0
    };
    picker.error.is_some().then_some(BANNER_HEIGHT + details)
}

/// How many bands show below the card for `waiting` links.
fn shown_sheets(waiting: usize) -> usize {
    waiting.min(MAX_SHEETS)
}

/// The most height the card may take: the cap, less the bands below it.
fn card_budget(waiting: usize) -> f32 {
    MAX_HEIGHT - SHEET_STEP * to_f32(shown_sheets(waiting))
}

/// The most the card's body may take, a tile grid or the no-apps panel and its ground: the rest of the
/// card, less the rows fixed around it. Without the cap a tall grid would take every pixel and leave the
/// queue row none.
#[must_use]
pub fn body_max_height(picker: &Picker, waiting: usize) -> f32 {
    let banner = banner_height(picker);
    let queue = (waiting > 0).then_some(QUEUE_ROW_HEIGHT);
    let fixed: f32 = banner
        .into_iter()
        .chain(queue)
        .map(|height| height + SECTION_GAP)
        .sum();
    card_budget(waiting) - CARD_CHROME - fixed
}

/// The height of the no-apps panel and the room left under it for the desert's ground, from what the card
/// leaves its body. The ground yields first, then the panel; the desert keeps its bottom on the card's
/// bottom edge.
fn no_apps_heights(picker: &Picker, waiting: usize) -> (f32, f32) {
    let room = body_max_height(picker, waiting);
    let ground = (room - NO_APPS_CARD_HEIGHT).clamp(0.0, SECTION_GAP + NO_APPS_SCENE);
    ((room - ground).min(NO_APPS_CARD_HEIGHT), ground)
}

#[must_use]
pub fn menu_icon(item: &MenuItem) -> Icon {
    match item {
        MenuItem::Open => Icon::ArrowSquareOut,
        MenuItem::Action(TileAction::Private) => Icon::Detective,
        MenuItem::Action(TileAction::Desktop { .. }) => Icon::AppWindow,
        MenuItem::OpenKeep => Icon::PushPin,
        MenuItem::Copy => Icon::Copy,
    }
}

fn menu_label(item: &MenuItem) -> String {
    match item {
        MenuItem::Open => fl!("open"),
        MenuItem::Action(TileAction::Desktop { label, .. }) => label.clone(),
        MenuItem::Action(TileAction::Private) => fl!("private-window"),
        MenuItem::OpenKeep => fl!("open-keep"),
        MenuItem::Copy => fl!("copy-link"),
    }
}

/// Vertical scroll position (0 top, 1 bottom) that shows row `row` of `rows` equal rows: at `r / (n - 1)`
/// row `r` lies wholly inside any viewport at least one row tall.
#[must_use]
pub fn scroll_fraction(row: usize, rows: usize) -> f32 {
    let last = u16::try_from(rows.saturating_sub(1)).unwrap_or(u16::MAX);
    if last == 0 {
        return 0.0;
    }
    let row = u16::try_from(row).unwrap_or(u16::MAX).min(last);
    f32::from(row) / f32::from(last)
}

/// The scroll position that shows the picker's keyboard focus: the focused menu row while the menu is
/// open, else the focused grid row. Exact because the rows of the grid or of the menu are as tall as each
/// other ([`TILE_HEIGHT`] for a tile, [`Rows`] for a menu row) and the gaps constant.
// A known limit: the separator before Copy makes the menu's last row start 9 px lower than equal rows would;
// the last row scrolls to the end anyway, so only a viewport of one row could cut another a little.
#[must_use]
pub fn focus_scroll(picker: &Picker) -> f32 {
    if picker.menu.is_some() {
        return scroll_fraction(picker.menu_focus, picker.menu_items().len());
    }
    grid_scroll(picker)
}

/// The scroll position that shows the focused tile, whether or not a menu is open.
#[must_use]
pub fn grid_scroll(picker: &Picker) -> f32 {
    let rows = rows(&picker.tiles);
    let row = grid_position(&rows, picker.focus).map_or(0, |(row, _)| row);
    scroll_fraction(row, rows.len())
}

/// The card with a band below it for each waiting link, each narrower than the one above. The card gets
/// the cap less the bands, whatever it holds, so the bands lie below it and never behind it and a
/// translucent card looks the same whatever waits. They have nothing to press.
fn with_sheets(card: Element<'_, Message>, waiting: usize) -> Element<'_, Message> {
    let sheets = shown_sheets(waiting);
    if sheets == 0 {
        return card;
    }
    let edged = sheets > 1;
    let base = column::with_capacity(2)
        .push(container(card).max_height(card_budget(waiting)))
        .push(widget::Space::new().height(SHEET_STEP * to_f32(sheets)));
    (1..=sheets)
        .fold(Stack::new().push(base), |stack, level| {
            let band = container(widget::Space::new())
                .width(Length::Fill)
                .height(Length::Fixed(SHEET_STEP))
                .class(theme::Container::custom(move |theme| {
                    sheet_style(theme, edged)
                }));
            let inset = SHEET_STEP * to_f32(level);
            let slot = container(band)
                .padding(Padding {
                    right: inset,
                    bottom: SHEET_STEP * to_f32(sheets - level),
                    left: inset,
                    ..Padding::ZERO
                })
                .align_y(alignment::Vertical::Bottom)
                .width(Length::Fill)
                .height(Length::Fill);
            stack.push_under(slot)
        })
        .into()
}

/// Everything the picker's view reads.
#[derive(Clone, Copy)]
pub struct View<'a> {
    pub id: window::Id,
    pub picker: &'a Picker,
    pub colors: &'a Overrides,
    pub queue: &'a LinkQueue,
    /// Whether the theme is dark, which picks the picture behind the no-apps card.
    pub dark: bool,
    /// The theme's `radius_m`, which the corners of that picture follow.
    pub radius: [f32; 4],
}

impl<'a> View<'a> {
    pub fn render(self) -> Element<'a, Message> {
        let card = self.dismissing(self.card());
        widget::autosize::autosize(
            with_sheets(card, self.queue.waiting()),
            PICKER_AUTOSIZE.clone(),
        )
        .max_width(MAX_WIDTH)
        .max_height(MAX_HEIGHT)
        .into()
    }

    fn message(self, input: Input) -> Message {
        Message::Tile(self.id, input)
    }

    /// While the menu is open a press of any button, or a touch, on the card closes it and does nothing else:
    /// a shield over the card takes the press before the card's widgets can. The card is a stack with or
    /// without the shield, so its widgets keep their state.
    fn dismissing(self, card: Element<'a, Message>) -> Element<'a, Message> {
        let stack = Stack::new().push(card);
        if self.picker.menu.is_none() {
            return stack.into();
        }
        let shield = canvas(PressShield { id: self.id })
            .width(Length::Fill)
            .height(Length::Fill);
        stack.push(shield).into()
    }

    fn card(self) -> Element<'a, Message> {
        let waiting = self.queue.waiting();
        let mut content = column::with_capacity(5)
            .spacing(SECTION_GAP)
            .push(self.header());
        if let Some(error) = &self.picker.error {
            content = content.push(self.banner(error));
        }
        content = content.push(self.body(waiting));
        if let Some(next) = self.queue.peek() {
            content = content.push(self.queue_row(next, waiting));
        }
        let padded = container(content)
            .padding(CARD_PADDING)
            .width(Length::Fixed(card_width(self.picker)));
        if self.picker.tiles.is_empty() {
            let inked = padded.class(theme::Container::custom(shell_ink));
            return scene(inked, self.dark, self.radius);
        }
        padded.class(theme::Container::custom(shell_style)).into()
    }

    fn header(self) -> Element<'a, Message> {
        let spacing = theme::spacing();
        let parts = link_parts(&self.picker.uri);
        let lock = if parts.secure {
            Icon::LockSimple
        } else {
            Icon::LockSimpleOpen
        };
        let badge = container(icons::icon(lock, ICON_SIZE))
            .center(Length::Fixed(HEADER_BADGE_SIZE))
            .class(theme::Container::custom(badge_style));
        let mut path = row::with_capacity(2).spacing(spacing.space_xxs);
        if !parts.secure {
            path =
                path.push(text::body(fl!("not-secure")).class(theme::Text::Custom(warning_text)));
        }
        if !parts.rest.is_empty() {
            path = path.push(one_line(parts.rest).class(theme::Text::Custom(muted)));
        }
        let title = column::with_capacity(2)
            .push(one_line(parts.host).font(cosmic::font::bold()))
            .push(path)
            .width(Length::Fill);
        row::with_capacity(4)
            .spacing(spacing.space_s)
            .align_y(Alignment::Center)
            .height(Length::Fixed(HEADER_HEIGHT))
            .push(badge)
            .push(title)
            .push(self.pin_button())
            .push(self.copy_button())
            .into()
    }

    fn pin_button(self) -> Element<'a, Message> {
        let (class, state) = if self.picker.pinned {
            (theme::Button::Suggested, fl!("pin-on"))
        } else {
            (header_button_class(), fl!("pin-off"))
        };
        let pin = button::icon(icons::handle(Icon::PushPin))
            .name(state)
            .class(class)
            .on_press(self.message(Input::TogglePin));
        tooltip_below(pin, fl!("pin-tooltip"))
    }

    fn copy_button(self) -> Element<'a, Message> {
        let (glyph, label) = if self.picker.copied.is_some() {
            (Icon::Check, fl!("link-copied"))
        } else {
            (Icon::Copy, fl!("copy-link"))
        };
        let copy = button::icon(icons::handle(glyph))
            .name(label.clone())
            .class(header_button_class())
            .on_press(self.message(Input::Copy));
        tooltip_below(copy, label)
    }

    fn banner(self, error: &'a str) -> Element<'a, Message> {
        let details = if self.picker.details_open {
            fl!("hide-details")
        } else {
            fl!("show-details")
        };
        let mut words = column::with_capacity(4)
            .spacing(BANNER_GAP)
            .width(Length::Fill)
            .push(ellipsized(
                text::heading(failure_title(self.picker.failed.as_ref())).width(Length::Fill),
            ))
            .push(one_line(fl!("failure-hint")))
            .push(
                button::link(details)
                    .padding(0)
                    .on_press(self.message(Input::ToggleDetails)),
            );
        if self.picker.details_open {
            let error = text::monotext(error).wrapping(Wrapping::WordOrGlyph);
            let details = widget::scrollable(error).spacing(theme::spacing().space_xxs);
            words = words.push(container(details).max_height(DETAILS_MAX_HEIGHT));
        }
        container(
            row::with_capacity(2)
                .spacing(theme::spacing().space_s)
                .push(warning_icon())
                .push(words),
        )
        .padding(BANNER_PADDING)
        .width(Length::Fill)
        .class(theme::Container::custom(|theme| container_style::Style {
            border: Border {
                color: Color {
                    a: BANNER_OUTLINE_ALPHA,
                    ..theme.cosmic().warning_color().into()
                },
                width: BORDER_WIDTH,
                radius: theme.cosmic().radius_s().into(),
            },
            ..Default::default()
        }))
        .into()
    }

    fn body(self, waiting: usize) -> Element<'a, Message> {
        if self.picker.tiles.is_empty() {
            return self.empty_state(waiting);
        }
        self.grid(waiting)
    }

    fn grid(self, waiting: usize) -> Element<'a, Message> {
        let mut grid = widget::grid()
            .column_spacing(GRID_GAP)
            .row_spacing(GRID_GAP);
        let row_starts: Vec<usize> = rows(&self.picker.tiles)
            .iter()
            .skip(1)
            .filter_map(|row| row.first().copied())
            .collect();
        for (index, tile) in self.picker.tiles.iter().enumerate() {
            if row_starts.contains(&index) {
                grid = grid.insert_row();
            }
            grid = grid.push(self.tile(index, tile));
        }
        let tiles = container(widget::scrollable(grid).id(PICKER_SCROLL.clone()))
            .max_height(body_max_height(self.picker, waiting));
        widget::mouse_area(tiles)
            .on_exit(self.message(Input::HoverClear))
            .into()
    }

    /// Not a `button`: its native Tab stop would drift from `Picker::focus` (Enter launches the model's
    /// tile), and a pinned button captures the press (`button/widget.rs:816`) before the `mouse_area`
    /// that owns it.
    fn tile(self, index: usize, tile: &'a Tile) -> Element<'a, Message> {
        let look = TileLook {
            color: effective_color(tile, self.colors),
            lit: self.picker.menu == Some(index) || self.picker.hover == Some(index),
            ring: self.picker.focus_visible
                && index == self.picker.focus
                && self.picker.menu.is_none(),
        };
        widget::mouse_area(look.surface(tile_id(index), self.tile_face(index, tile)))
            .interaction(mouse::Interaction::Pointer)
            .on_enter(self.message(Input::HoverStart(index)))
            .on_exit(self.message(Input::HoverEnd(index)))
            .on_press(self.message(Input::PressStart(index)))
            .on_release(self.message(Input::PressEnd {
                index,
                keep_open: false,
            }))
            .on_middle_press(self.message(Input::Click {
                index,
                keep_open: true,
            }))
            .on_right_press(self.message(Input::OpenMenu(Some(index))))
            .into()
    }

    /// The icon and label (a profile's or, without one, the app's), and the keycap in the corner.
    fn tile_face(self, index: usize, tile: &'a Tile) -> Element<'a, Message> {
        let spacing = theme::spacing();
        let body = column::with_capacity(2)
            .align_x(Alignment::Center)
            .spacing(spacing.space_xxxs)
            .width(Length::Fill)
            .push(app_icon(
                tile,
                badge_color(&self.picker.tiles, index, self.colors),
            ))
            .push(centered_line(tile.label.clone()));
        let mut face = Stack::new().push(
            container(body)
                .padding([0.0, f32::from(spacing.space_xxs)])
                .center(Length::Fill),
        );
        if let Some(digit) = keycap(index) {
            face = face.push(
                container(chip(digit.to_string()))
                    .padding(KEYCAP_INSET)
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        }
        face.into()
    }

    /// The content of the actions menu's popup: as big as the menu needs, and nothing around it. It scrolls
    /// only past [`MAX_HEIGHT`].
    pub fn menu_popup(self) -> Element<'a, Message> {
        let Some(index) = self.picker.menu else {
            return widget::Space::new().into();
        };
        let mut rows = Vec::new();
        for (position, item) in self.picker.menu_items().into_iter().enumerate() {
            if item.separator_before() {
                rows.push(separator());
            }
            rows.push(self.menu_row(index, position, item));
        }
        let surface =
            container(widget::scrollable(Element::new(Rows::new(rows))).id(MENU_SCROLL.clone()))
                .padding(MENU_PADDING)
                .class(theme::Container::custom(menu_style));
        let tab = canvas(TabCatcher { id: self.id })
            .width(Length::Shrink)
            .height(Length::Shrink);
        widget::autosize::autosize(Stack::new().push(surface).push(tab), MENU_AUTOSIZE.clone())
            .max_height(MAX_HEIGHT)
            .into()
    }

    fn menu_row(self, index: usize, position: usize, item: MenuItem) -> Element<'a, Message> {
        let spacing = theme::spacing();
        let focused = self.picker.focus_visible && position == self.picker.menu_focus;
        let content = row::with_capacity(3)
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center)
            .height(Length::Fill)
            .padding([f32::from(spacing.space_xxs), f32::from(spacing.space_xs)])
            .push(icons::icon(menu_icon(&item), ICON_SIZE))
            .push(
                text::body(menu_label(&item))
                    .wrapping(Wrapping::None)
                    .width(Length::Fill),
            )
            .push(
                row::with_children(item.shortcut().iter().map(|key| chip((*key).to_owned())))
                    .spacing(spacing.space_xxs),
            );
        let rest =
            move |theme: &cosmic::Theme| row_style(theme, focused.then(|| accent_tint(theme)));
        let hover = move |theme: &cosmic::Theme| {
            let lit = if focused {
                accent_tint(theme)
            } else {
                hover_color(theme)
            };
            row_style(theme, Some(lit))
        };
        button::custom(content)
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .class(button_class(rest, hover))
            .on_press(self.message(Input::ChooseItem {
                index,
                item,
                keep_open: false,
            }))
            .into()
    }

    fn no_apps_words(self) -> Element<'a, Message> {
        column::with_capacity(3)
            .align_x(Alignment::Center)
            .spacing(theme::spacing().space_xs)
            .push(text::heading(fl!("no-apps")))
            .push(text::body(fl!("no-apps-body")).class(theme::Text::Custom(muted)))
            .push(
                button::standard(fl!("copy-link"))
                    .leading_icon(icons::handle(Icon::Copy))
                    .on_press(self.message(Input::Copy)),
            )
            .into()
    }

    fn empty_state(self, waiting: usize) -> Element<'a, Message> {
        let (panel, ground) = no_apps_heights(self.picker, waiting);
        let panel = Stack::new()
            .push(
                container(self.no_apps_words())
                    .center(Length::Fill)
                    .class(theme::Container::custom(shell_ink)),
            )
            .push_under(
                canvas(DashedOutline)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fixed(panel));
        container(panel)
            .padding(Padding {
                bottom: ground,
                ..Padding::ZERO
            })
            .into()
    }

    /// The row for the next waiting link; pressing it closes this picker, which shows the next.
    fn queue_row(self, next: &QueuedLink, waiting: usize) -> Element<'a, Message> {
        let spacing = theme::spacing();
        let content = row::with_capacity(4)
            .spacing(spacing.space_s)
            .align_y(Alignment::Center)
            .height(Length::Fill)
            .padding([0.0, f32::from(spacing.space_s)])
            .push(icons::icon(Icon::Stack, ICON_SIZE))
            .push(text::body(fl!("next-link")).class(theme::Text::Custom(muted)))
            .push(one_line(host_and_path(&next.uri)).width(Length::Fill))
            .push(pill(waiting_label(waiting)));
        let skip = button::custom(content)
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fixed(QUEUE_ROW_HEIGHT))
            .class(button_class(
                |theme| row_style(theme, Some(card_surface(theme))),
                |theme| row_style(theme, Some(hover_color(theme))),
            ))
            .on_press(self.message(Input::SkipToNext));
        widget::tooltip(
            skip,
            text::body(fl!("skip-to-next")),
            widget::tooltip::Position::Top,
        )
        .into()
    }
}

/// The tile's icon, bare, with the profile's color badge on its bottom-right corner when it has one.
fn app_icon<'a>(tile: &Tile, badge: Option<Rgb>) -> Element<'a, Message> {
    let shown: Element<'a, Message> = match tile.icon.as_deref().map(icons::app_icon_of) {
        Some(icons::AppIcon::Named(name)) => widget::icon::from_name(name.to_owned())
            .size(APP_ICON_SIZE)
            .into(),
        Some(icons::AppIcon::File(path)) => {
            widget::icon::icon(widget::icon::from_path(path.to_owned()))
                .size(APP_ICON_SIZE)
                .into()
        }
        Some(icons::AppIcon::Missing) | None => widget::icon::from_name(FALLBACK_APP_ICON)
            .size(APP_ICON_SIZE)
            .into(),
    };
    let icon = container(shown).padding(Padding {
        top: 0.0,
        right: BADGE_OVERHANG,
        bottom: BADGE_OVERHANG,
        left: BADGE_OVERHANG,
    });
    let Some(color) = badge else {
        return icon.into();
    };
    Stack::new()
        .push(icon)
        .push(
            container(badge_disk(color))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(alignment::Horizontal::Right)
                .align_y(alignment::Vertical::Bottom),
        )
        .into()
}

fn separator<'a>() -> Element<'a, Message> {
    container(widget::divider::horizontal::light())
        .padding([f32::from(theme::spacing().space_xxxs), 0.0])
        .into()
}

/// The no-apps card over its illustration: the picker's fill, the desert on its bottom edge, rounded
/// as the card is at `radius`, the content, then the picker's edge, so the artwork never covers it.
fn scene<'a>(
    content: impl Into<Element<'a, Message>>,
    dark: bool,
    radius: [f32; 4],
) -> Element<'a, Message> {
    // The picture's top is the window's to fade in, as its file's sky is opaque.
    let picture = widget::svg(icons::no_apps_rounded(dark, radius))
        .width(Length::Fixed(ILLUSTRATION_SIZE.width))
        .height(Length::Fixed(
            ILLUSTRATION_SIZE.height - icons::NO_APPS_FADE,
        ));
    let art = column::with_capacity(2)
        .width(Length::Fixed(ILLUSTRATION_SIZE.width))
        .push(icons::sky_fade(
            icons::sky(icons::Illustration::NoApps, dark),
            icons::NO_APPS_FADE,
        ))
        .push(picture);
    let ground = container(art)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Bottom);
    let layer = |style: fn(&cosmic::Theme) -> container_style::Style| {
        container(widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .class(theme::Container::custom(style))
    };
    Stack::new()
        .push(content)
        .push(layer(shell_edge))
        .push_under(ground)
        .push_under(layer(shell_fill))
        .into()
}

#[cfg(test)]
mod tests;
