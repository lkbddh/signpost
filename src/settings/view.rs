//! The settings window's chrome and main page.

use std::collections::HashMap;

use cosmic::cosmic_theme::palette::Srgba;
use cosmic::iced::widget::text::{Style as TextStyle, Wrapping};
use cosmic::iced::widget::{container as container_style, svg as svg_style};
use cosmic::iced::{Alignment, Background, Border, Color, Length, Padding, window};
use cosmic::widget::{self, button, column, container, icon, list, menu, row, settings, text};
use cosmic::{Element, theme};

use super::window::{Link, Message, Page, Titlebar, Window};
use super::{Action, AppRow, ErrorRow, HandlerView, drawer, pages};
use crate::colors::Overrides;
use crate::fl;
use crate::icons::{self, Icon};

/// Space kept free under the scrolled content: the toaster floats at the bottom of the window and
/// must never cover the last control (a toast is about 48 px tall, 15 px above the edge).
const TOAST_CLEARANCE: u16 = 72;
const PAGE_MAX_WIDTH: f32 = 800.0;
const APP_ICON_SIZE: u16 = 24;
const ROW_ICON_SIZE: u16 = 20;
const CARET_SIZE: u16 = 16;
/// Between a row's title and the lines under it, as `settings::item` spaces them.
const LINE_SPACING: u16 = 2;
const MUTED_ALPHA: f32 = 0.75;
const ERROR_TINT_ALPHA: f32 = 0.12;
const MENU_ITEM_HEIGHT: u16 = 40;
const MENU_WIDTH: u16 = 240;

/// Centered, at most [`PAGE_MAX_WIDTH`] wide, inset by `gutter` on both sides and by `[top, bottom]`.
pub(super) fn page_content<'a>(
    content: impl Into<Element<'a, Message>>,
    gutter: u16,
    [top, bottom]: [u16; 2],
) -> Element<'a, Message> {
    container(
        container(content)
            .max_width(PAGE_MAX_WIDTH)
            .width(Length::Fill),
    )
    .center_x(Length::Fill)
    .padding([top, gutter, bottom, gutter])
    .into()
}

/// The page body scrolls; its insets sit inside the scrollbar, which stays at the window edge.
pub(super) fn scroll<'a>(
    content: impl Into<Element<'a, Message>>,
    gutter: u16,
    top: u16,
) -> Element<'a, Message> {
    let bottom = theme::spacing().space_m + TOAST_CLEARANCE;
    widget::scrollable(page_content(content, gutter, [top, bottom]))
        .height(Length::Fill)
        .into()
}

/// An icon in a theme color, for the ones that carry meaning (warning, link).
pub(super) fn tinted(
    glyph: Icon,
    size: u16,
    color: fn(&cosmic::cosmic_theme::Theme) -> Srgba,
) -> Element<'static, Message> {
    icons::icon(glyph, size)
        .class(theme::Svg::custom(move |theme| svg_style::Style {
            color: Some(color(theme.cosmic()).into()),
        }))
        .into()
}

pub(super) fn muted(theme: &cosmic::Theme) -> TextStyle {
    let mut color = theme.current_container().component.on;
    color.alpha *= MUTED_ALPHA;
    TextStyle {
        color: Some(color.into()),
        ..Default::default()
    }
}

fn app_icon(name: Option<&str>, size: u16) -> Element<'static, Message> {
    match name.map(icons::app_icon_of) {
        Some(icons::AppIcon::Named(name)) => icon::from_name(name.to_owned()).size(size).into(),
        Some(icons::AppIcon::File(path)) => icon::icon(icon::from_path(path.to_owned()))
            .size(size)
            .into(),
        Some(icons::AppIcon::Missing) | None => icons::icon(Icon::AppWindow, size).into(),
    }
}

fn profile_count(profiles: usize) -> Option<String> {
    (profiles > 0).then(|| fl!("profile-count", count = profiles))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileItem {
    Shortcuts,
    About,
}

impl menu::Action for FileItem {
    type Message = Message;

    fn message(&self) -> Message {
        match self {
            Self::Shortcuts => Message::Go(Page::Shortcuts),
            Self::About => Message::Go(Page::About),
        }
    }
}

/// The File menu's entries, in order.
fn file_items() -> Vec<(String, FileItem)> {
    vec![
        (fl!("keyboard-shortcuts"), FileItem::Shortcuts),
        (fl!("about-signpost"), FileItem::About),
    ]
}

fn file_menu(id: window::Id) -> Element<'static, Message> {
    let root: Element<'static, Message> = menu::root(fl!("file")).into();
    let items = menu::items(
        &HashMap::new(),
        file_items()
            .into_iter()
            .map(|(label, item)| menu::Item::Button(label, None, item))
            .collect(),
    );
    menu::bar(vec![menu::Tree::with_children(root, items)])
        .item_height(menu::ItemHeight::Dynamic(MENU_ITEM_HEIGHT))
        .item_width(menu::ItemWidth::Uniform(MENU_WIDTH))
        .window_id(id)
        .on_surface_action(Message::Surface)
        .into()
}

fn test_link_button() -> Element<'static, Message> {
    let label = fl!("open-test-link");
    widget::tooltip(
        button::icon(icons::handle(Icon::CursorClick))
            .name(label.clone())
            .on_press(Message::OpenLink(Link::Test)),
        text::body(label),
        widget::tooltip::Position::Bottom,
    )
    .into()
}

fn error_tint(radius: [f32; 4]) -> theme::Container<'static> {
    theme::Container::custom(move |theme| {
        let mut tint = Color::from(theme.cosmic().warning_color());
        tint.a = ERROR_TINT_ALPHA;
        container_style::Style {
            background: Some(Background::Color(tint)),
            border: Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
}

impl Window {
    /// Side inset: narrow windows keep less of it.
    pub(super) fn gutter(&self) -> u16 {
        let spacing = theme::spacing();
        if self.condensed {
            return spacing.space_s;
        }
        spacing.space_l
    }

    #[must_use]
    pub fn view(&self, focused: bool, colors: &Overrides) -> Element<'_, Message> {
        let gutter = self.gutter();
        let page = match self.page {
            Page::Main => self.under_drawer(self.main_page(gutter), colors),
            Page::Shortcuts => pages::shortcuts(gutter),
            Page::About => pages::about(&self.hero, self.hero_sky, gutter),
        };
        let content = container(
            column::with_capacity(2)
                .push(self.header(focused))
                .push(page),
        )
        .class(theme::Container::WindowBackground)
        .width(Length::Fill)
        .height(Length::Fill);
        widget::toaster(&self.toasts, content)
    }

    /// `page`, with the open app's drawer over it while that app is still listed. Either way the page
    /// is the first child of a stateless wrapper, so iced keeps its scroll offset across a toggle.
    fn under_drawer<'a>(
        &'a self,
        page: Element<'a, Message>,
        colors: &Overrides,
    ) -> Element<'a, Message> {
        let open = self.drawer.as_ref().and_then(|open| {
            let app = self.inventory.iter().find(|app| app.id == open.app)?;
            Some((open, app))
        });
        let Some((open, app)) = open else {
            return column::with_capacity(1).push(page).into();
        };
        drawer::view(page, open, app, colors)
    }

    fn header(&self, focused: bool) -> Element<'_, Message> {
        let ask = Message::Titlebar;
        let mut bar = widget::header_bar()
            .title(fl!("app-name"))
            .focused(focused)
            .maximized(self.maximized)
            .start(file_menu(self.id()))
            .end(test_link_button())
            .on_close(ask(Titlebar::Close))
            .on_drag(ask(Titlebar::Drag))
            .on_double_click(ask(Titlebar::Maximize))
            .on_right_click(ask(Titlebar::Menu));
        if cosmic::config::show_maximize() {
            bar = bar.on_maximize(ask(Titlebar::Maximize));
        }
        if cosmic::config::show_minimize() {
            bar = bar.on_minimize(ask(Titlebar::Minimize));
        }
        bar.into()
    }

    fn main_page(&self, gutter: u16) -> Element<'_, Message> {
        let spacing = theme::spacing();
        let sections = column::with_capacity(2)
            .spacing(spacing.space_m)
            .push(self.web_links())
            .push(self.apps());
        scroll(sections, gutter, spacing.space_xs)
    }

    fn web_links(&self) -> Element<'_, Message> {
        let radius = theme::active().cosmic().radius_s();
        let errors: Vec<&ErrorRow> = self
            .model
            .record_error
            .iter()
            .chain(&self.model.error_row)
            .collect();
        let last = errors.len().saturating_sub(1);
        let mut rows = widget::list_column().add(self.handler_row());
        for (index, error) in errors.iter().enumerate() {
            let corners = if index == last {
                [0.0, 0.0, radius[2], radius[3]]
            } else {
                [0.0; 4]
            };
            rows = rows
                .list_item_padding(Padding::ZERO)
                .add(self.error_band(error, corners));
        }
        settings::section::with_column(rows).into()
    }

    /// "Web browser", who opens web links now, and the one thing to do about it, in the same places whatever the
    /// state.
    fn handler_row(&self) -> Element<'_, Message> {
        let spacing = theme::spacing();
        let icon: Option<Element<'_, Message>> = match &self.model.handler {
            HandlerView::Signpost => Some(icon::icon(icons::logo()).size(ROW_ICON_SIZE).into()),
            HandlerView::App(app) => Some(app_icon(app.icon.as_deref(), ROW_ICON_SIZE)),
            HandlerView::Split { .. } | HandlerView::None => None,
        };
        let current = row::with_capacity(2)
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center)
            .push_maybe(icon)
            .push(text::body(self.model.handler.value()));
        // A flexing control drops below the title when the window is too narrow for both. The flex row stretches
        // its items to the row's height, which would leave the title at the top.
        settings::item::builder(fl!("default-handler"))
            .flex_control(
                row::with_capacity(2)
                    .spacing(spacing.space_s)
                    .align_y(Alignment::Center)
                    .push(current)
                    .push(self.action_button(self.model.action, self.model.can_run)),
            )
            .align_items(Alignment::Center)
            .into()
    }

    /// Disabled while an operation runs, so one press never starts two, and while `can_run` is false.
    fn action_button(&self, action: Action, can_run: bool) -> Element<'_, Message> {
        if action.is_suggested() {
            button::suggested(action.label())
        } else {
            button::standard(action.label())
        }
        .on_press_maybe((can_run && !self.is_busy()).then_some(Message::Run(action)))
        .into()
    }

    /// The warning, the title and Restore on one line that stays put; the rest opens beneath the title.
    /// A narrow window has Restore beneath too, so the title keeps its room.
    fn error_band(&self, error: &ErrorRow, corners: [f32; 4]) -> Element<'_, Message> {
        let spacing = theme::spacing();
        let restore = error
            .offers_restore
            .then(|| self.action_button(Action::Restore, true));
        let (beside, beneath) = if self.condensed {
            (None, restore)
        } else {
            (restore, None)
        };
        let top = row::with_capacity(3)
            .spacing(spacing.space_s)
            .align_y(Alignment::Center)
            .push(tinted(
                Icon::WarningCircle,
                ROW_ICON_SIZE,
                cosmic::cosmic_theme::Theme::warning_color,
            ))
            .push(
                text::heading(error.title.clone())
                    .wrapping(Wrapping::WordOrGlyph)
                    .width(Length::Fill),
            )
            .push_maybe(beside);
        let below = self.error_words(error, beneath).map(|words| {
            container(words).padding(Padding::ZERO.left(ROW_ICON_SIZE + spacing.space_s))
        });
        container(
            column::with_capacity(2)
                .spacing(LINE_SPACING)
                .push(top)
                .push_maybe(below),
        )
        .padding([spacing.space_xs, spacing.space_m])
        .width(Length::Fill)
        .class(error_tint(corners))
        .into()
    }

    /// The explanation, `action`, the details toggle, and the details once toggled open; `None` when the band
    /// has none of them.
    fn error_words<'a>(
        &'a self,
        error: &ErrorRow,
        action: Option<Element<'a, Message>>,
    ) -> Option<Element<'a, Message>> {
        let hint = error
            .body
            .as_ref()
            .map(|body| text::body(body.clone()).wrapping(Wrapping::Word));
        let disclosure = self.disclosure(error);
        let details: Vec<Element<'a, Message>> = error
            .details
            .iter()
            .filter(|_| self.details_open)
            .map(|line| {
                text::monotext(line.clone())
                    .wrapping(Wrapping::WordOrGlyph)
                    .into()
            })
            .collect();
        if hint.is_none() && action.is_none() && disclosure.is_none() {
            return None;
        }
        Some(
            column::with_capacity(3 + details.len())
                .spacing(LINE_SPACING)
                .width(Length::Fill)
                .push_maybe(hint)
                .push_maybe(action)
                .push_maybe(disclosure)
                .extend(details)
                .into(),
        )
    }

    fn disclosure(&self, error: &ErrorRow) -> Option<Element<'_, Message>> {
        if error.details.is_empty() {
            return None;
        }
        let label = if self.details_open {
            fl!("hide-details")
        } else {
            fl!("show-details")
        };
        Some(button::link(label).on_press(Message::ToggleDetails).into())
    }

    fn apps(&self) -> Element<'_, Message> {
        let title = fl!("apps-in-picker");
        if self.inventory.is_empty() {
            return column::with_capacity(2)
                .spacing(theme::spacing().space_xxs)
                .push(text::heading(title))
                .push(text::body(fl!("apps-empty")).wrapping(Wrapping::Word))
                .into();
        }
        settings::section()
            .title(title)
            .extend(self.inventory.iter().map(app_row))
            .into()
    }
}

fn app_row(app: &AppRow) -> list::ListButton<'_, Message> {
    let trailing = row::with_capacity(2)
        .spacing(theme::spacing().space_s)
        .align_y(Alignment::Center)
        .push_maybe(
            profile_count(app.profiles.len())
                .map(|count| text::body(count).class(theme::Text::Custom(muted))),
        )
        .push(icons::icon(Icon::CaretRight, CARET_SIZE));
    list::button(
        settings::item::builder(app.name.as_str())
            .icon(app_icon(app.icon.as_deref(), APP_ICON_SIZE))
            .control(trailing),
    )
    .on_press(Message::OpenAppDrawer(app.id.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::tests::plain;

    #[test]
    fn an_app_shows_its_profile_count_in_the_singular_and_plural_and_nothing_without_profiles() {
        let counts: Vec<Option<String>> = [0, 1, 2, 3]
            .into_iter()
            .map(|n| profile_count(n).map(|text| plain(&text)))
            .collect();
        assert_eq!(
            counts,
            [
                None,
                Some("1 profile".to_owned()),
                Some("2 profiles".to_owned()),
                Some("3 profiles".to_owned())
            ]
        );
    }

    #[test]
    fn the_file_menu_opens_the_two_sub_pages_and_nothing_else() {
        let messages: Vec<Message> = file_items()
            .iter()
            .map(|(_, item)| menu::Action::message(item))
            .collect();
        assert!(
            matches!(
                messages.as_slice(),
                [Message::Go(Page::Shortcuts), Message::Go(Page::About)]
            ),
            "the File menu holds {} entries; the test link lives in the header only",
            messages.len()
        );
    }
}
