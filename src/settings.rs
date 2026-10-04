use std::path::PathBuf;

use crate::fl;
use crate::index::AppEntry;
use crate::profiles::{
    Env, Profile, Rgb, Tile, TileAction, link_tiles_from, profiles_for, shown_icon,
};
use crate::setup::{self, DESKTOP_FILE, Record, SetupError, Snapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRow {
    pub key: String,
    pub label: String,
    pub dir: PathBuf,
    /// The browser's own color, before any override.
    pub seed: Option<Rgb>,
    /// The 1-based picker tile that launches this profile.
    pub tile: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRow {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub flatpak: bool,
    pub profiles: Vec<ProfileRow>,
    /// The app's tiles offer a private window.
    pub private: bool,
    /// Names of the URL-taking desktop actions its tiles offer.
    pub desktop_actions: Vec<String>,
}

/// One row per app, in the order and numbering of [`link_tiles`](crate::profiles::link_tiles) for the same
/// `entries`; each app's profiles are discovered once, for its tiles and its rows alike.
#[must_use]
pub fn inventory(entries: &[AppEntry], env: &Env) -> Vec<AppRow> {
    let discovered: Vec<Vec<Profile>> = entries.iter().map(|e| profiles_for(e, env)).collect();
    let tiles = link_tiles_from(entries, &discovered);
    entries
        .iter()
        .zip(discovered)
        .map(|(entry, profiles)| {
            let own: Vec<(usize, &Tile)> = tiles
                .iter()
                .enumerate()
                .filter(|(_, tile)| tile.app_id == entry.id)
                .collect();
            let mut row = app_row(entry, &own, profiles);
            row.icon = shown_icon(row.icon.as_deref(), env.view);
            row
        })
        .collect()
}

/// `own` are the app's tiles with their 0-based picker positions; they share one action list.
fn app_row(entry: &AppEntry, own: &[(usize, &Tile)], profiles: Vec<Profile>) -> AppRow {
    let first = own.first().map(|(_, tile)| *tile);
    let actions = first.map_or(&[][..], |tile| tile.actions.as_slice());
    let single = profiles.len() == 1;
    AppRow {
        id: entry.id.clone(),
        name: entry.name.clone(),
        icon: entry.icon.clone(),
        flatpak: first.is_some_and(|tile| tile.flatpak),
        profiles: profiles
            .into_iter()
            .map(|profile| profile_row(profile, own, single))
            .collect(),
        private: actions.contains(&TileAction::Private),
        desktop_actions: actions
            .iter()
            .filter_map(|action| match action {
                TileAction::Desktop { label, .. } => Some(label.clone()),
                TileAction::Private => None,
            })
            .collect(),
    }
}

/// A lone profile has no tile of its own: its plain tile launches it.
fn profile_row(profile: Profile, own: &[(usize, &Tile)], single: bool) -> ProfileRow {
    let tile = own
        .iter()
        .find(|(_, tile)| {
            tile.profile
                .as_ref()
                .map_or(single, |shown| shown.key == profile.key)
        })
        .map(|(position, _)| position + 1);
    ProfileRow {
        key: profile.key,
        label: profile.label,
        dir: profile.dir,
        seed: profile.color,
        tile,
    }
}

/// A desktop id as listed in `mimeapps.list`, with what the page shows for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

/// What one scheme opens with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Named {
    Signpost,
    App(Identity),
    Unset,
}

impl Named {
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Signpost => fl!("app-name"),
            Self::App(app) => app.name.clone(),
            Self::Unset => fl!("handler-none"),
        }
    }
}

/// Who opens web links: one app for both schemes, or a split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerView {
    Signpost,
    App(Identity),
    Split { http: Named, https: Named },
    None,
}

impl HandlerView {
    /// What the row names as opening web links: one app, "Mixed" when the schemes differ, or "None".
    #[must_use]
    pub fn value(&self) -> String {
        match self {
            Self::Signpost => fl!("app-name"),
            Self::App(app) => app.name.clone(),
            Self::Split { .. } => fl!("handler-mixed"),
            Self::None => fl!("handler-none"),
        }
    }
}

/// The button of the "Web browser" row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Make Signpost open both HTTP and HTTPS links.
    UseSignpost,
    /// Put back what opened web links before Signpost did.
    Restore,
}

impl Action {
    #[must_use]
    pub fn is_suggested(self) -> bool {
        self == Self::UseSignpost
    }

    #[must_use]
    pub fn op(self) -> Op {
        match self {
            Self::UseSignpost => Op::Set,
            Self::Restore => Op::Restore,
        }
    }

    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::UseSignpost => fl!("use-signpost"),
            Self::Restore => fl!("restore-defaults"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Set,
    Restore,
}

/// A [`SetupError`] the page can keep and compare, in the catalogue's words. Only the failures the page
/// treats differently are told apart; `Verify` is explained from the snapshot instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpError {
    /// A failure its title says all of.
    Failed(String),
    /// Setup was made in another file, which restore can undo.
    PathChanged(String),
    /// The record's own error, with its path, for the details.
    Record(String),
    Verify,
    /// The task that ran the operation died before reporting; the page words it from the catalogue.
    Task,
}

impl From<&SetupError> for OpError {
    fn from(error: &SetupError) -> Self {
        let shown = |path: &std::path::Path| path.display().to_string();
        match error {
            SetupError::Write(path, e) => Self::Failed(fl!(
                "setup-write-failed",
                path = shown(path),
                reason = e.to_string()
            )),
            SetupError::Read(path, e) => Self::Failed(fl!(
                "setup-read-failed",
                path = shown(path),
                reason = e.to_string()
            )),
            SetupError::Record(..) => Self::Record(error.to_string()),
            SetupError::Verify => Self::Verify,
            SetupError::NoRestorePoint => Self::Failed(fl!("setup-nothing-to-restore")),
            SetupError::PathChanged { recorded, current } => Self::PathChanged(fl!(
                "setup-path-changed",
                recorded = shown(recorded),
                current = shown(current)
            )),
            SetupError::Unresolvable(path, e) => Self::Failed(fl!(
                "setup-unresolvable",
                path = shown(path),
                reason = e.to_string()
            )),
            SetupError::LinkChanged {
                path,
                recorded,
                current,
            } => Self::Failed(fl!(
                "setup-link-changed",
                path = shown(path),
                recorded = shown(recorded),
                current = shown(current)
            )),
        }
    }
}

/// The error row under "Web browser" after a failed operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorRow {
    pub title: String,
    pub body: Option<String>,
    pub details: Vec<String>,
    pub offers_restore: bool,
}

/// `undo` is the action its Undo button runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub text: String,
    pub undo: Option<Action>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub handler: HandlerView,
    /// The row's one button: Restore while Signpost opens web links, Use Signpost otherwise.
    pub action: Action,
    /// Whether that button can work now; when not, it stays in its place, disabled.
    pub can_run: bool,
    /// Why the record of the saved defaults cannot be read, while it cannot.
    pub record_error: Option<ErrorRow>,
    pub error_row: Option<ErrorRow>,
    pub toast: Option<Toast>,
}

/// The settings page's facts for `snapshot`. `describe` names a desktop id; `last` is the newest operation
/// and its result, so a later success replaces an earlier failure.
#[must_use]
pub fn model(
    snapshot: &Snapshot,
    describe: &dyn Fn(&str) -> Option<Identity>,
    last: Option<&(Op, Result<(), OpError>)>,
) -> Model {
    let handler = handler_view(
        named(&snapshot.http, describe),
        named(&snapshot.https, describe),
    );
    let action = if handler == HandlerView::Signpost {
        Action::Restore
    } else {
        Action::UseSignpost
    };
    let record_error = match &snapshot.record {
        Record::Unreadable(reason) => Some(record_error(reason)),
        Record::Absent | Record::Valid(_) => None,
    };
    Model {
        // Setup and restore both read the record first, so neither can work until it is repaired; restore needs
        // one to put back.
        can_run: match &snapshot.record {
            Record::Unreadable(_) => false,
            Record::Absent => action == Action::UseSignpost,
            Record::Valid(_) => true,
        },
        action,
        toast: last.and_then(|last| toast(last, &handler)),
        handler,
        error_row: last
            .and_then(|(_, result)| result.as_ref().err())
            // A failure the record stopped is the record's own band while that still shows.
            .filter(|error| !(matches!(error, OpError::Record(_)) && record_error.is_some()))
            .map(|error| error_row(error, snapshot)),
        record_error,
    }
}

fn named(handler: &setup::Handler, describe: &dyn Fn(&str) -> Option<Identity>) -> Named {
    match handler.apps.as_deref().and_then(<[String]>::first) {
        None => Named::Unset,
        Some(id) if id == DESKTOP_FILE => Named::Signpost,
        Some(id) => Named::App(describe(id).unwrap_or_else(|| Identity {
            id: id.to_owned(),
            name: id.strip_suffix(".desktop").unwrap_or(id).to_owned(),
            icon: None,
        })),
    }
}

fn handler_view(http: Named, https: Named) -> HandlerView {
    match (http, https) {
        (Named::Signpost, Named::Signpost) => HandlerView::Signpost,
        (Named::Unset, Named::Unset) => HandlerView::None,
        (Named::App(a), Named::App(b)) if a.id == b.id => HandlerView::App(a),
        (http, https) => HandlerView::Split { http, https },
    }
}

fn error_row(error: &OpError, snapshot: &Snapshot) -> ErrorRow {
    match error {
        OpError::Verify => ErrorRow {
            title: fl!("setup-overridden-title"),
            body: Some(fl!("setup-overridden-body")),
            details: blocking_entries(snapshot),
            offers_restore: matches!(snapshot.record, Record::Valid(_)),
        },
        OpError::Failed(text) => titled(text.clone()),
        OpError::PathChanged(text) => ErrorRow {
            offers_restore: matches!(snapshot.record, Record::Valid(_)),
            ..titled(text.clone())
        },
        OpError::Record(reason) => record_error(reason),
        OpError::Task => titled(fl!("setup-task-failed")),
    }
}

/// The band for a record of saved defaults that cannot be read: what it stops and how to repair it, with
/// `reason`, its path and the parser's error, as the details.
fn record_error(reason: &str) -> ErrorRow {
    ErrorRow {
        title: fl!("record-unreadable-title"),
        body: Some(fl!("record-unreadable-body")),
        details: vec![reason.to_owned()],
        offers_restore: false,
    }
}

fn titled(title: String) -> ErrorRow {
    ErrorRow {
        title,
        body: None,
        details: Vec::new(),
        offers_restore: false,
    }
}

/// The list entries that keep a scheme away from Signpost: `<list>: <mime type>=<apps>`.
fn blocking_entries(snapshot: &Snapshot) -> Vec<String> {
    [
        (setup::HTTP, &snapshot.http),
        (setup::HTTPS, &snapshot.https),
    ]
    .into_iter()
    .filter_map(|(mime, handler)| {
        let (apps, source) = (handler.apps.as_ref()?, handler.source.as_ref()?);
        let blocked = apps.first().is_none_or(|app| app != DESKTOP_FILE);
        blocked.then(|| format!("{}: {mime}={}", source.display(), apps.join(";")))
    })
    .collect()
}

/// After a success: who opens web links now, with Undo after Signpost was made the default.
fn toast(last: &(Op, Result<(), OpError>), handler: &HandlerView) -> Option<Toast> {
    match last {
        (Op::Set, Ok(())) => Some(Toast {
            text: fl!("toast-app-default", name = fl!("app-name")),
            undo: Some(Action::Restore),
        }),
        (Op::Restore, Ok(())) => Some(Toast {
            text: now_open(handler),
            undo: None,
        }),
        (_, Err(_)) => None,
    }
}

/// What opens web links, for a toast: the app, each scheme's, or that nothing does.
fn now_open(handler: &HandlerView) -> String {
    match handler {
        HandlerView::Signpost => fl!("toast-app-default", name = fl!("app-name")),
        HandlerView::App(app) => fl!("toast-app-default", name = app.name.clone()),
        HandlerView::None => fl!("toast-cleared"),
        HandlerView::Split { http, https } => {
            let scheme = |label: &str, named: &Named| match named {
                Named::Unset => (false, fl!("toast-scheme-none", scheme = label)),
                named => (
                    true,
                    fl!("toast-scheme-app", scheme = label, name = named.name()),
                ),
            };
            // A scheme with an app first, HTTPS before HTTP.
            let mut parts = [scheme("HTTPS", https), scheme("HTTP", http)];
            parts.sort_by_key(|(set, _)| !set);
            parts.map(|(_, part)| part).join("; ")
        }
    }
}

mod drawer;
mod pages;
mod view;
mod window;

pub use window::{Link, Message, Operations, Page, Sources, Window, window_settings};

#[cfg(test)]
mod tests;
