//! The Keyboard shortcuts and About pages.

use cosmic::iced::widget::text::Wrapping;
use cosmic::iced::widget::{Stack, container as container_style, pin, responsive};
use cosmic::iced::{Alignment, Background, Border, Color, ContentFit, Length};
use cosmic::widget::{
    Space, button, column, container, icon, list, list_column, row, scrollable, settings, svg, text,
};
use cosmic::{Element, theme};

use super::view::{page_content, scroll, tinted};
use super::window::{Link, Message, Page, REPO_URL};
use crate::fl;
use crate::icons::{self, Icon};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const LICENSE_NAME: &str = "GPL-3.0-or-later";
/// The size `about-hero.svg` is drawn at; the page scales it to the window width.
const HERO_SIZE: (f32, f32) = (640.0, 240.0);
/// The share of the hero above its skyline, which fades into the window and may lie behind the list: the
/// tallest roof in the file starts 104 of its 240 units down.
const HERO_SKY: f32 = 104.0 / 240.0;
const LIST_SURFACE_ALPHA: f32 = 0.3;
const CHIP_RADIUS: f32 = 4.0;
const CHIP_PADDING: [u16; 2] = [2, 8];
const BACK_SPACING: u16 = 4;
const TITLE_SPACING: u16 = 6;

/// Each shortcut as its description and the keys to press, in the order the picker teaches them.
fn shortcuts_table() -> Vec<(String, Vec<String>)> {
    vec![
        (fl!("shortcut-select"), vec![fl!("key-arrows")]),
        (fl!("shortcut-numbered"), vec![fl!("key-digits")]),
        (fl!("shortcut-open"), vec![fl!("key-enter")]),
        (
            fl!("shortcut-keep"),
            vec![fl!("key-ctrl"), fl!("key-enter")],
        ),
        (fl!("shortcut-copy"), vec![fl!("key-ctrl"), fl!("key-c")]),
        (fl!("shortcut-close"), vec![fl!("key-esc")]),
    ]
}

/// A Back button to the main page over the page's title.
fn page_title(title: String) -> Element<'static, Message> {
    let back = button::icon(icon::from_name("go-previous-symbolic"))
        .extra_small()
        .padding(0)
        .label(fl!("app-name"))
        .spacing(BACK_SPACING)
        .class(button::ButtonClass::Link)
        .on_press(Message::Go(Page::Main));
    column::with_capacity(2)
        .spacing(TITLE_SPACING)
        .push(back)
        .push(
            text::title3(title)
                .wrapping(Wrapping::WordOrGlyph)
                .width(Length::Fill),
        )
        .into()
}

/// The page's title and a Back button to the main page, fixed above the scrolling body.
fn sub_page(title: String, body: Element<'_, Message>, gutter: u16) -> Element<'_, Message> {
    let spacing = theme::spacing();
    column::with_capacity(2)
        .push(page_content(
            page_title(title),
            gutter,
            [spacing.space_xs, spacing.space_m],
        ))
        .push(scroll(body, gutter, 0))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn chip_style(theme: &cosmic::Theme) -> container_style::Style {
    container_style::Style {
        border: Border {
            color: theme.current_container().divider.into(),
            width: 1.0,
            radius: CHIP_RADIUS.into(),
        },
        ..Default::default()
    }
}

/// A key cap: monospace text in a thin bordered box, on one line.
pub(super) fn chip(label: String) -> Element<'static, Message> {
    container(text::monotext(label).wrapping(Wrapping::None))
        .padding(CHIP_PADDING)
        .class(theme::Container::custom(chip_style))
        .into()
}

pub(super) fn shortcuts(gutter: u16) -> Element<'static, Message> {
    let spacing = theme::spacing();
    let mut section = settings::section();
    for (description, keys) in shortcuts_table() {
        let chips = row::with_children(keys.into_iter().map(chip)).spacing(spacing.space_xxs);
        // The keys drop below the description when the window is too narrow for both.
        section = section.add(settings::flex_item(description, chips));
    }
    let body = column::with_capacity(2)
        .spacing(spacing.space_m)
        .push(section)
        .push(text::body(fl!("shortcuts-footnote")).wrapping(Wrapping::Word));
    sub_page(fl!("keyboard-shortcuts"), body.into(), gutter)
}

/// The theme's own surface while it is translucent, so the list is as frosted as the user chose; otherwise a
/// thin veil.
fn translucent(theme: &cosmic::Theme) -> container_style::Style {
    let mut surface = Color::from(theme.current_container().component.base);
    if !theme.transparent {
        surface.a = LIST_SURFACE_ALPHA;
    }
    container_style::Style {
        background: Some(Background::Color(surface)),
        border: Border {
            radius: theme.cosmic().radius_s().into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A row whose value is a link: accent text and an arrow out of the window.
fn link_row(title: String, value: String, link: Link) -> list::ListButton<'static, Message> {
    let value = row::with_capacity(2)
        .spacing(theme::spacing().space_xxs)
        .align_y(Alignment::Center)
        .push(text::body(value).class(theme::Text::Accent))
        .push(tinted(
            Icon::ArrowSquareOut,
            16,
            cosmic::cosmic_theme::Theme::accent_text_color,
        ));
    list::button(settings::item(title, value)).on_press(Message::OpenLink(link))
}

/// The hero spans the page width and sits on the bottom edge, its file opaque: its `sky` fades in over all the
/// page above it, so it begins behind the list however tall the window is. A page too short for the hero crops
/// its top.
/// Not `aspect_ratio_container`: it panics laying out an `Svg`, which has no child tree for it to index.
fn hero(handle: &svg::Handle, sky: Color) -> Element<'static, Message> {
    let handle = handle.clone();
    let image = responsive(move |available| {
        let height = available.width * HERO_SIZE.1 / HERO_SIZE.0;
        let fade = (available.height - height).max(0.0);
        let image = svg(handle.clone())
            .width(available.width)
            .height(height)
            .content_fit(ContentFit::Fill);
        let scene = column::with_capacity(2)
            .push(icons::sky_fade(sky, fade))
            .push(image);
        pin(scene).y(available.height - height - fade).into()
    });
    // An `Svg` clips only to its own bounds. A clipping `Stack` draws every layer above its first on a layer
    // of its own size, so the image goes above an empty one.
    Stack::new()
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .push(Space::new())
        .push(image)
        .into()
}

fn repository_label() -> &'static str {
    REPO_URL.strip_prefix("https://").unwrap_or(REPO_URL)
}

/// The facts about Signpost, over the hero.
fn facts() -> Element<'static, Message> {
    list_column()
        .style(theme::Container::custom(translucent))
        .add(settings::item(
            fl!("about-version"),
            chip(VERSION.to_owned()),
        ))
        .add(link_row(
            fl!("about-source"),
            repository_label().to_owned(),
            Link::Source,
        ))
        .add(link_row(
            fl!("about-issues"),
            fl!("about-issues-link"),
            Link::Issues,
        ))
        .add(link_row(
            fl!("about-license"),
            LICENSE_NAME.to_owned(),
            Link::License,
        ))
        .into()
}

/// The title, the facts and room for the hero's skyline below them, over the hero, as one page that is never
/// shorter than the window: the hero sits on its bottom edge, and the list never lies over the skyline. A
/// window too short for all of it scrolls the page.
pub(super) fn about<'a>(
    hero_handle: &svg::Handle,
    sky: Color,
    gutter: u16,
) -> Element<'a, Message> {
    let hero_handle = hero_handle.clone();
    responsive(move |size| {
        let spacing = theme::spacing();
        let skyline = size.width * HERO_SIZE.1 / HERO_SIZE.0 * (1.0 - HERO_SKY);
        let content = column::with_capacity(3)
            .spacing(spacing.space_m)
            .push(page_title(fl!("about-signpost")))
            .push(facts())
            .push(Space::new().height(skyline));
        // A space one pixel wide (one of no width is dropped) makes the page at least as tall as the window.
        let page = row::with_capacity(2)
            .push(page_content(content, gutter, [spacing.space_xs, 0]))
            .push(Space::new().width(1).height(size.height));
        scrollable(Stack::new().push(page).push_under(hero(&hero_handle, sky)))
            .height(Length::Fill)
            .into()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use cosmic::cosmic_theme::BlurStrength;

    use super::*;
    use crate::test_support::frosted::frosted;

    const STRENGTHS: [BlurStrength; 3] = [
        BlurStrength::ExtremelyLow,
        BlurStrength::Medium,
        BlurStrength::ExtremelyHigh2,
    ];

    fn list_alpha(theme: &cosmic::Theme) -> f32 {
        let Some(Background::Color(surface)) = translucent(theme).background else {
            panic!("the list is filled with a color");
        };
        surface.a
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn the_about_list_keeps_the_alpha_a_translucent_theme_gives_its_surface() {
        for dark in [true, false] {
            for strength in STRENGTHS {
                let mut theme = frosted(dark, strength);
                theme.transparent = true;
                let given = theme.current_container().component.base.alpha;
                let drawn = list_alpha(&theme);
                assert!(
                    near(drawn, given),
                    "dark={dark}, {strength:?}: drawn at {drawn}, the theme gives {given}"
                );
            }
        }
    }

    #[test]
    fn the_about_list_is_a_thin_veil_while_the_theme_is_opaque() {
        for dark in [true, false] {
            let theme = frosted(dark, BlurStrength::Medium);
            assert!(!theme.transparent);
            let drawn = list_alpha(&theme);
            assert!(near(drawn, LIST_SURFACE_ALPHA), "dark={dark}: {drawn}");
        }
    }

    #[test]
    fn the_hero_scales_from_the_size_its_svg_is_drawn_at() {
        let (width, height) = HERO_SIZE;
        let view_box = format!("viewBox=\"0 0 {width} {height}\"");
        for hero in [
            include_str!("../../resources/about-hero.svg"),
            include_str!("../../resources/about-hero-light.svg"),
        ] {
            assert!(hero.contains(&view_box));
        }
    }

    #[test]
    fn the_shortcuts_page_teaches_the_picker_keys_in_order() {
        let table: Vec<(String, Vec<String>)> = shortcuts_table();
        let keys: Vec<Vec<&str>> = table
            .iter()
            .map(|(_, keys)| keys.iter().map(String::as_str).collect())
            .collect();
        assert_eq!(
            keys,
            [
                vec!["Arrow keys"],
                vec!["1–9"],
                vec!["Enter"],
                vec!["Ctrl", "Enter"],
                vec!["Ctrl", "C"],
                vec!["Esc"]
            ]
        );
        let descriptions: Vec<&str> = table.iter().map(|(text, _)| text.as_str()).collect();
        assert_eq!(
            descriptions,
            [
                "Select an app or profile",
                "Open a numbered tile",
                "Open the selection",
                "Open and keep the picker",
                "Copy the link",
                "Close the picker"
            ]
        );
    }

    #[test]
    fn the_source_row_shows_the_one_repository_constant_without_its_scheme() {
        assert_eq!(format!("https://{}", repository_label()), REPO_URL);
    }
}
