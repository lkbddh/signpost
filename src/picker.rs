use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::profiles::{Tile, TileAction};

pub const COLUMNS: usize = 4;
pub const LONG_PRESS: std::time::Duration = std::time::Duration::from_millis(500);
/// How long the copy button shows its confirmation.
pub const COPIED_FLASH: std::time::Duration = std::time::Duration::from_millis(1500);
/// Tiles past the ninth have no digit key.
const LAST_KEYCAP: u8 = 9;
const ENTER: &str = "Enter";
const CTRL: &str = "Ctrl";

/// One row of a tile's actions menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    Open,
    Action(TileAction),
    OpenKeep,
    Copy,
}

impl MenuItem {
    /// The key chips shown at the row's end.
    #[must_use]
    pub fn shortcut(&self) -> &'static [&'static str] {
        match self {
            Self::Open => &[ENTER],
            Self::OpenKeep => &[CTRL, ENTER],
            Self::Copy => &[CTRL, "C"],
            Self::Action(_) => &[],
        }
    }

    /// Drawn above the row; never a focus stop, so `menu_focus` indexes `menu_items` directly.
    #[must_use]
    pub fn separator_before(&self) -> bool {
        matches!(self, Self::Copy)
    }
}

/// The app and profile a launch went to; survives the picker that tried it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub app_id: String,
    pub profile_key: Option<String>,
    pub label: String,
}

impl From<&Tile> for Target {
    fn from(tile: &Tile) -> Self {
        Self {
            app_id: tile.app_id.clone(),
            profile_key: tile.profile.as_ref().map(|p| p.key.clone()),
            label: tile.label.clone(),
        }
    }
}

impl Target {
    #[must_use]
    pub fn matches(&self, tile: &Tile) -> bool {
        self.app_id == tile.app_id
            && self.profile_key.as_deref() == tile.profile.as_ref().map(|p| p.key.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Move(i32, i32),
    DigitPress(u8),
    DigitRelease(u8),
    Activate {
        ctrl: bool,
    },
    Click {
        index: usize,
        keep_open: bool,
    },
    /// Opens a tile's menu: the focused tile's from the Menu key (`None`), a pressed tile's from the pointer.
    OpenMenu(Option<usize>),
    ChooseItem {
        index: usize,
        item: MenuItem,
        keep_open: bool,
    },
    CloseMenu,
    /// Pointer/finger went down on a tile (long-press tracking).
    PressStart(usize),
    /// Pointer/finger released over tile `index`: a normal click on the pressed tile unless a long-press already
    /// fired.
    PressEnd {
        index: usize,
        keep_open: bool,
    },
    /// The touch moved away or was lost.
    PressCancel,
    /// The pointer entered a tile.
    HoverStart(usize),
    /// The pointer left a tile, which also ends a press on it. The pointer area reports it twice per exit.
    HoverEnd(usize),
    /// The pointer left the whole grid, where a scrollable hides the pointer from its tiles.
    HoverClear,
    /// The long-press timer for press `seq` fired.
    LongPress(u64),
    Escape,
    FocusLost,
    /// Tab (`shift` for Shift+Tab): walks the open menu's rows; with no menu the native controls take over.
    Tab {
        shift: bool,
    },
    /// A mouse or touch press landed in the picker, whichever widget took it.
    PointerDown,
    Copy,
    TogglePin,
    SkipToNext,
    ToggleDetails,
    CopiedExpired(u64),
}

impl Input {
    /// A click-like gesture made with Ctrl held asks to keep the picker open; every other input is as is.
    #[must_use]
    pub fn with_ctrl(self, ctrl: bool) -> Self {
        match self {
            Self::Click { index, keep_open } => Self::Click {
                index,
                keep_open: keep_open || ctrl,
            },
            Self::PressEnd { index, keep_open } => Self::PressEnd {
                index,
                keep_open: keep_open || ctrl,
            },
            Self::ChooseItem {
                index,
                item,
                keep_open,
            } => Self::ChooseItem {
                index,
                item,
                keep_open: keep_open || ctrl,
            },
            other => other,
        }
    }

    /// Tab with Shift held walks backwards; every other input is as is.
    #[must_use]
    pub fn with_shift(self, shift: bool) -> Self {
        match self {
            Self::Tab { .. } => Self::Tab { shift },
            other => other,
        }
    }

    /// A key that moves the focus the keyboard shows, even where it cannot go further.
    #[must_use]
    pub fn is_navigation(&self) -> bool {
        matches!(
            self,
            Self::Move(..) | Self::DigitPress(_) | Self::OpenMenu(None) | Self::Tab { .. }
        )
    }

    /// A mouse or touch press, which takes the shown focus from the keyboard.
    fn is_press(&self) -> bool {
        matches!(
            self,
            Self::PointerDown
                | Self::PressStart(_)
                | Self::Click { .. }
                | Self::OpenMenu(Some(_))
                | Self::CloseMenu
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    None,
    Launch {
        index: usize,
        action: Option<TileAction>,
        keep_open: bool,
    },
    Cancel,
    Copy {
        uri: String,
        seq: u64,
    },
    /// Start a `LONG_PRESS` timer that delivers `Input::LongPress(seq)`.
    ArmLongPress(u64),
}

/// How the picker's layer surface holds the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keyboard {
    Exclusive,
    OnDemand,
}

/// What the app does once a launch attempt has settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// The picker shows the failure and stays, the failed tile focused.
    Failed,
    Close,
    /// The picker stays. `relax_focus` is set once, on the first launch that kept it open: its target may
    /// take the keyboard, so the surface stops holding it exclusively.
    KeepOpen {
        relax_focus: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent switches of the view, not the states of one machine"
)]
pub struct Picker {
    pub uri: String,
    pub tiles: Vec<Tile>,
    pub focus: usize,
    /// Whether the focus shows: the tile's ring and the menu's focused row. Off until the keyboard moves
    /// the focus and off again once the pointer takes over; `focus` and `menu_focus` stay where they are.
    pub focus_visible: bool,
    pub keep_open: bool,
    pub pinned: bool,
    pub error: Option<String>,
    pub failed: Option<Target>,
    pub details_open: bool,
    pub copied: Option<u64>,
    /// The tile under the pointer; styling only, the keyboard focus stays `focus`.
    pub hover: Option<usize>,
    pub menu: Option<usize>,
    /// The focused row of the open actions menu.
    pub menu_focus: usize,
    keyboard: Keyboard,
    armed: Option<u8>,
    press: Option<Press>,
    press_seq: u64,
    copy_seq: u64,
}

/// A pointer or finger held down on a tile.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Press {
    index: usize,
    seq: u64,
    /// Its release is not a click: a long-press fired, or the press only closed the menu.
    swallowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Enter,
    Escape,
    Menu,
    Tab,
    Digit(u8),
    CopyC,
}

impl Picker {
    #[must_use]
    pub fn new(uri: String, tiles: Vec<Tile>) -> Self {
        Self {
            uri,
            tiles,
            focus: 0,
            focus_visible: false,
            keep_open: false,
            pinned: false,
            error: None,
            failed: None,
            details_open: false,
            copied: None,
            hover: None,
            menu: None,
            menu_focus: 0,
            keyboard: Keyboard::Exclusive,
            armed: None,
            press: None,
            press_seq: 0,
            copy_seq: 0,
        }
    }

    /// A picker for a queued link: after a failed launch the tile that failed has focus again.
    #[must_use]
    pub fn from_link(link: QueuedLink, tiles: Vec<Tile>) -> Self {
        let mut picker = Self::new(link.uri, tiles);
        picker.error = link.error;
        picker.failed = link.failed;
        picker.focus_failed();
        picker
    }

    /// Whether the picker stays after a launch: one requested to keep it, or any launch while pinned.
    #[must_use]
    pub fn keeps_open(&self, requested: bool) -> bool {
        requested || self.pinned
    }

    /// Shows `message` and gives the failed tile focus again, wherever the focus went meanwhile. An open
    /// menu is not yanked away: it keeps the focus, and the banner still names the failed tile.
    pub fn fail(&mut self, message: String, target: Option<Target>) {
        self.error = Some(message);
        self.failed = target;
        self.focus_failed();
    }

    fn focus_failed(&mut self) {
        if self.menu.is_some() {
            return;
        }
        let failed = self.failed.as_ref();
        if let Some(index) = failed.and_then(|t| self.tiles.iter().position(|tile| t.matches(tile)))
        {
            self.focus = index;
        }
    }

    /// Launch attempt of tile `index` settled, `requested` keep-open and `error` its failure, if any.
    pub fn settle(&mut self, index: usize, requested: bool, error: Option<String>) -> Settled {
        if let Some(message) = error {
            self.fail(message, self.tiles.get(index).map(Target::from));
            return Settled::Failed;
        }
        if !self.keeps_open(requested) {
            return Settled::Close;
        }
        self.clear_failure();
        let relax_focus = self.keyboard == Keyboard::Exclusive;
        self.keyboard = Keyboard::OnDemand;
        Settled::KeepOpen { relax_focus }
    }

    pub fn clear_failure(&mut self) {
        self.error = None;
        self.failed = None;
        self.details_open = false;
    }

    /// Rows of the menu of the tile whose menu is open: Open, Private, desktop actions, keep, copy.
    #[must_use]
    pub fn menu_items(&self) -> Vec<MenuItem> {
        let Some(tile) = self.menu.and_then(|i| self.tiles.get(i)) else {
            return Vec::new();
        };
        let (private, desktop): (Vec<_>, Vec<_>) = tile
            .actions
            .iter()
            .cloned()
            .partition(|a| matches!(a, TileAction::Private));
        std::iter::once(MenuItem::Open)
            .chain(private.into_iter().chain(desktop).map(MenuItem::Action))
            .chain([MenuItem::OpenKeep, MenuItem::Copy])
            .collect()
    }

    /// Opens the actions menu of tile `index` with its first row focused. The release of a press held meanwhile is
    /// not a click.
    fn open_menu(&mut self, index: usize) {
        if index < self.tiles.len() {
            self.focus = index;
            self.menu = Some(index);
            self.menu_focus = 0;
            self.armed = None;
            if let Some(press) = &mut self.press {
                press.swallowed = true;
            }
        }
    }

    /// Moves within the open menu (any arrow), or within the grid's rows.
    fn move_focus(&mut self, dx: i32, dy: i32) {
        if self.menu.is_some() {
            let last = self.menu_items().len().saturating_sub(1);
            self.menu_focus = step(self.menu_focus, i64::from(dx) + i64::from(dy), last);
            return;
        }
        let rows = rows(&self.tiles);
        let Some((row, col)) = grid_position(&rows, self.focus) else {
            return;
        };
        let target = &rows[step(row, i64::from(dy), rows.len() - 1)];
        self.focus = target[step(col, i64::from(dx), target.len() - 1)];
    }

    /// Tab walks the rows of the open menu, round and round. With no menu the native controls take the
    /// focus over, so the tile's ring goes.
    fn tab(&mut self, shift: bool) {
        let rows = self.menu_items().len();
        self.focus_visible = rows > 0;
        if rows == 0 {
            return;
        }
        let by = if shift { rows - 1 } else { 1 };
        self.menu_focus = (self.menu_focus + by) % rows;
    }

    /// Digit `d` arms and shows its tile, if there is one.
    fn digit_press(&mut self, d: u8) {
        let i = usize::from(d).wrapping_sub(1);
        if i >= self.tiles.len() {
            return;
        }
        self.focus = i;
        self.focus_visible = true;
        self.armed = Some(d);
    }

    fn digit_release(&mut self, d: u8) -> Outcome {
        if self.armed != Some(d) {
            return Outcome::None; // unarmed or a different digit: the armed one stays armed
        }
        self.armed = None;
        let i = usize::from(d) - 1;
        if i != self.focus {
            return Outcome::None;
        }
        self.launch(i, None, false)
    }

    /// Launches tile `index`; `requested` is the gesture's own keep-open request, the pin adds to it.
    fn launch(&mut self, index: usize, action: Option<TileAction>, requested: bool) -> Outcome {
        if index >= self.tiles.len() {
            return Outcome::None;
        }
        self.focus = index;
        self.menu = None;
        Outcome::Launch {
            index,
            action,
            keep_open: self.keeps_open(requested),
        }
    }

    /// Copies the link and starts the confirmation flash; the menu closes.
    fn copy(&mut self) -> Outcome {
        self.menu = None;
        self.copy_seq += 1;
        self.copied = Some(self.copy_seq);
        Outcome::Copy {
            uri: self.uri.clone(),
            seq: self.copy_seq,
        }
    }

    /// A row of the menu of tile `index` chosen by click or Enter; `keep_open` is the gesture's Ctrl.
    fn choose(&mut self, index: usize, item: MenuItem, keep_open: bool) -> Outcome {
        match item {
            MenuItem::Open => self.launch(index, None, keep_open),
            MenuItem::Action(action) => self.launch(index, Some(action), keep_open),
            MenuItem::OpenKeep => self.launch(index, None, true),
            MenuItem::Copy => self.copy(),
        }
    }

    /// Enter: the focused menu row while the menu is open (Ctrl+Enter: Open and keep, whatever row),
    /// otherwise the focused tile.
    fn activate(&mut self, ctrl: bool) -> Outcome {
        let Some(index) = self.menu else {
            return self.launch(self.focus, None, ctrl);
        };
        let item = if ctrl {
            Some(MenuItem::OpenKeep)
        } else {
            self.menu_items().get(self.menu_focus).cloned()
        };
        item.map_or(Outcome::None, |item| self.choose(index, item, ctrl))
    }

    /// A click on a tile launches it, unless it only closes the open menu.
    fn click(&mut self, index: usize, keep_open: bool) -> Outcome {
        if self.menu.take().is_some() {
            return Outcome::None;
        }
        self.launch(index, None, keep_open)
    }

    /// A release over tile `index` ends only a press on that tile: with two fingers down, one lifting from its
    /// own tile does not launch the other's.
    fn press_end(&mut self, index: usize, keep_open: bool) -> Outcome {
        match self.press.take_if(|press| press.index == index) {
            Some(Press {
                swallowed: false, ..
            }) => self.launch(index, None, keep_open),
            _ => Outcome::None,
        }
    }

    /// The pointer left tile `index`, which is then not hovered and has no press under way. An exit
    /// that arrives late, after the pointer entered another tile, leaves that one alone.
    fn leave(&mut self, index: usize) {
        self.hover.take_if(|hovered| *hovered == index);
        self.press.take_if(|press| press.index == index);
    }

    fn long_press(&mut self, seq: u64) {
        let Some(index) = self
            .press
            .as_ref()
            .filter(|p| p.seq == seq)
            .map(|p| p.index)
        else {
            return;
        };
        // Which also keeps its release from launching.
        self.open_menu(index);
        self.focus_visible = false;
    }

    fn focus_lost(&mut self) -> Outcome {
        // Pending gestures die with the focus, also in a picker that stays open.
        self.armed = None;
        self.press = None;
        if self.keep_open || self.pinned {
            Outcome::None
        } else {
            Outcome::Cancel
        }
    }

    /// A `Launch` leaves `keep_open` alone: the caller sets it once it admits the launch.
    pub fn handle(&mut self, input: Input) -> Outcome {
        if input.is_press() {
            self.focus_visible = false;
        }
        match input {
            Input::Move(dx, dy) => {
                self.focus_visible = true;
                self.move_focus(dx, dy);
                Outcome::None
            }
            // Digits pick tiles, which the open menu covers.
            Input::DigitPress(_) | Input::DigitRelease(_) if self.menu.is_some() => Outcome::None,
            Input::DigitPress(d) => {
                self.digit_press(d);
                Outcome::None
            }
            Input::DigitRelease(d) => self.digit_release(d),
            Input::PressStart(index) => {
                self.press_seq += 1;
                self.press = Some(Press {
                    index,
                    seq: self.press_seq,
                    swallowed: self.menu.take().is_some(),
                });
                Outcome::ArmLongPress(self.press_seq)
            }
            Input::PressEnd { index, keep_open } => self.press_end(index, keep_open),
            Input::PressCancel => {
                self.press = None;
                Outcome::None
            }
            Input::HoverStart(index) => {
                self.hover = Some(index);
                Outcome::None
            }
            Input::HoverEnd(index) => {
                self.leave(index);
                Outcome::None
            }
            Input::HoverClear => {
                self.hover = None;
                self.press = None;
                Outcome::None
            }
            Input::LongPress(seq) => {
                self.long_press(seq);
                Outcome::None
            }
            Input::Activate { ctrl } => self.activate(ctrl),
            Input::Click { index, keep_open } => self.click(index, keep_open),
            Input::OpenMenu(None) => {
                self.open_menu(self.focus);
                self.focus_visible = self.menu.is_some();
                Outcome::None
            }
            Input::OpenMenu(Some(index)) => {
                self.open_menu(index);
                Outcome::None
            }
            Input::ChooseItem {
                index,
                item,
                keep_open,
            } => self.choose(index, item, keep_open),
            Input::TogglePin => {
                self.pinned = !self.pinned;
                // Unpinning also drops a keep-open a pinned launch latched, so focus loss closes again.
                self.keep_open &= self.pinned;
                Outcome::None
            }
            Input::SkipToNext => Outcome::Cancel,
            Input::ToggleDetails => {
                self.details_open = !self.details_open;
                Outcome::None
            }
            Input::CopiedExpired(seq) => {
                self.copied = self.copied.filter(|&current| current != seq);
                Outcome::None
            }
            Input::CloseMenu => {
                self.menu = None;
                Outcome::None
            }
            Input::Escape => {
                if self.menu.take().is_some() {
                    Outcome::None
                } else {
                    Outcome::Cancel
                }
            }
            Input::FocusLost => self.focus_lost(),
            Input::Tab { shift } => {
                self.tab(shift);
                Outcome::None
            }
            Input::PointerDown => Outcome::None,
            Input::Copy => self.copy(),
        }
    }
}

/// Keyboard → Input. Digits 1–9 act on press (focus) and release (activate); Ctrl+C copies.
#[must_use]
pub fn map_key(key: Key, ctrl: bool, pressed: bool) -> Option<Input> {
    match (key, pressed) {
        (Key::Digit(d), true) if (1..=9).contains(&d) => Some(Input::DigitPress(d)),
        (Key::Digit(d), false) if (1..=9).contains(&d) => Some(Input::DigitRelease(d)),
        (_, false) | (Key::Digit(_), true) => None,
        (Key::Left, true) => Some(Input::Move(-1, 0)),
        (Key::Right, true) => Some(Input::Move(1, 0)),
        (Key::Up, true) => Some(Input::Move(0, -1)),
        (Key::Down, true) => Some(Input::Move(0, 1)),
        (Key::Enter, true) => Some(Input::Activate { ctrl }),
        (Key::Escape, true) => Some(Input::Escape),
        (Key::Menu, true) => Some(Input::OpenMenu(None)),
        (Key::Tab, true) => (!ctrl).then_some(Input::Tab { shift: false }),
        (Key::CopyC, true) => ctrl.then_some(Input::Copy),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedLink {
    pub uri: String,
    pub error: Option<String>,
    pub failed: Option<Target>,
}

/// A launch that failed after it was reported started, e.g. the program exited at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchFailure {
    pub uri: String,
    pub failed: Target,
    pub message: String,
}

/// An open picker shows the failure in place; for a closed one the link comes back, carrying it, to be
/// presented again.
#[must_use]
pub fn early_failure(picker: Option<&mut Picker>, failure: LaunchFailure) -> Option<QueuedLink> {
    let LaunchFailure {
        uri,
        failed,
        message,
    } = failure;
    let Some(picker) = picker else {
        return Some(QueuedLink {
            uri,
            error: Some(message),
            failed: Some(failed),
        });
    };
    picker.fail(message, Some(failed));
    None
}

impl QueuedLink {
    #[must_use]
    pub fn new(uri: String) -> Self {
        Self {
            uri,
            error: None,
            failed: None,
        }
    }
}

/// `from` moved by `by` and kept within `0..=last`.
fn step(from: usize, by: i64, last: usize) -> usize {
    let target = i64::try_from(from).unwrap_or(i64::MAX).saturating_add(by);
    usize::try_from(target.max(0))
        .unwrap_or(usize::MAX)
        .min(last)
}

/// The grid: every tile, in order, fills rows of `COLUMNS`, whatever app it belongs to.
#[must_use]
pub fn rows(tiles: &[Tile]) -> Vec<Vec<usize>> {
    let indices: Vec<usize> = (0..tiles.len()).collect();
    indices.chunks(COLUMNS).map(<[usize]>::to_vec).collect()
}

/// The (row, column) of tile `index` in `rows`.
#[must_use]
pub fn grid_position(rows: &[Vec<usize>], index: usize) -> Option<(usize, usize)> {
    rows.iter().enumerate().find_map(|(row, cells)| {
        cells
            .iter()
            .position(|&cell| cell == index)
            .map(|col| (row, col))
    })
}

/// The digit key that picks tile `index`, for the first nine tiles.
#[must_use]
pub fn keycap(index: usize) -> Option<u8> {
    u8::try_from(index)
        .ok()
        .filter(|&i| i < LAST_KEYCAP)
        .map(|i| i + 1)
}

/// How many links may wait for a picker at once, counting those still on their way to the queue.
pub const MAX_WAITING_LINKS: usize = 64;

/// The links accepted but not yet presented: still in the bus's channel, or waiting in the [`LinkQueue`].
/// The bus counts a call's links before queueing them, so a flood is turned away instead of retained.
#[derive(Debug, Clone, Default)]
pub struct Backlog(Arc<AtomicUsize>);

impl Backlog {
    /// Counts `links` more, unless that would leave more than `MAX_WAITING_LINKS` waiting. A call
    /// with no links is always accepted.
    #[must_use]
    #[allow(
        deprecated,
        reason = "`try_update`, its new name, is newer than the supported Rust"
    )]
    pub fn try_add(&self, links: usize) -> bool {
        self.0
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |waiting| {
                let total = waiting.saturating_add(links);
                (links == 0 || total <= MAX_WAITING_LINKS).then_some(total)
            })
            .is_ok()
    }

    /// Counts links that cannot be turned away: the starter's own, which `admit_starter_links` keeps within
    /// the limit, and a failed launch's link coming back.
    pub fn add(&self, links: usize) {
        self.0.fetch_add(links, Ordering::SeqCst);
    }

    /// `links` were presented or dropped. The count stops at zero.
    #[allow(
        deprecated,
        reason = "`try_update`, its new name, is newer than the supported Rust"
    )]
    pub fn release(&self, links: usize) {
        let _ = self
            .0
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |waiting| {
                Some(waiting.saturating_sub(links))
            });
    }

    #[must_use]
    pub fn waiting(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// How soon after a link the same link is taken for that one sent twice.
pub const DUPLICATE_WINDOW: std::time::Duration = std::time::Duration::from_secs(1);

/// A link that came in over the bus and when, so the same link again at once is known for a repeat. The
/// app's own links have none and are never taken for one.
type Arrival = Option<(String, std::time::Instant)>;

/// Where the presented picker is.
#[derive(Debug, Default)]
enum Stage {
    #[default]
    Idle,
    Showing(Arrival),
    /// The picker is closing, and the next link waits until its surface is gone.
    Closing,
}

/// At most one picker is presented; further links wait and are presented in order, each once the picker
/// before it is gone from the screen.
#[derive(Debug, Default)]
pub struct LinkQueue {
    waiting: std::collections::VecDeque<(QueuedLink, Arrival)>,
    stage: Stage,
    backlog: Backlog,
}

impl LinkQueue {
    /// A queue that frees its room in `backlog` as links leave it.
    #[must_use]
    pub fn counting(backlog: Backlog) -> Self {
        Self {
            backlog,
            ..Self::default()
        }
    }

    /// Returns the link to present now, if no picker is presented; otherwise queues it.
    pub fn push(&mut self, link: QueuedLink) -> Option<QueuedLink> {
        self.admit(link, None)
    }

    /// [`LinkQueue::push`] for a link that has just come in. The same link as the one showing or the last one
    /// waiting, within [`DUPLICATE_WINDOW`] of when that came in, is one link sent twice, as some terminals do,
    /// and is dropped with its room.
    pub fn arrive(&mut self, link: QueuedLink, now: std::time::Instant) -> Option<QueuedLink> {
        let repeats = |arrival: &Arrival| {
            arrival.as_ref().is_some_and(|(uri, at)| {
                *uri == link.uri && now.saturating_duration_since(*at) < DUPLICATE_WINDOW
            })
        };
        let shown = matches!(&self.stage, Stage::Showing(arrival) if repeats(arrival));
        let last = self
            .waiting
            .back()
            .is_some_and(|(_, arrival)| repeats(arrival));
        if shown || last {
            tracing::debug!("dropped a link sent twice");
            self.backlog.release(1);
            return None;
        }
        let arrival = Some((link.uri.clone(), now));
        self.admit(link, arrival)
    }

    fn admit(&mut self, link: QueuedLink, arrival: Arrival) -> Option<QueuedLink> {
        if !matches!(self.stage, Stage::Idle) {
            self.waiting.push_back((link, arrival));
            return None;
        }
        Some(self.present(link, arrival))
    }

    fn present(&mut self, link: QueuedLink, arrival: Arrival) -> QueuedLink {
        self.stage = Stage::Showing(arrival);
        self.backlog.release(1);
        link
    }

    /// The presented picker is closing: nothing is presented until it is gone, and its link sent again is
    /// no longer a repeat.
    pub fn closing(&mut self) {
        self.stage = Stage::Closing;
    }

    /// The closed picker is gone from the screen; returns the next link to present, if any.
    pub fn gone(&mut self) -> Option<QueuedLink> {
        self.stage = Stage::Idle;
        let (link, arrival) = self.waiting.pop_front()?;
        Some(self.present(link, arrival))
    }

    /// Drops every waiting link.
    pub fn discard_waiting(&mut self) {
        self.backlog.release(self.waiting.len());
        self.waiting.clear();
    }

    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    #[must_use]
    pub fn peek(&self) -> Option<&QueuedLink> {
        self.waiting.front().map(|(link, _)| link)
    }
}

#[cfg(test)]
mod focus_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod tests;
