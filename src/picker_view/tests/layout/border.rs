//! The card drawn as an edged shell, and its bands as the same fill without the edge: the Background layer's
//! fill and divider, rounded at `radius_m`, under every theme the picker meets.

use std::f32::consts::FRAC_1_SQRT_2;
use std::sync::Arc;

use cosmic::cosmic_theme::{Roundness, ThemeBuilder};

use super::*;

/// The pixel row or column a 1 px edge fills, and the one inside it.
/// How much of the backdrop the antialiasing of an edge inset from the surface lets into its pixel.
const EDGE_BLEED: f32 = 0.02;
const EDGE: f32 = 0.0;
const INSIDE_EDGE: f32 = 1.0;
/// How many pixels, along each side, the probe inside a corner's arc sits past the one outside it.
const ACROSS_THE_ARC: f32 = 3.0;
/// How far in from a corner, along each side, the pixel is that the default arc leaves outside the card
/// and the arc of a slightly round theme holds.
const BETWEEN_ARCS: f32 = 3.0;

fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(
        SCRATCH_HOME,
        &format!("picker_view::tests::layout::border::{name}"),
    )
}

/// A dark theme at COSMIC's "slightly round" setting, whose `radius_m` is not the default's.
fn slightly_round() -> cosmic::Theme {
    let theme = ThemeBuilder {
        corner_radii: Roundness::SlightlyRound.into(),
        ..ThemeBuilder::dark()
    }
    .build();
    cosmic::Theme::system(Arc::new(theme))
}

/// Both tones of the surface the picker draws on: frosted, so its fill is translucent, and opaque; and
/// a roundness other than the default.
fn themes() -> [(&'static str, cosmic::Theme); 5] {
    [
        ("dark frosted", translucent(true, FROSTED_ALPHA)),
        ("light frosted", translucent(false, FROSTED_ALPHA)),
        ("dark opaque", cosmic::Theme::dark()),
        ("light opaque", cosmic::Theme::light()),
        ("dark slightly round", slightly_round()),
    ]
}

/// The pixels of each edge of `card`, each with the pixel just inside it: the middle of the top and bottom
/// edges, and the left and right ones at the header's height, clear of the empty card's illustration.
fn edges(card: Rectangle) -> [(Point, Point); 4] {
    let middle = (card.x + card.width / 2.0).round();
    let header = (card.y + CARD_PADDING + HEADER_HEIGHT / 2.0).round();
    let right = (card.x + card.width).round() - 1.0;
    let bottom = (card.y + card.height).round() - 1.0;
    [
        (Point::new(middle, EDGE), Point::new(middle, INSIDE_EDGE)),
        (
            Point::new(middle, bottom),
            Point::new(middle, bottom - INSIDE_EDGE),
        ),
        (Point::new(EDGE, header), Point::new(INSIDE_EDGE, header)),
        (
            Point::new(right, header),
            Point::new(right - INSIDE_EDGE, header),
        ),
    ]
}

/// A pixel in each corner of `card` that the arc of `radius_m` leaves outside it, though the arc of
/// `radius_s` would still hold it, and one inside the arc, clear of the edge.
fn corners(card: Rectangle, theme: &cosmic::Theme) -> [(Point, Point); 4] {
    let [radius, ..] = theme.cosmic().radius_m();
    let outside = (radius * (1.0 - FRAC_1_SQRT_2)).floor() - 1.0;
    let inside = outside + ACROSS_THE_ARC;
    let (left, top) = (card.x.round(), card.y.round());
    let right = (card.x + card.width).round() - 1.0;
    let bottom = (card.y + card.height).round() - 1.0;
    [
        (Point::new(left, top), Vector::new(1.0, 1.0)),
        (Point::new(right, top), Vector::new(-1.0, 1.0)),
        (Point::new(right, bottom), Vector::new(-1.0, -1.0)),
        (Point::new(left, bottom), Vector::new(1.0, -1.0)),
    ]
    .map(|(corner, inward)| (corner + inward * outside, corner + inward * inside))
}

/// Draws `picker` under every theme and holds its edges to a divider over what `beneath` makes of the pixel
/// just inside each: a card's own border replaces its fill, while an edge layer over a fill lies on it.
fn assert_edged(kind: &str, picker: &Picker, beneath: impl Fn(Color) -> Color) {
    for (name, theme) in themes() {
        let mut scene = Scene::new(picker.clone(), 0);
        let card = scene
            .probe()
            .container_named(&PICKER_AUTOSIZE)
            .expect("the card");
        let drawn = scene.drawn(&theme);
        let divider: Color = theme.cosmic().background(theme.transparent).divider.into();
        for (edge, inside) in edges(card) {
            let (seen, inner) = (drawn.at(edge), drawn.at(inside));
            let edged = over(divider, beneath(inner));
            let note = format!("{kind}, {name}, at {edge:?}");
            assert!(
                is_rounding_apart(seen, edged),
                "{note}: drawn {seen:?}, a divider over {inner:?} inside gives {edged:?}"
            );
            assert!(
                !is_rounding_apart(seen, inner),
                "{note}: the edge is no different from the pixel inside it, {inner:?}"
            );
        }
        for (outside, inside) in corners(card, &theme) {
            let seen = drawn.at(outside);
            assert!(
                is_rounding_apart(seen, BACKDROP),
                "{kind}, {name}: the corner at {outside:?} is drawn {seen:?}, not rounded away to {BACKDROP:?}"
            );
            let seen = drawn.at(inside);
            assert!(
                !is_rounding_apart(seen, BACKDROP),
                "{kind}, {name}: the card is cut away inside its corner arc, at {inside:?}"
            );
        }
    }
}

#[test]
fn a_card_of_tiles_has_a_one_pixel_divider_edge_rounded_at_radius_m() {
    if !in_scratch_home("a_card_of_tiles_has_a_one_pixel_divider_edge_rounded_at_radius_m") {
        return;
    }
    assert_edged("a card of tiles", &plain_picker(2), |_| BACKDROP);
}

#[test]
fn the_no_apps_card_has_the_same_edge_over_its_illustration() {
    if !in_scratch_home("the_no_apps_card_has_the_same_edge_over_its_illustration") {
        return;
    }
    assert_edged(
        "the no-apps card",
        &Picker::new(URI.into(), Vec::new()),
        |inside| inside,
    );
}

#[test]
fn the_no_apps_illustration_follows_the_arc_of_a_less_round_theme() {
    if !in_scratch_home("the_no_apps_illustration_follows_the_arc_of_a_less_round_theme") {
        return;
    }
    let theme = slightly_round();
    let mut scene = Scene::new(Picker::new(URI.into(), Vec::new()), 0);
    let card = scene
        .probe()
        .container_named(&PICKER_AUTOSIZE)
        .expect("the card");
    let drawn = scene.drawn(&theme);
    let filled = over(fill(&theme, &theme::Container::Background), BACKDROP);
    let right = (card.x + card.width).round() - 1.0 - BETWEEN_ARCS;
    let bottom = (card.y + card.height).round() - 1.0 - BETWEEN_ARCS;
    for corner in [Point::new(BETWEEN_ARCS, bottom), Point::new(right, bottom)] {
        let seen = drawn.at(corner);
        assert!(
            !is_rounding_apart(seen, filled),
            "inside the arc at {corner:?} the card shows its fill {seen:?}, not the illustration"
        );
    }
}

#[test]
fn the_no_apps_card_draws_the_illustration_of_its_themes_tone() {
    if !in_scratch_home("the_no_apps_card_draws_the_illustration_of_its_themes_tone") {
        return;
    }
    for (name, theme) in themes() {
        let cosmic = theme.cosmic();
        let expected = icons::no_apps_rounded(cosmic.is_dark, cosmic.radius_m());
        let mut scene = Scene::new(Picker::new(URI.into(), Vec::new()), 0);
        let view = view(&scene.picker, &scene.queue, &scene.colors, &theme);
        let drawn = scene.frames.drawn_copy_of(&expected, view);
        assert!(
            std::ptr::eq(drawn.data(), expected.data()),
            "{name}: drew an illustration of the other tone"
        );
    }
}

#[test]
fn the_no_apps_card_is_filled_once_behind_its_illustration() {
    if !in_scratch_home("the_no_apps_card_is_filled_once_behind_its_illustration") {
        return;
    }
    let between_header_and_panel = Point::new(
        ILLUSTRATION_SIZE.width / 2.0,
        CARD_PADDING + HEADER_HEIGHT + SECTION_GAP / 2.0,
    );
    for (name, theme) in themes() {
        let once = over(fill(&theme, &theme::Container::Background), BACKDROP);
        let mut scene = Scene::new(Picker::new(URI.into(), Vec::new()), 0);
        let seen = scene.drawn(&theme).at(between_header_and_panel);
        assert!(
            is_rounding_apart(seen, once),
            "{name}: drawn {seen:?}, one fill gives {once:?}"
        );
    }
}

/// The middle of each of band `band`'s four edges, clear of its corners, where two edges meet and
/// antialiasing blends them.
fn band_edges(block: Rectangle, card_bottom: f32, band: usize) -> [Point; 4] {
    let inset = SHEET_STEP * to_f32(band + 1);
    let top = (card_bottom + SHEET_STEP * to_f32(band)).round();
    let bottom = (card_bottom + SHEET_STEP * to_f32(band + 1)).round() - 1.0;
    let middle_y = top.midpoint(bottom).round();
    let middle_x = (block.x + block.width / 2.0).round();
    [
        Point::new((block.x + inset).round(), middle_y),
        Point::new((block.x + block.width - inset).round() - 1.0, middle_y),
        Point::new(middle_x, top),
        Point::new(middle_x, bottom),
    ]
}

#[test]
fn a_lone_band_has_no_edge_and_stacked_bands_wear_the_cards() {
    if !in_scratch_home("a_lone_band_has_no_edge_and_stacked_bands_wear_the_cards") {
        return;
    }
    for (name, theme) in themes() {
        let cosmic = theme.cosmic();
        let primary: Color = cosmic.primary(theme.transparent).base.into();
        let background: Color = cosmic.background(theme.transparent).base.into();
        assert!(
            !is_rounding_apart(primary, background),
            "{name}: Primary and Background must differ for this to tell them apart"
        );
        for waiting in [1, 2] {
            let mut scene = Scene::new(plain_picker(1), waiting);
            let (block, card_bottom) = card_and_bands(&scene.probe());
            let drawn = scene.drawn(&theme);
            let card = drawn.at(card_interior());
            let middle = (block.x + block.width / 2.0).round();
            for band in 0..waiting {
                let note = format!("{name}, {waiting} waiting, band {band}");
                let fill = drawn.at(Point::new(
                    middle,
                    card_bottom + SHEET_STEP * (to_f32(band) + 0.5),
                ));
                assert!(
                    is_rounding_apart(fill, card),
                    "{note}: the band's fill is drawn {fill:?}, the card's is {card:?}"
                );
                assert!(
                    !is_rounding_apart(fill, BACKDROP),
                    "{note}: the band's fill {fill:?} is no different from the backdrop"
                );
                // One band reads as part of the card; stacked bands need the card's edge to read apart.
                let divider: Color = cosmic.background(theme.transparent).divider.into();
                let edge_color = if waiting > 1 {
                    over(divider, BACKDROP)
                } else {
                    fill
                };
                for edge in band_edges(block, card_bottom, band) {
                    let seen = drawn.at(edge);
                    let gap = (seen.r - edge_color.r)
                        .abs()
                        .max((seen.g - edge_color.g).abs())
                        .max((seen.b - edge_color.b).abs());
                    assert!(
                        gap <= EDGE_BLEED,
                        "{note}: the band's edge at {edge:?} is drawn {seen:?}, not {edge_color:?}"
                    );
                }
            }
        }
        let [_, _, bottom_right, bottom_left] = cosmic.radius_m();
        for edged in [false, true] {
            assert_eq!(
                sheet_style(&theme, edged).border.radius,
                [0.0, 0.0, bottom_right, bottom_left].into(),
                "{name}: a band is rounded below and square above"
            );
        }
    }
}

#[test]
fn the_no_apps_deserts_sky_fades_in_above_its_picture() {
    // No step shows between neighboring pixels.
    const GENTLE: f32 = 3.0 / 255.0;
    if !in_scratch_home("the_no_apps_deserts_sky_fades_in_above_its_picture") {
        return;
    }
    for (name, theme) in themes() {
        let cosmic = theme.cosmic();
        let picture = icons::no_apps_rounded(cosmic.is_dark, cosmic.radius_m());
        let mut scene = Scene::new(Picker::new(URI.into(), Vec::new()), 0);
        let shown = view(&scene.picker, &scene.queue, &scene.colors, &theme);
        let bounds = scene.frames.drawn_svg_of(&picture, shown).bounds;
        let drawn = scene.drawn(&theme);
        // All along the picture's top, the fade above joins its sky without a seam.
        for tenth in 0..10u8 {
            let x = (bounds.x + bounds.width * (f32::from(tenth) + 0.5) / 10.0).floor();
            let sky = drawn.at(Point::new(x, bounds.y));
            let above = drawn.at(Point::new(x, bounds.y - 1.0));
            assert!(
                headless::channel_gap(sky, above) <= GENTLE,
                "{name}: at x={x} the sky ends at the picture's top, {sky:?} below and {above:?} above"
            );
        }
        // Clear of the clouds, the sun and the signpost.
        let x = (bounds.x + bounds.width * 0.6).floor();
        let sky = drawn.at(Point::new(x, bounds.y + 1.0));
        let above = drawn.at(Point::new(x, bounds.y - 1.0));
        let faded = drawn.at(Point::new(x, bounds.y - icons::NO_APPS_FADE));
        assert!(
            headless::channel_gap(faded, sky) > headless::channel_gap(above, sky),
            "{name}: over the fade the sky does not give way to the card ({faded:?} at its top)"
        );
    }
}
