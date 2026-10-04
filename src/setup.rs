use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

mod snapshot;

pub use snapshot::{Handler, Record, Snapshot, snapshot};

pub const HTTP: &str = "x-scheme-handler/http";
pub const HTTPS: &str = "x-scheme-handler/https";
pub const DESKTOP_FILE: &str = "com.lkbddh.signpost.desktop";

const DEFAULTS_GROUP: &str = "Default Applications";
const RECORD_KEYS: [&str; 7] = [
    "path",
    "existed",
    "added_defaults_group",
    "http",
    "https",
    "effective_http",
    "effective_https",
];
const TMP_ATTEMPTS: u64 = 16;
/// How many times a list another program keeps saving is read and edited again before Signpost gives up.
const EDIT_ATTEMPTS: usize = 3;
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("could not write {path}: {1}", path = .0.display())]
    Write(PathBuf, std::io::Error),
    #[error("could not read {path}: {1}", path = .0.display())]
    Read(PathBuf, std::io::Error),
    #[error("restore record {path} is unreadable: {1}", path = .0.display())]
    Record(PathBuf, String),
    #[error("the change did not take effect (another mimeapps.list overrides it)")]
    Verify,
    #[error("there is no previous browser to restore")]
    NoRestorePoint,
    #[error(
        "Signpost was already set up in {from}; restore the previous browser before setting it up in {to}",
        from = .recorded.display(),
        to = .current.display()
    )]
    PathChanged { recorded: PathBuf, current: PathBuf },
    #[error("{path} cannot be resolved ({1}); fix or remove that symlink and try again", path = .0.display())]
    Unresolvable(PathBuf, std::io::Error),
    #[error(
        "{path} now leads to {now}, not {then} where Signpost was set up; point it back to restore the previous browser",
        path = .path.display(),
        now = .current.display(),
        then = .recorded.display()
    )]
    LinkChanged {
        path: PathBuf,
        recorded: PathBuf,
        current: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MimeBaseline {
    pub path: PathBuf,
    /// The file `path` led to at setup, through every link in it and its folders. Records written before this
    /// was kept have none.
    #[serde(default)]
    pub resolved: Option<PathBuf>,
    pub existed: bool,
    /// The exact text (separator and header) setup appended to open a `[Default Applications]` group
    /// the file did not have; restore removes it again while it is still the empty last group.
    pub added_defaults_group: Option<String>,
    pub http: Option<Vec<String>>,
    pub https: Option<Vec<String>>,
    pub effective_http: Option<Vec<String>>,
    pub effective_https: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RestoreRecord {
    pub mimeapps: Option<MimeBaseline>,
}

/// True when a baseline exists, or when the record cannot be read (restore then reports the error).
#[must_use]
pub fn has_restore_point(state_dir: &Path) -> bool {
    !matches!(load_record(state_dir), Ok(RestoreRecord { mimeapps: None }))
}

fn record_path(state_dir: &Path) -> PathBuf {
    state_dir.join("signpost").join("restore.json")
}

/// A record must be an object with a `mimeapps` baseline holding every key (a list key may be an
/// explicit `null`). A partial record would otherwise read as "no baseline" or lose the original defaults.
fn parse_record(text: &str) -> Result<RestoreRecord, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let baseline = value
        .get("mimeapps")
        .and_then(serde_json::Value::as_object)
        .ok_or("the `mimeapps` baseline is missing")?;
    if let Some(key) = RECORD_KEYS.iter().find(|k| !baseline.contains_key(**k)) {
        return Err(format!("the baseline is missing `{key}`"));
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn load_record(state_dir: &Path) -> Result<RestoreRecord, SetupError> {
    let path = record_path(state_dir);
    match crate::index::read_regular(&path) {
        Ok(text) => parse_record(&text).map_err(|e| SetupError::Record(path, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RestoreRecord::default()),
        Err(e) => Err(SetupError::Record(path, e.to_string())),
    }
}

fn mime(s: &str) -> mime::Mime {
    s.parse().expect("static scheme-handler MIME types parse")
}

/// `None` when the file does not exist. Only a regular file is read.
fn read_text(path: &Path) -> Result<Option<String>, SetupError> {
    match crate::index::read_regular(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(SetupError::Read(path.to_owned(), e)),
    }
}

fn parse_list(text: &str) -> cosmic_mime_apps::List {
    let mut list = cosmic_mime_apps::List::default();
    list.load_from(text);
    list
}

/// The real file behind `path`: its resolved target, or `path` itself when nothing is there yet. A
/// symlink that cannot be resolved (dangling, a loop, …) is an error, so it is never replaced.
fn resolve(path: &Path) -> Result<PathBuf, SetupError> {
    match std::fs::canonicalize(path) {
        Ok(real) => Ok(real),
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                && path
                    .symlink_metadata()
                    .is_err_and(|m| m.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(path.to_owned())
        }
        Err(e) => Err(SetupError::Unresolvable(path.to_owned(), e)),
    }
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), SetupError> {
    write_atomic_with(path, contents, || Ok(()))
}

fn tmp_path(dir: &Path, file_name: &OsStr, n: u64) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(file_name);
    name.push(format!(".signpost-{}-{n}", std::process::id()));
    dir.join(name)
}

/// `create_new` is `O_CREAT | O_EXCL`: it never follows a symlink and never reuses an existing file.
fn create_exclusive(
    dir: &Path,
    file_name: &OsStr,
    first: u64,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    for n in first..first.saturating_add(TMP_ATTEMPTS) {
        let tmp = tmp_path(dir, file_name, n);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(file) => return Ok((tmp, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no free temporary file name",
    ))
}

/// Unique exclusive temp file in the same directory, synced, then renamed over `path`. An existing file
/// is replaced where it really lives (a symlinked list, e.g. from a dotfile manager, stays a symlink)
/// and keeps its permission bits. `before_rename` exists for failure-injection tests. Only the temp file
/// this call created is removed.
fn write_atomic_with(
    path: &Path,
    contents: &str,
    before_rename: impl FnOnce() -> std::io::Result<()>,
) -> Result<(), SetupError> {
    let err = |e| SetupError::Write(path.to_owned(), e);
    let target = resolve(path)?;
    let permissions = std::fs::metadata(&target).ok().map(|m| m.permissions());
    let dir = target.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(err)?;
    let file_name = target.file_name().unwrap_or(OsStr::new("file"));
    let first = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let (tmp, mut file) = create_exclusive(dir, file_name, first).map_err(err)?;
    let result = permissions
        .map_or(Ok(()), |p| file.set_permissions(p))
        .and_then(|()| file.write_all(contents.as_bytes()))
        .and_then(|()| file.sync_all())
        .and_then(|()| before_rename())
        .and_then(|()| std::fs::rename(&tmp, &target));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(err)
}

/// Replaces `path` with what `edit` makes of its text (`None` while there is no file; an edit of `None` removes
/// it). The file is read again just before it is replaced, and when another program saved it meanwhile its
/// change is kept: `edit` runs again on the new text, a few times at most.
fn edit_file(
    path: &Path,
    edit: impl FnMut(Option<&str>) -> Result<Option<String>, SetupError>,
) -> Result<(), SetupError> {
    edit_file_with(path, edit, || {})
}

/// [`edit_file`], with `meanwhile` run between the read and the check, for tests to save the file there.
fn edit_file_with(
    path: &Path,
    mut edit: impl FnMut(Option<&str>) -> Result<Option<String>, SetupError>,
    mut meanwhile: impl FnMut(),
) -> Result<(), SetupError> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        let read = read_text(path)?;
        let mut unchanged = || {
            meanwhile();
            let now = match crate::index::read_regular(path) {
                Ok(text) => Some(text),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            };
            if now == read {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "another program kept changing it",
                ))
            }
        };
        let result = match edit(read.as_deref())? {
            Some(text) => write_atomic_with(path, &text, unchanged),
            None => unchanged()
                .and_then(|()| match std::fs::remove_file(path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    other => other,
                })
                .map_err(|e| SetupError::Write(path.to_owned(), e)),
        };
        match result {
            Err(SetupError::Write(_, e))
                if e.kind() == std::io::ErrorKind::Interrupted && attempt < EDIT_ATTEMPTS => {}
            result => return result,
        }
    }
}

/// Where `path` leads: every link in it and its folders followed, as far as they exist.
fn destination(path: &Path) -> PathBuf {
    let mut below = Vec::new();
    let mut at = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(at) {
            return below.iter().rev().fold(real, |real, name| real.join(name));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                below.push(name);
                at = parent;
            }
            _ => return path.to_owned(),
        }
    }
}

/// Refuses a list that now leads somewhere other than the file setup changed: the recorded defaults belong to that
/// file, not to whatever a link points to now.
fn same_destination(baseline: &MimeBaseline) -> Result<(), SetupError> {
    let Some(recorded) = &baseline.resolved else {
        return Ok(());
    };
    let current = destination(&baseline.path);
    if current == *recorded {
        Ok(())
    } else {
        Err(SetupError::LinkChanged {
            path: baseline.path.clone(),
            recorded: recorded.clone(),
            current,
        })
    }
}

/// The group name of a `[name]` header line, classified the way `cosmic_mime_apps` reads it.
fn group_name(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix('[')
        .and_then(|rest| rest.rfind(']').map(|end| &rest[..end]))
}

fn has_defaults_group(text: &str) -> bool {
    text.lines().any(|l| group_name(l) == Some(DEFAULTS_GROUP))
}

/// What opens a new `[Default Applications]` group at the end of `text`: the header on its own line,
/// after a blank line.
fn new_group_opening(text: &str) -> String {
    let separator = if text.is_empty() || text.ends_with("\n\n") {
        ""
    } else if text.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{separator}[{DEFAULTS_GROUP}]\n")
}

/// Rewrites only `edits` (MIME key, new list or `None` to remove) inside `[Default Applications]` groups of
/// `text`: every occurrence of each key is removed, then each `Some` value is inserted as the first lines after
/// the first group header (a group is appended when there is none). All other bytes stay as they were.
/// Lines are classified the way `cosmic_mime_apps` reads them, so what it sees for these keys is what is replaced.
fn edit_defaults(text: &str, edits: &[(&str, Option<&[String]>)]) -> String {
    let keys: Vec<mime::Mime> = edits.iter().map(|(key, _)| mime(key)).collect();
    let new_lines: String = edits
        .iter()
        .filter_map(|(key, apps)| {
            let apps = apps.as_ref()?;
            let value: String = apps.iter().flat_map(|a| [a.as_str(), ";"]).collect();
            Some(format!("{key}={value}\n"))
        })
        .collect();
    let mut out = String::with_capacity(text.len() + new_lines.len());
    let (mut in_defaults, mut inserted) = (false, false);
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(group) = group_name(line) {
            in_defaults = group == DEFAULTS_GROUP;
            out.push_str(line);
            if in_defaults && !inserted {
                inserted = true;
                if !line.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&new_lines);
            }
            continue;
        }
        let ours = in_defaults
            && !trimmed.starts_with('#')
            && trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.parse::<mime::Mime>().is_ok_and(|m| keys.contains(&m)));
        if !ours {
            out.push_str(line);
        }
    }
    if !inserted && !new_lines.is_empty() {
        out.push_str(&new_group_opening(&out));
        out.push_str(&new_lines);
    }
    out
}

/// Blank, or nothing but empty `[Default Applications]` headers.
fn nothing_left(text: &str) -> bool {
    text.lines().map(str::trim).all(|l| {
        l.is_empty()
            || l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) == Some(DEFAULTS_GROUP)
    })
}

fn own_entries(list: &cosmic_mime_apps::List, key: &str) -> Option<Vec<String>> {
    list.default_apps
        .get(&mime(key))
        .map(|v| v.iter().map(ToString::to_string).collect())
}

fn effective(all_lists: &[PathBuf], key: &str) -> Option<Vec<String>> {
    own_entries(&crate::index::load_lists(all_lists), key)
}

fn save_record(state_dir: &Path, record: &RestoreRecord) -> Result<(), SetupError> {
    let path = record_path(state_dir);
    if *record == RestoreRecord::default() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(SetupError::Write(path, e)),
            _ => Ok(()),
        };
    }
    write_atomic(
        &path,
        &serde_json::to_string_pretty(record).expect("record serializes"),
    )
}

/// True when both `http` and `https` resolve, across `all_lists` (highest precedence first), to Signpost.
#[must_use]
pub fn is_default_browser(all_lists: &[PathBuf]) -> bool {
    [HTTP, HTTPS].iter().all(|k| {
        effective(all_lists, k)
            .and_then(|v| v.into_iter().next())
            .is_some_and(|a| a == DESKTOP_FILE)
    })
}

/// `all_lists` with `user_list` among them at its precedence, to check what was written: cosmic-mime-apps names only
/// the lists that exist, so one setup has just created goes first, after any list of its own folder that precedes
/// it (a desktop's own list before the plain one).
fn with_user_list(all_lists: &[PathBuf], user_list: &Path) -> Vec<PathBuf> {
    let mut lists = all_lists.to_vec();
    if !lists.iter().any(|list| list == user_list) {
        let folder = user_list.parent();
        let at = lists
            .iter()
            .take_while(|list| list.parent() == folder)
            .count();
        lists.insert(at, user_list.to_owned());
    }
    lists
}

/// Makes Signpost the default `http`/`https` handler in `user_list`, recording the baseline once.
/// Only those two keys of `[Default Applications]` are edited; everything else in the file is kept.
///
/// # Errors
/// Nothing is written on [`SetupError::Record`], if an existing restore record is unreadable, corrupt or
/// incomplete; [`SetupError::PathChanged`], if the baseline was recorded for a different file (restore first);
/// [`SetupError::Unresolvable`], if `user_list` is a symlink that cannot be resolved; or
/// [`SetupError::LinkChanged`], if a link in it or its folders now leads somewhere other than the file the
/// baseline was recorded for. [`SetupError::Read`]/[`SetupError::Write`] on I/O failure or when another program
/// keeps saving the list, and [`SetupError::Verify`] when another list in `all_lists` still overrides the
/// change.
pub fn set_default_browser(
    user_list: &Path,
    all_lists: &[PathBuf],
    state_dir: &Path,
) -> Result<(), SetupError> {
    let mut record = load_record(state_dir)?;
    if let Some(baseline) = &record.mimeapps
        && baseline.path != user_list
    {
        return Err(SetupError::PathChanged {
            recorded: baseline.path.clone(),
            current: user_list.to_owned(),
        });
    }
    // Before anything is written, the record included.
    resolve(user_list)?;
    if let Some(baseline) = &record.mimeapps {
        same_destination(baseline)?;
    }
    let resolved = destination(user_list);
    // A baseline is taken from the text this setup edits, so one taken from a list saved meanwhile is replaced.
    let first = record.mimeapps.is_none();
    let ours = [DESKTOP_FILE.to_owned()];
    edit_file(user_list, |read| {
        let text = read.unwrap_or_default();
        if first {
            let list = parse_list(text);
            record.mimeapps = Some(MimeBaseline {
                path: user_list.to_owned(),
                resolved: Some(resolved.clone()),
                existed: read.is_some(),
                added_defaults_group: (!has_defaults_group(text)).then(|| new_group_opening(text)),
                http: own_entries(&list, HTTP),
                https: own_entries(&list, HTTPS),
                effective_http: effective(all_lists, HTTP),
                effective_https: effective(all_lists, HTTPS),
            });
            save_record(state_dir, &record)?;
        }
        Ok(Some(edit_defaults(
            text,
            &[
                (HTTP, Some(ours.as_slice())),
                (HTTPS, Some(ours.as_slice())),
            ],
        )))
    })?;
    if is_default_browser(&with_user_list(all_lists, user_list)) {
        Ok(())
    } else {
        Err(SetupError::Verify)
    }
}

/// Puts back the recorded `http`/`https` defaults in the recorded file, then verifies and clears the record.
/// A `[Default Applications]` group setup appended is removed while it is still empty and last. The file
/// is deleted only when it was originally absent and nothing but an empty `[Default Applications]`
/// header is left.
///
/// # Errors
/// [`SetupError::NoRestorePoint`] without a record, [`SetupError::Record`] if the record is unreadable, corrupt or
/// incomplete (left untouched), [`SetupError::Unresolvable`] if the recorded file is a symlink that cannot be
/// resolved and [`SetupError::LinkChanged`] if a link in it or its folders now leads somewhere other than the file
/// setup changed (nothing is written in either case), [`SetupError::Read`]/[`SetupError::Write`] on I/O failure or
/// when another program keeps saving the list, and [`SetupError::Verify`] when the file or the effective defaults
/// do not match the baseline afterwards (the record is kept for a retry).
pub fn restore_default_browser(all_lists: &[PathBuf], state_dir: &Path) -> Result<(), SetupError> {
    let mut record = load_record(state_dir)?;
    let baseline = record.mimeapps.clone().ok_or(SetupError::NoRestorePoint)?;
    let path = &baseline.path;
    resolve(path)?;
    same_destination(&baseline)?;
    edit_file(path, |text| {
        let mut restored = edit_defaults(
            text.unwrap_or_default(),
            &[
                (HTTP, baseline.http.as_deref()),
                (HTTPS, baseline.https.as_deref()),
            ],
        );
        // The group setup appended goes again while nothing else was added to it.
        if let Some(opening) = &baseline.added_defaults_group
            && restored.ends_with(opening.as_str())
        {
            restored.truncate(restored.len() - opening.len());
        }
        Ok((baseline.existed || !nothing_left(&restored)).then_some(restored))
    })?;
    let check = parse_list(&read_text(path)?.unwrap_or_default());
    let file_ok =
        own_entries(&check, HTTP) == baseline.http && own_entries(&check, HTTPS) == baseline.https;
    let effective_ok = effective(all_lists, HTTP) == baseline.effective_http
        && effective(all_lists, HTTPS) == baseline.effective_https;
    if !(file_ok && effective_ok) {
        return Err(SetupError::Verify);
    }
    record.mimeapps = None;
    save_record(state_dir, &record)
}

#[cfg(test)]
mod tests;
