use std::path::{Path, PathBuf};
use std::slice::from_ref;

use super::{HTTP, HTTPS, MimeBaseline, RestoreRecord, load_record, own_entries};

/// What a scheme resolves to across the lists, and the list that supplies it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Handler {
    /// The entries of the effective `[Default Applications]` line, best first.
    pub apps: Option<Vec<String>>,
    /// The list whose own entry is the effective one.
    pub source: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Absent,
    Valid(MimeBaseline),
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub http: Handler,
    pub https: Handler,
    pub record: Record,
}

/// Loading keeps the first entry it meets, so the first list with an own entry for `key` is the effective one.
/// Each list is loaded alone, so an unreadable one, or one that is not a regular file, is skipped.
fn handler(all_lists: &[PathBuf], key: &str) -> Handler {
    all_lists
        .iter()
        .find_map(|path| {
            own_entries(&crate::index::load_lists(from_ref(path)), key).map(|apps| Handler {
                apps: Some(apps),
                source: Some(path.clone()),
            })
        })
        .unwrap_or_default()
}

/// Reads the effective `http`/`https` handlers across `all_lists` (highest precedence first) and the
/// restore record. Writes nothing.
#[must_use]
pub fn snapshot(all_lists: &[PathBuf], state_dir: &Path) -> Snapshot {
    let record = match load_record(state_dir) {
        Ok(RestoreRecord { mimeapps: None }) => Record::Absent,
        Ok(RestoreRecord {
            mimeapps: Some(baseline),
        }) => Record::Valid(baseline),
        Err(e) => Record::Unreadable(e.to_string()),
    };
    Snapshot {
        http: handler(all_lists, HTTP),
        https: handler(all_lists, HTTPS),
        record,
    }
}

#[cfg(test)]
mod tests {
    use std::slice::from_ref;

    use super::*;
    use crate::setup::{
        DESKTOP_FILE, HTTP, HTTPS, effective, load_record, own_entries, parse_list,
        set_default_browser,
    };

    const DEFAULTS: &str = "[Default Applications]\n";

    struct Fixture {
        tmp: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                tmp: tempfile::tempdir().unwrap(),
            }
        }

        /// `Some(text)` writes the list; `None` names a list that does not exist.
        fn list(&self, name: &str, text: Option<&str>) -> PathBuf {
            let path = self.tmp.path().join(name);
            if let Some(text) = text {
                std::fs::write(&path, text).unwrap();
            }
            path
        }

        fn state(&self) -> PathBuf {
            self.tmp.path().join("state")
        }

        fn record_file(&self, text: &str) {
            let path = self.state().join("signpost/restore.json");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    fn apps(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|e| (*e).to_owned()).collect()
    }

    #[test]
    fn an_inherited_system_default_names_the_system_list_as_its_source() {
        let f = Fixture::new();
        let user = f.list(
            "user.list",
            Some("[Default Applications]\ntext/html=a.desktop\n"),
        );
        let system = f.list(
            "system.list",
            Some("[Default Applications]\nx-scheme-handler/https=firefox.desktop\n"),
        );
        let s = snapshot(&[user, system.clone()], &f.state());
        assert_eq!(
            s.https,
            Handler {
                apps: Some(apps(&["firefox.desktop"])),
                source: Some(system),
            }
        );
        assert_eq!(s.http, Handler::default());
    }

    #[test]
    fn no_lists_empty_lists_and_missing_lists_resolve_to_nothing() {
        let f = Fixture::new();
        let empty = f.list("empty.list", Some(""));
        let missing = f.list("missing.list", None);
        for lists in [vec![], vec![empty.clone()], vec![missing.clone(), empty]] {
            let s = snapshot(&lists, &f.state());
            assert_eq!((s.http, s.https), (Handler::default(), Handler::default()));
        }
    }

    #[test]
    fn each_scheme_comes_from_the_highest_list_that_has_it() {
        let f = Fixture::new();
        let high = f.list(
            "high.list",
            Some("[Default Applications]\nx-scheme-handler/https=chrome.desktop;brave.desktop;\n"),
        );
        let low = f.list(
            "low.list",
            Some(
                "[Default Applications]\nx-scheme-handler/http=firefox.desktop\nx-scheme-handler/https=firefox.desktop\n",
            ),
        );
        let s = snapshot(&[high.clone(), low.clone()], &f.state());
        assert_eq!(
            s.https,
            Handler {
                apps: Some(apps(&["chrome.desktop", "brave.desktop"])),
                source: Some(high),
            }
        );
        assert_eq!(
            s.http,
            Handler {
                apps: Some(apps(&["firefox.desktop"])),
                source: Some(low),
            }
        );
    }

    #[test]
    fn apps_equal_the_effective_entries_and_the_source_supplies_them() {
        let f = Fixture::new();
        let cases: [&[Option<&str>]; 9] = [
            &[],
            &[None],
            &[Some("")],
            &[Some(
                "[Default Applications]\nx-scheme-handler/https=a.desktop\n",
            )],
            &[
                Some("[Default Applications]\ntext/html=a.desktop\n"),
                Some("[Default Applications]\nx-scheme-handler/https=b.desktop\n"),
            ],
            &[
                Some("[Default Applications]\nx-scheme-handler/http=\n"),
                Some("[Default Applications]\nx-scheme-handler/http=b.desktop\n"),
            ],
            &[
                Some("[Added Associations]\nx-scheme-handler/https=a.desktop\n"),
                Some("[Default Applications]\nx-scheme-handler/https=b.desktop\n"),
            ],
            &[Some(
                "[Default Applications]\nx-scheme-handler/http=a.desktop\nx-scheme-handler/http=b.desktop\n",
            )],
            &[
                None,
                Some(
                    "[Other]\nx-scheme-handler/https=a.desktop\n\n[Default Applications]\nx-scheme-handler/https=b.desktop;c.desktop\n",
                ),
                Some("[Default Applications]\nx-scheme-handler/https=d.desktop\n"),
            ],
        ];
        for (case, texts) in cases.iter().enumerate() {
            let lists: Vec<PathBuf> = texts
                .iter()
                .enumerate()
                .map(|(i, text)| f.list(&format!("{case}-{i}.list"), *text))
                .collect();
            let s = snapshot(&lists, &f.state());
            for (key, handler) in [(HTTP, &s.http), (HTTPS, &s.https)] {
                assert_eq!(handler.apps, effective(&lists, key), "case {case} {key}");
                let Some(source) = &handler.source else {
                    assert_eq!(handler.apps, None, "case {case} {key}: no source, no apps");
                    continue;
                };
                let own = own_entries(&parse_list(&std::fs::read_to_string(source).unwrap()), key);
                assert_eq!(
                    own, handler.apps,
                    "case {case} {key}: the source's own entry"
                );
                let position = |p: &PathBuf| lists.iter().position(|l| l == p).unwrap();
                for earlier in &lists[..position(source)] {
                    let text = std::fs::read_to_string(earlier).unwrap_or_default();
                    assert_eq!(
                        own_entries(&parse_list(&text), key),
                        None,
                        "case {case} {key}: {earlier:?} should have won"
                    );
                }
            }
        }
    }

    #[test]
    fn an_absent_record_is_absent_and_nothing_is_written() {
        let f = Fixture::new();
        let list = f.list("user.list", Some(DEFAULTS));
        let s = snapshot(from_ref(&list), &f.state());
        assert_eq!(s.record, Record::Absent);
        assert!(!f.state().exists(), "snapshot created the state directory");
        assert_eq!(std::fs::read_to_string(list).unwrap(), DEFAULTS);
    }

    #[test]
    fn a_valid_record_carries_the_recorded_baseline() {
        let f = Fixture::new();
        let list = f.list(
            "user.list",
            Some("[Default Applications]\nx-scheme-handler/https=firefox.desktop\n"),
        );
        set_default_browser(&list, from_ref(&list), &f.state()).unwrap();
        let baseline = load_record(&f.state()).unwrap().mimeapps.unwrap();
        let s = snapshot(from_ref(&list), &f.state());
        assert_eq!(s.record, Record::Valid(baseline));
        assert_eq!(s.http.apps, Some(apps(&[DESKTOP_FILE])));
        assert_eq!(s.https.apps, Some(apps(&[DESKTOP_FILE])));
        assert_eq!(s.https.source, Some(list));
    }

    #[test]
    fn a_record_load_record_rejects_is_unreadable_with_its_reason() {
        let f = Fixture::new();
        let list = f.list("user.list", Some(DEFAULTS));
        f.record_file("{ not json");
        let Record::Unreadable(reason) = snapshot(from_ref(&list), &f.state()).record else {
            panic!("a corrupt record must be unreadable");
        };
        assert!(reason.contains("restore.json"), "{reason}");

        f.record_file(
            r#"{"mimeapps":{"path":"p","existed":true,"added_defaults_group":null,"http":null,"effective_http":null,"effective_https":null}}"#,
        );
        let Record::Unreadable(reason) = snapshot(from_ref(&list), &f.state()).record else {
            panic!("a record without a key must be unreadable");
        };
        assert!(reason.contains("`https`"), "{reason}");
    }
}
