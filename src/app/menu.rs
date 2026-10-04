//! The actions menu in a popup of its own: where it opens, whose keys and presses it takes, and how it
//! goes. The picker stays the one model of the menu; the popup only shows it.

use cosmic::Element;
use cosmic::cctk::wayland_protocols::xdg::shell::client::xdg_positioner::{Anchor, Gravity};
use cosmic::iced::runtime::platform_specific::wayland::popup::{SctkPopupSettings, SctkPositioner};
use cosmic::iced::{Rectangle, window};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::surface::surface_task;
use cosmic::widget;

use super::{App, Message, PICKER_SCROLL, PickerState, Task, snap};
use crate::picker::{Input, Outcome, Picker};
use crate::picker_view::{MENU_GAP, grid_scroll, tile_id};
use crate::{frost, native_focus, widget_bounds};

/// The popup that shows the open actions menu, and the picker whose menu it shows. The compositor grabs for
/// one popup at a time, and a picker is presented at a time.
#[derive(Clone, Copy)]
pub(super) struct MenuPopup {
    id: window::Id,
    owner: window::Id,
}

/// A whole number of pixels.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a popup and a tile are a few hundred pixels"
)]
fn pixels(logical: f32) -> i32 {
    logical.round() as i32
}

/// The request for `popup`: beside the tile at `tile`, and grabbing, so that the keys and presses of the
/// compositor's grab go to it. It has no size: the autosize container at the root of its view, which is as
/// big as the menu, asks the surface for one.
fn popup_request(popup: MenuPopup, tile: Rectangle) -> cosmic::surface::Action<Message> {
    let MenuPopup { id, owner } = popup;
    let anchor_rect = Rectangle {
        x: pixels(tile.x),
        y: pixels(tile.y),
        width: pixels(tile.width),
        height: pixels(tile.height),
    };
    app_popup::<App>(
        |_| frost::live_settings(false),
        move |_| SctkPopupSettings {
            parent: owner,
            id,
            positioner: SctkPositioner {
                anchor_rect,
                anchor: Anchor::TopRight,
                gravity: Gravity::BottomRight,
                offset: (pixels(MENU_GAP), 0),
                ..SctkPositioner::default()
            },
            parent_size: None,
            grab: true,
            close_with_children: false,
            input_zone: None,
        },
        Some(Box::new(move |app: &App| {
            app.menu_view(owner).map(cosmic::Action::App)
        })),
    )
}

/// Destroys `popup`.
pub(super) fn destroy(popup: window::Id) -> Task<Message> {
    surface_task(destroy_popup(popup))
}

/// The grid scrolled to the focused tile when the keyboard opened its menu: the pointer opens menus on
/// tiles it sees, and the grid stays where the user left it.
fn reveal_tile(picker: &Picker) -> Task<Message> {
    if !picker.focus_visible {
        return Task::none();
    }
    snap(PICKER_SCROLL.clone(), grid_scroll(picker))
}

impl App {
    /// The picker surface `surface` belongs to: itself, or the picker whose menu it is the popup of, or
    /// whose launch keeps it for its token.
    pub(super) fn picker_of(&self, surface: window::Id) -> window::Id {
        self.menu_popup
            .as_ref()
            .filter(|popup| popup.id == surface)
            .map(|popup| popup.owner)
            .or_else(|| self.holder_of(surface))
            .unwrap_or(surface)
    }

    /// The picker whose launch holds `popup` for its token.
    fn holder_of(&self, popup: window::Id) -> Option<window::Id> {
        self.pickers
            .iter()
            .find_map(|(id, state)| state.holds(popup).then_some(*id))
    }

    /// Clears the native focus of the widgets of picker `id` and of its menu's popup, so that the focus the
    /// picker shows is the only one there, and leaves the focus of the other windows alone.
    pub(super) fn clear_native_focus(&self, id: window::Id) -> Task<Message> {
        let popup = self
            .menu_popup
            .as_ref()
            .filter(|popup| popup.owner == id)
            .map(|popup| popup.id);
        let windows = std::iter::once(id).chain(popup).collect();
        cosmic::iced::runtime::task::effect(cosmic::iced::runtime::Action::widget(
            native_focus::unfocus_in(windows),
        ))
    }

    /// Brings the popup of picker `id`'s menu in line with the menu it has now, `before` being the tile whose
    /// menu it had: the popup of a menu that closed or moved goes, and an open menu gets one once the grid
    /// shows its tile and the tile can be measured.
    pub(super) fn follow_menu(&mut self, id: window::Id, before: Option<usize>) -> Task<Message> {
        let Some(picker) = self.picker(id).filter(|picker| picker.menu != before) else {
            return Task::none();
        };
        let Some(tile) = picker.menu else {
            return self.destroy_menu_popup(id);
        };
        let reveal = reveal_tile(picker);
        let anchor = widget_bounds::bounds_of(tile_id(tile))
            .map(move |bounds| cosmic::Action::App(Message::MenuAnchored { id, tile, bounds }));
        self.destroy_popups(id).chain(reveal).chain(anchor)
    }

    /// Picker `id`'s menu at `tile` is anchored at `bounds`, `None` where no tile answered: it gets its popup
    /// there, unless it has closed or moved on, or has a popup already.
    pub(super) fn menu_anchored(
        &mut self,
        id: window::Id,
        tile: usize,
        bounds: Option<Rectangle>,
    ) -> Task<Message> {
        let open = self
            .picker(id)
            .is_some_and(|picker| picker.menu == Some(tile));
        if !open || self.menu_popup.is_some() {
            return Task::none();
        }
        let Some(bounds) = bounds else {
            return self.apply(id, Input::CloseMenu);
        };
        let popup = MenuPopup {
            id: window::Id::unique(),
            owner: id,
        };
        let request = popup_request(popup, bounds);
        self.menu_popup = Some(popup);
        surface_task(request)
    }

    /// Destroys the popup of picker `owner`'s menu, if it has one.
    pub(super) fn destroy_menu_popup(&mut self, owner: window::Id) -> Task<Message> {
        self.menu_popup
            .take_if(|popup| popup.owner == owner)
            .map_or_else(Task::none, |popup| destroy(popup.id))
    }

    /// Destroys every popup picker `owner` has, the one its menu shows and the one its launch holds for its
    /// token, so that a picker never has two.
    pub(super) fn destroy_popups(&mut self, owner: window::Id) -> Task<Message> {
        let held = self
            .pickers
            .get_mut(&owner)
            .and_then(PickerState::take_held_popup);
        self.destroy_menu_popup(owner)
            .chain(held.map_or_else(Task::none, destroy))
    }

    /// Esc in `surface`, the popup a launch holds for its token: it goes at once and the launch goes on.
    /// `None` for any other surface.
    pub(super) fn dismiss_held_popup(&mut self, surface: window::Id) -> Option<Task<Message>> {
        let owner = self.holder_of(surface)?;
        let held = self.pickers.get_mut(&owner)?.take_held_popup()?;
        Some(destroy(held))
    }

    /// The popup of picker `id`'s menu when `outcome` is the launch the menu chose, taken from the menu: the
    /// launch asks for its token from the popup, which has the input that chose the row, and the popup goes
    /// once the token is in.
    pub(super) fn claim_popup(&mut self, id: window::Id, outcome: &Outcome) -> Option<window::Id> {
        if !matches!(outcome, Outcome::Launch { .. }) {
            return None;
        }
        self.menu_popup
            .take_if(|popup| popup.owner == id)
            .map(|popup| popup.id)
    }

    /// Destroys the popup that picker `id`'s launch `attempt` has waited with for its token, if the token has
    /// come or been given up on.
    pub(super) fn release_popup(&mut self, id: window::Id, attempt: u64) -> Task<Message> {
        self.pickers
            .get_mut(&id)
            .filter(|state| state.awaiting_token(attempt))
            .and_then(PickerState::take_held_popup)
            .map_or_else(Task::none, destroy)
    }

    /// `surface` is the popup of an open menu and it is gone, closed by the compositor or destroyed here: the
    /// menu closes and the picker stays. `None` for any other surface, a popup that was replaced included.
    pub(super) fn menu_popup_closed(&mut self, surface: window::Id) -> Option<Task<Message>> {
        let popup = self.menu_popup.take_if(|popup| popup.id == surface)?;
        Some(self.apply(popup.owner, Input::CloseMenu))
    }

    /// The content of the popup of picker `owner`'s menu, under a theme of its own: the popup is opaque
    /// and a surface of its own, so it takes nothing from the picker's frost.
    pub(super) fn menu_view(&self, owner: window::Id) -> Element<'_, Message> {
        let Some(picker) = self.picker(owner) else {
            return widget::Space::new().into();
        };
        let theme = cosmic::theme::active();
        let view = self.view_of(owner, picker, &theme).menu_popup();
        frost::scoped(theme, false, view)
    }
}
