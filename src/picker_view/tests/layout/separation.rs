//! A frosted picker keeps its tiles, lock badge and queue row apart from the card, and its profile dots
//! clear of a second translucent layer, as the opaque picker has them.

use std::sync::Arc;

use cosmic::cosmic_theme::palette::Srgba;
use cosmic::cosmic_theme::{BlurStrength, ThemeBuilder};

use super::*;
use crate::colors::{PALETTE, needs_outline};
use crate::test_support::frosted::frosted;
use parts::{BADGE_HALO, BADGE_SIZE};

/// The most and the least translucent cards COSMIC offers, and its default.
const STRENGTHS: [BlurStrength; 3] = [
    BlurStrength::ExtremelyLow,
    BlurStrength::Medium,
    BlurStrength::ExtremelyHigh2,
];
const DARK_BACKDROP: Color = Color::from_rgb(0.1, 0.1, 0.1);
const LIGHT_BACKDROP: Color = Color::from_rgb(0.9, 0.9, 0.9);
const BACKDROPS: [Color; 3] = [DARK_BACKDROP, BACKDROP, LIGHT_BACKDROP];
/// From the queue row's bottom left to a pixel clear of its icon.
const QUEUE_INSET: Vector = Vector::new(20.0, -4.0);
/// From the queue row's right to a pixel inside its count pill, clear of the pill's words.
const PILL_INSET: Vector = Vector::new(-22.0, QUEUE_ROW_HEIGHT / 2.0);
/// From a tile's top left to the middle of its keycap's left edge.
const KEYCAP_EDGE: Vector = Vector::new(KEYCAP_INSET, KEYCAP_INSET + 8.0);
/// Antialiasing of an edge reaches this far to either side of it.
const EDGE_REACH: f32 = 1.0;
/// How many pixels, either way, the search for the halo's ring reaches from the dot's center.
const RING_SPAN: i8 = 13;
/// From the dot's center to the pixel, to its right, that its 1 px outline fills.
const OUTLINE_PIXEL: Vector = Vector::new(BADGE_SIZE / 2.0 - 1.0, 0.0);
/// How much lighter or darker than the dot its rim is drawn when an outline is.
const OUTLINE_DIFFERENCE: f32 = 0.1;
/// A dot well apart from the tile of a dark theme, and of a light one, and from the outline's colors.
const DOT_ON_DARK: Rgb = Rgb {
    r: 0x90,
    g: 0x90,
    b: 0x90,
};
const DOT_ON_LIGHT: Rgb = Rgb {
    r: 0x60,
    g: 0x60,
    b: 0x60,
};

fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(
        SCRATCH_HOME,
        &format!("picker_view::tests::layout::separation::{name}"),
    )
}

fn lightness(color: Color) -> f32 {
    (color.r + color.g + color.b) / 3.0
}

/// `frosted(dark, strength)` as the app has it while the compositor cannot blur, and while it can.
fn themes(dark: bool, strength: BlurStrength) -> (cosmic::Theme, cosmic::Theme) {
    let mut translucent = frosted(dark, strength);
    translucent.transparent = true;
    (frosted(dark, strength), translucent)
}

impl Scene {
    /// The surface as the picker draws itself under `theme`, over `backdrop`.
    fn drawn_over(&mut self, theme: &cosmic::Theme, backdrop: Color) -> Drawn {
        let view = view(&self.picker, &self.queue, &self.colors, theme);
        self.frames.drawn_over(view, theme, backdrop)
    }
}

/// A dark theme in which the card, the tiles and both ends of the neutrals are one white.
fn even_theme() -> cosmic::Theme {
    let white = Srgba::new(1.0, 1.0, 1.0, 1.0);
    let mut builder = ThemeBuilder::dark();
    let palette = builder.palette.as_mut();
    palette.neutral_0 = white;
    palette.neutral_2 = white;
    builder.bg_color = Some(white);
    let mut theme = cosmic::Theme::system(Arc::new(builder.build()));
    theme.transparent = true;
    theme
}

/// How much lighter what is drawn at `point` is than what is drawn at `under`.
fn standing(drawn: &Drawn, point: Point, under: Point) -> f32 {
    lightness(drawn.at(point)) - lightness(drawn.at(under))
}

#[test]
fn frosted_parts_stand_off_what_they_are_on_as_in_the_opaque_theme() {
    if !in_scratch_home("frosted_parts_stand_off_what_they_are_on_as_in_the_opaque_theme") {
        return;
    }
    let mut scene = Scene::new(plain_picker(2), 1);
    let queue_row = scene.probe().stops.last().copied().expect("the queue row");
    let queue = Point::new(queue_row.x, queue_row.y + queue_row.height) + QUEUE_INSET;
    let pill = Point::new(queue_row.x + queue_row.width, queue_row.y) + PILL_INSET;
    let keycap = tile_center(1) - Vector::new(TILE_WIDTH, TILE_HEIGHT) * 0.5 + KEYCAP_EDGE;
    let parts = [
        ("tile", tile_interior(), card_interior()),
        ("lock badge", badge_interior(), card_interior()),
        ("queue row", queue, card_interior()),
        ("count pill", pill, queue),
        ("keycap's edge", keycap, tile_interior()),
    ];
    for dark in [true, false] {
        for strength in STRENGTHS {
            let (opaque, translucent) = themes(dark, strength);
            let opaque_drawn = scene.drawn_over(&opaque, BACKDROP);
            for backdrop in BACKDROPS {
                let drawn = scene.drawn_over(&translucent, backdrop);
                for (part, point, under) in parts {
                    let seen = standing(&drawn, point, under);
                    let opaque_gap = standing(&opaque_drawn, point, under);
                    assert!(
                        seen * opaque_gap > 0.0 && seen.abs() >= opaque_gap.abs() / 2.0,
                        "dark={dark}, {strength:?}, over {backdrop:?}: the {part} stands {seen} off what it is on, the opaque theme's {opaque_gap}"
                    );
                }
            }
        }
    }
}

#[test]
fn an_even_theme_lifts_tiles_by_nothing() {
    let lift = card_surface(&even_theme()).a;
    assert!(near(lift, 0.0), "the tile is lifted by {lift}");
}

#[test]
fn a_hovered_queue_row_is_lit_over_its_rest_as_in_the_opaque_theme() {
    let lit_over_rest = |theme: &cosmic::Theme, backdrop: Color| {
        let card = over(fill(theme, &theme::Container::Background), backdrop);
        let rest = over(card_surface(theme), card);
        lightness(over(hover_color(theme), card)) - lightness(rest)
    };
    for dark in [true, false] {
        for strength in STRENGTHS {
            let (opaque, translucent) = themes(dark, strength);
            let opaque_gap = lit_over_rest(&opaque, BACKDROP);
            for backdrop in BACKDROPS {
                let seen = lit_over_rest(&translucent, backdrop);
                assert!(
                    seen * opaque_gap > 0.0 && seen.abs() >= opaque_gap.abs() / 2.0,
                    "dark={dark}, {strength:?}, over {backdrop:?}: hover lights the row {seen} over its rest, the opaque theme's {opaque_gap}"
                );
            }
        }
    }
}

/// A tile with nothing in it but the badge of a profile of color `dot`, at its bottom right as the icon's
/// badge is.
fn badged_tile<'a>(dot: Rgb) -> Element<'a, Message> {
    let face = container(badge_disk(dot))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Right)
        .align_y(alignment::Vertical::Bottom);
    TileLook {
        color: Some(dot),
        lit: false,
        ring: false,
    }
    .surface(tile_id(0), face.into())
}

/// The middle of the dot in [`badged_tile`].
fn dot_center() -> Point {
    let reach = BADGE_HALO + BADGE_SIZE / 2.0;
    Point::new(TILE_WIDTH - reach, TILE_HEIGHT - reach)
}

/// The pixels, in [`badged_tile`], of the halo's ring: those whose middle is clear of the dot, its outline
/// and the antialiasing of the edges of both.
fn halo_ring() -> impl Iterator<Item = Point> {
    let center = dot_center();
    let (inner, outer) = (
        BADGE_SIZE / 2.0 + EDGE_REACH,
        BADGE_SIZE / 2.0 + BADGE_HALO - EDGE_REACH,
    );
    (-RING_SPAN..RING_SPAN)
        .flat_map(|down| {
            (-RING_SPAN..RING_SPAN).map(move |right| Vector::new(f32::from(right), f32::from(down)))
        })
        .filter(move |offset| {
            let middle = Vector::new(offset.x + 0.5, offset.y + 0.5);
            (inner..=outer).contains(&middle.x.hypot(middle.y))
        })
        .map(move |offset| center + offset)
}

#[test]
fn a_dot_adds_nothing_to_its_tile_in_a_frosted_picker_but_itself_and_its_outline() {
    if !in_scratch_home(
        "a_dot_adds_nothing_to_its_tile_in_a_frosted_picker_but_itself_and_its_outline",
    ) {
        return;
    }
    let mut frames = Frames::new(SURFACE);
    for dark in [true, false] {
        let (opaque, translucent) = themes(dark, BlurStrength::Medium);
        let tile_color = parts::rgb_of(opaque.cosmic().background(false).component.base.into());
        let apart = if dark { DOT_ON_DARK } else { DOT_ON_LIGHT };
        for theme in [&opaque, &translucent] {
            for backdrop in BACKDROPS {
                for dot in [tile_color, apart] {
                    let drawn = frames.drawn_over(badged_tile(dot), theme, backdrop);
                    let note = format!(
                        "dark={dark}, frosted={}, over {backdrop:?}, dot {dot:?}",
                        theme.transparent
                    );
                    let fill = drawn.at(Point::new(10.0, 10.0));
                    for pixel in halo_ring() {
                        let seen = drawn.at(pixel);
                        assert!(
                            is_rounding_apart(seen, fill),
                            "{note}: the halo is drawn {seen:?} at {pixel:?}, the tile {fill:?}"
                        );
                    }
                    let outline = drawn.at(dot_center() + OUTLINE_PIXEL);
                    let outlined = (lightness(outline) - lightness(drawn.at(dot_center()))).abs()
                        > OUTLINE_DIFFERENCE;
                    assert_eq!(
                        outlined,
                        theme.transparent || needs_outline(dot, tile_color),
                        "{note}: the dot's rim is drawn {outline:?}"
                    );
                }
            }
        }
    }
}

/// What [`on_the_card`] takes up.
const CARDED_TILE: Size = Size::new(
    TILE_WIDTH + 2.0 * CARD_PADDING,
    TILE_HEIGHT + 2.0 * CARD_PADDING,
);

/// `tile` on the picker's card, as it stands in the picker: over the card, over the backdrop.
fn on_the_card(tile: Element<'_, Message>) -> Element<'_, Message> {
    container(tile)
        .padding(CARD_PADDING)
        .class(theme::Container::custom(parts::shell_fill))
        .into()
}

#[test]
fn a_frosted_dot_of_any_palette_color_keeps_a_visible_edge_on_its_tile() {
    if !in_scratch_home("a_frosted_dot_of_any_palette_color_keeps_a_visible_edge_on_its_tile") {
        return;
    }
    let card = Vector::new(CARD_PADDING, CARD_PADDING);
    let mut frames = Frames::new(CARDED_TILE);
    for dark in [true, false] {
        for strength in STRENGTHS {
            let (_, translucent) = themes(dark, strength);
            for backdrop in BACKDROPS {
                for dot in PALETTE {
                    let drawn =
                        frames.drawn_over(on_the_card(badged_tile(dot)), &translucent, backdrop);
                    let fill = parts::rgb_of(drawn.at(Point::new(10.0, 10.0) + card));
                    let center = dot_center() + card;
                    let seen =
                        [drawn.at(center), drawn.at(center + OUTLINE_PIXEL)].map(parts::rgb_of);
                    assert!(
                        seen.iter().any(|ink| !needs_outline(*ink, fill)),
                        "dark={dark}, {strength:?}, over {backdrop:?}: dot {dot:?} is drawn {:?} with its rim {:?} on a tile drawn {fill:?}",
                        seen[0],
                        seen[1]
                    );
                }
            }
        }
    }
}

#[test]
fn the_header_buttons_share_the_lock_badges_tone() {
    if !in_scratch_home("the_header_buttons_share_the_lock_badges_tone") {
        return;
    }
    let mut scene = Scene::new(plain_picker(2), 0);
    let stops = scene.probe().stops;
    // The pin and copy buttons are the header's first two stops; a pixel inside each, clear of its icon.
    let inside = |stop: Rectangle| Point::new(stop.x + stop.width * 0.15, stop.center_y());
    let buttons = [("pin", inside(stops[0])), ("copy", inside(stops[1]))];
    for dark in [true, false] {
        for strength in STRENGTHS {
            let (opaque, translucent) = themes(dark, strength);
            for (kind, theme) in [("opaque", &opaque), ("translucent", &translucent)] {
                let drawn = scene.drawn_over(theme, BACKDROP);
                let badge = drawn.at(badge_interior());
                for (button, point) in buttons {
                    let seen = drawn.at(point);
                    assert!(
                        is_rounding_apart(seen, badge),
                        "dark={dark}, {strength:?}, {kind}: the {button} button is drawn {seen:?}, the lock badge {badge:?}"
                    );
                }
            }
        }
    }
}
