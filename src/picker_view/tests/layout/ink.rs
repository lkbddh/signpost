//! The no-apps card's words are drawn in the colors of the tile card's.

use super::*;

/// How far the header's host reaches from where it starts.
const HOST_WIDTH: f32 = 90.0;
/// Where the no-apps card's heading, hint and button are, from the card's top and in from its sides.
const WORDS_TOP: f32 = 130.0;
const WORDS_HEIGHT: f32 = 95.0;
const WORDS_INSET: f32 = 64.0;

fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(
        SCRATCH_HOME,
        &format!("picker_view::tests::layout::ink::{name}"),
    )
}

fn lightness(color: Color) -> f32 {
    (color.r + color.g + color.b) / 3.0
}

/// `from`, then every pixel after it up to `to`.
fn steps(from: f32, to: f32) -> impl Iterator<Item = f32> + Clone {
    std::iter::successors(Some(from), move |at| (at + 1.0 < to).then_some(at + 1.0))
}

/// The color of the pixel of `area` that stands furthest from the card's fill: the color of the boldest
/// word in it.
fn ink_in(drawn: &Drawn, area: Rectangle) -> Color {
    let fill = lightness(drawn.at(card_interior()));
    steps(area.y, area.y + area.height)
        .flat_map(|y| steps(area.x, area.x + area.width).map(move |x| Point::new(x, y)))
        .map(|pixel| drawn.at(pixel))
        .max_by(|a, b| {
            (lightness(*a) - fill)
                .abs()
                .total_cmp(&(lightness(*b) - fill).abs())
        })
        .expect("the area has pixels")
}

/// Where the header's host is: past its badge, across the header's height.
fn host_area() -> Rectangle {
    let left = CARD_PADDING + HEADER_BADGE_SIZE + f32::from(theme::spacing().space_s);
    Rectangle::new(
        Point::new(left, CARD_PADDING),
        Size::new(HOST_WIDTH, HEADER_HEIGHT),
    )
}

#[test]
fn the_no_apps_cards_words_are_drawn_in_the_tile_cards_color() {
    if !in_scratch_home("the_no_apps_cards_words_are_drawn_in_the_tile_cards_color") {
        return;
    }
    let empty = Picker::new(URI.into(), Vec::new());
    let words = Rectangle::new(
        Point::new(WORDS_INSET, WORDS_TOP),
        Size::new(card_width(&empty) - 2.0 * WORDS_INSET, WORDS_HEIGHT),
    );
    for theme in [cosmic::Theme::dark(), cosmic::Theme::light()] {
        let note = format!("dark={}", theme.cosmic().is_dark);
        let host = host_area();
        let tiled = ink_in(&Scene::new(plain_picker(1), 0).drawn(&theme), host);
        let drawn = Scene::new(empty.clone(), 0).drawn(&theme);
        let bare = ink_in(&drawn, host);
        assert!(
            is_rounding_apart(bare, tiled),
            "{note}: the header host is drawn {bare:?} on the no-apps card, {tiled:?} on the tile card"
        );
        let shell = shell_fill(&theme)
            .text_color
            .expect("the shell sets a text color");
        let heading = ink_in(&drawn, words);
        assert!(
            is_rounding_apart(heading, shell),
            "{note}: the no-apps heading is drawn {heading:?}, the shell's text is {shell:?}"
        );
    }
}
