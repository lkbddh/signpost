//! The failure details wrap clear of their scrollbar.

use super::*;

/// libcosmic's scrollbar is this many pixels wide, at the scrollable's right edge.
const SCROLLBAR_COLUMNS: u8 = 8;
/// A row inside the thumb of the bar, counted from the top of long details.
const IN_THUMB: f32 = 10.0;
/// Rows of long details, counted from their top, that the thumb has left and the bar's rounded end has
/// not reached.
const BELOW_THUMB: std::ops::Range<u8> = 48..80;
/// A tile count for each width the card takes: its least, a middle one and its widest.
const TILE_COUNTS: [usize; 3] = [1, 3, 4];

fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(
        SCRATCH_HOME,
        &format!("picker_view::tests::layout::details::{name}"),
    )
}

/// Lines of one unbroken word, which wrap wherever the width ends.
fn unbroken_error() -> String {
    format!("{}\n", "M".repeat(100)).repeat(10)
}

#[test]
fn failure_details_wrap_clear_of_their_scrollbar() {
    if !in_scratch_home("failure_details_wrap_clear_of_their_scrollbar") {
        return;
    }
    for tiles in TILE_COUNTS {
        for theme in [cosmic::Theme::dark(), cosmic::Theme::light()] {
            let mut picker = failed_picker(tiles, "Work", unbroken_error());
            picker.handle(Input::ToggleDetails);
            let mut scene = Scene::new(picker, 0);
            let probe = scene.probe();
            let grid = probe.scrollable_named(&PICKER_SCROLL).expect("the grid");
            let details = probe
                .scrollables
                .iter()
                .find(|scrolled| scrolled.bounds != grid.bounds)
                .expect("the details")
                .bounds;
            let drawn = scene.drawn(&theme);
            let top = details.y.round();
            let bar = (details.x + details.width).round() - f32::from(SCROLLBAR_COLUMNS);
            let note = format!("{tiles} tiles, dark={}", theme.cosmic().is_dark);

            for column in 0..SCROLLBAR_COLUMNS {
                let x = bar + f32::from(column);
                let rail = drawn.at(Point::new(x, top + f32::from(BELOW_THUMB.start)));
                let thumb = drawn.at(Point::new(x, top + IN_THUMB));
                assert!(
                    !is_rounding_apart(thumb, rail),
                    "{note}: no thumb is drawn at {x} over {details:?}"
                );
                for row in BELOW_THUMB {
                    let at = Point::new(x, top + f32::from(row));
                    assert!(
                        is_rounding_apart(drawn.at(at), rail),
                        "{note}: a glyph lies under the scrollbar at {at:?}"
                    );
                }
            }
        }
    }
}
