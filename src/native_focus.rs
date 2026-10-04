//! The native focus of some windows. The runtime applies a widget operation to the widgets of every window
//! it has, so an operation meant for some windows has to ask which window it is visiting.

use cosmic::iced::advanced::widget::operation::Focusable;
use cosmic::iced::advanced::widget::{Id, Operation};
use cosmic::iced::{Rectangle, window};

/// The operation that takes the native focus from the widgets of `windows`, menu rows and header buttons
/// alike, and from the widgets of no other window.
pub(crate) fn unfocus_in(windows: Vec<window::Id>) -> impl Operation {
    Unfocus {
        windows,
        visiting: None,
    }
}

struct Unfocus {
    windows: Vec<window::Id>,
    /// The window the runtime said it is visiting; it says so before it visits each one.
    visiting: Option<window::Id>,
}

impl Operation for Unfocus {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if self
            .visiting
            .is_some_and(|window| self.windows.contains(&window))
        {
            state.unfocus();
        }
    }

    fn set_window_id(&mut self, id: window::Id) {
        self.visiting = Some(id);
    }
}
