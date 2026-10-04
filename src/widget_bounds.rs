//! Where a widget is on the surface of its window, whatever scrolls it.

use cosmic::iced::advanced::widget::operation::{Outcome, Scrollable};
use cosmic::iced::advanced::widget::{Id, Operation};
use cosmic::iced::runtime::task;
use cosmic::iced::{Rectangle, Task, Vector};

/// The bounds of the container named `target` on its window's surface; `None` when no window has one.
pub(crate) fn bounds_of(target: Id) -> Task<Option<Rectangle>> {
    task::widget(OnSurface {
        target,
        found: None,
        scrolled: Vec::new(),
    })
}

/// A container reports where it was laid out, in its scrollables' content, so what each scrollable that
/// holds it has scrolled is taken off. The scrollable reports after its content, so the container is
/// inside the ones that began before it was found.
struct OnSurface {
    target: Id,
    found: Option<Rectangle>,
    /// For each scrollable being visited, whether the container was found before its content began.
    scrolled: Vec<bool>,
}

impl Operation<Option<Rectangle>> for OnSurface {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<Rectangle>>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if self.found.is_none() && id == Some(&self.target) {
            self.found = Some(bounds);
        }
    }

    fn pre_operation(&mut self, _id: Option<&Id>) {
        self.scrolled.push(self.found.is_some());
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        _bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        let found_before = self.scrolled.pop().unwrap_or(true);
        if found_before {
            return;
        }
        self.found = self.found.map(|bounds| bounds - translation);
    }

    fn finish(&self) -> Outcome<Option<Rectangle>> {
        Outcome::Some(self.found)
    }
}

#[cfg(test)]
mod tests {
    use cosmic::iced::advanced::widget::operation::scrollable::{AbsoluteOffset, scroll_to};
    use cosmic::iced::runtime::Action as RuntimeAction;
    use cosmic::iced::runtime::user_interface::UserInterface;
    use cosmic::iced::{Length, Point, Size};
    use cosmic::widget::{self, column, container};
    use cosmic::{Element, Renderer, Theme};
    use futures_util::{FutureExt, StreamExt};

    use super::*;
    use crate::test_support::headless::{self, Frames};

    const SCRATCH_HOME: &str = "SIGNPOST_WIDGET_BOUNDS_SCRATCH_HOME";
    const SURFACE: Size = Size::new(200.0, 300.0);
    const TARGET_SIZE: Size = Size::new(20.0, 10.0);
    const ABOVE: f32 = 30.0;
    const VIEWPORT: f32 = 60.0;

    fn in_scratch_home(name: &str) -> bool {
        headless::in_scratch_home(SCRATCH_HOME, &format!("widget_bounds::tests::{name}"))
    }

    fn target() -> Id {
        Id::new("the target")
    }

    fn spacer<'a>(height: f32) -> Element<'a, ()> {
        widget::Space::new().height(height).into()
    }

    fn target_box<'a>() -> Element<'a, ()> {
        container(
            widget::Space::new()
                .width(TARGET_SIZE.width)
                .height(TARGET_SIZE.height),
        )
        .id(target())
        .into()
    }

    fn scrolled_by(id: &'static str, offset: f32) -> impl Operation {
        scroll_to(
            Id::new(id),
            AbsoluteOffset {
                x: None,
                y: Some(offset),
            },
        )
    }

    fn scrolling<'a>(
        id: &'static str,
        height: f32,
        content: impl Into<Element<'a, ()>>,
    ) -> Element<'a, ()> {
        widget::scrollable(content)
            .id(Id::new(id))
            .height(Length::Fixed(height))
            .into()
    }

    /// The bounds the task of [`bounds_of`] reports for the interface.
    fn found(
        ui: &mut UserInterface<'_, (), Theme, Renderer>,
        renderer: &Renderer,
    ) -> Option<Rectangle> {
        let mut stream = task::into_stream(bounds_of(target())).expect("a task that runs");
        let Some(RuntimeAction::Widget(mut operation)) = stream.next().now_or_never().flatten()
        else {
            panic!("the task operates on the widgets");
        };
        ui.operate(renderer, operation.as_mut());
        assert!(matches!(operation.finish(), Outcome::Some(())));
        drop(operation);
        let Some(RuntimeAction::Output(found)) = stream.next().now_or_never().flatten() else {
            panic!("the task reports what it found");
        };
        found
    }

    fn at(y: f32) -> Rectangle {
        Rectangle::new(Point::new(0.0, y), TARGET_SIZE)
    }

    #[test]
    fn a_container_is_where_its_scrollable_leaves_it() {
        if !in_scratch_home("a_container_is_where_its_scrollable_leaves_it") {
            return;
        }
        let list = column::with_children(vec![spacer(100.0), target_box()]);
        let view = column::with_children(vec![spacer(ABOVE), scrolling("only", VIEWPORT, list)]);
        let mut frames = Frames::new(SURFACE);
        let mut ui = frames.build(view);
        assert_eq!(found(&mut ui, &frames.renderer), Some(at(ABOVE + 100.0)));
        ui.operate(&frames.renderer, &mut scrolled_by("only", 50.0));
        assert_eq!(
            found(&mut ui, &frames.renderer),
            Some(at(ABOVE + 100.0 - 50.0)),
            "the content moved up by the scroll"
        );
    }

    #[test]
    fn nested_scrollables_each_move_it_and_a_later_one_does_not() {
        if !in_scratch_home("nested_scrollables_each_move_it_and_a_later_one_does_not") {
            return;
        }
        let inner = scrolling(
            "inner",
            40.0,
            column::with_children(vec![spacer(60.0), target_box()]),
        );
        let outer = scrolling(
            "outer",
            80.0,
            column::with_children(vec![spacer(50.0), inner]),
        );
        let after = scrolling("after", VIEWPORT, spacer(200.0));
        let view = column::with_children(vec![spacer(ABOVE), outer, after]);
        let mut frames = Frames::new(SURFACE);
        let mut ui = frames.build(view);
        ui.operate(&frames.renderer, &mut scrolled_by("outer", 10.0));
        ui.operate(&frames.renderer, &mut scrolled_by("inner", 30.0));
        ui.operate(&frames.renderer, &mut scrolled_by("after", 20.0));
        let laid_out = ABOVE + 50.0 + 60.0;
        assert_eq!(
            found(&mut ui, &frames.renderer),
            Some(at(laid_out - 30.0 - 10.0))
        );
    }

    #[test]
    fn a_container_no_window_has_is_not_found() {
        if !in_scratch_home("a_container_no_window_has_is_not_found") {
            return;
        }
        let mut frames = Frames::new(SURFACE);
        let mut ui = frames.build(spacer(10.0));
        assert_eq!(found(&mut ui, &frames.renderer), None);
    }
}
