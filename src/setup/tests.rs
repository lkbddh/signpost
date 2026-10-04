use super::*;
use std::ffi::OsStr;
use std::io::Write;
use std::slice::from_ref;

struct Dirs {
    tmp: tempfile::TempDir,
    list: PathBuf,
    state: PathBuf,
}

fn dirs(initial: Option<&str>) -> Dirs {
    let tmp = tempfile::tempdir().unwrap();
    let list = tmp.path().join("config/mimeapps.list");
    if let Some(text) = initial {
        std::fs::create_dir_all(list.parent().unwrap()).unwrap();
        std::fs::write(&list, text).unwrap();
    }
    let state = tmp.path().join("state");
    Dirs { tmp, list, state }
}

#[test]
fn a_pipe_where_the_record_or_the_list_should_be_is_an_error_not_a_hang() {
    let d = dirs(None);
    let record = record_path(&d.state);
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::create_dir_all(d.list.parent().unwrap()).unwrap();
    crate::test_support::pipe_at(&record);
    crate::test_support::pipe_at(&d.list);
    let (state, list) = (d.state.clone(), d.list.clone());
    let failed = crate::test_support::finishes(move || {
        (load_record(&state).is_err(), read_text(&list).is_err())
    });
    assert_eq!(failed, Some((true, true)));
}
#[test]
fn set_from_absent_then_restore_removes_the_file() {
    let d = dirs(None);
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    assert!(is_default_browser(from_ref(&d.list)));
    assert!(has_restore_point(&d.state));
    restore_default_browser(from_ref(&d.list), &d.state).unwrap();
    assert!(!d.list.exists());
    assert!(!has_restore_point(&d.state));
}

#[test]
fn set_creates_the_users_first_list_though_the_lists_name_only_existing_files() {
    let d = dirs(None);
    let system = d.tmp.path().join("xdg/mimeapps.list");
    std::fs::create_dir_all(system.parent().unwrap()).unwrap();
    std::fs::write(
        &system,
        "[Default Applications]\nx-scheme-handler/http=firefox.desktop\nx-scheme-handler/https=firefox.desktop\n",
    )
    .unwrap();
    // As cosmic-mime-apps lists them: only the files that exist, so not the user's yet.
    set_default_browser(&d.list, from_ref(&system), &d.state).unwrap();
    assert!(d.list.exists());
    let after = [d.list.clone(), system];
    assert!(is_default_browser(&after));
    restore_default_browser(&after, &d.state).unwrap();
    assert!(!d.list.exists());
}

#[test]
fn restore_puts_back_previous_defaults_and_keeps_other_entries() {
    let d = dirs(Some(
        "[Default Applications]\nx-scheme-handler/https=firefox.desktop\ntext/html=firefox.desktop\n",
    ));
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    let text = std::fs::read_to_string(&d.list).unwrap();
    assert!(
        text.contains("x-scheme-handler/https=com.lkbddh.signpost.desktop"),
        "{text}"
    );
    assert!(
        text.contains("x-scheme-handler/http=com.lkbddh.signpost.desktop"),
        "{text}"
    );
    assert!(
        text.contains("text/html=firefox.desktop"),
        "never touch text/html: {text}"
    );
    restore_default_browser(from_ref(&d.list), &d.state).unwrap();
    let text = std::fs::read_to_string(&d.list).unwrap();
    assert!(
        text.contains("x-scheme-handler/https=firefox.desktop"),
        "{text}"
    );
    assert!(
        !text.contains("x-scheme-handler/http="),
        "http had no default before: {text}"
    );
    assert!(text.contains("text/html=firefox.desktop"), "{text}");
}

#[test]
fn repeated_setup_keeps_the_original_baseline() {
    let d = dirs(Some(
        "[Default Applications]\nx-scheme-handler/https=firefox.desktop\n",
    ));
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    restore_default_browser(from_ref(&d.list), &d.state).unwrap();
    assert!(
        std::fs::read_to_string(&d.list)
            .unwrap()
            .contains("x-scheme-handler/https=firefox.desktop")
    );
}

#[test]
fn restore_writes_the_recorded_path_not_another_list() {
    let d = dirs(Some(
        "[Default Applications]\nx-scheme-handler/https=firefox.desktop\n",
    ));
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    let other = d.tmp.path().join("other-config/mimeapps.list");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::write(&other, "[Default Applications]\nimage/png=viewer.desktop\n").unwrap();
    restore_default_browser(&[other.clone(), d.list.clone()], &d.state).unwrap();
    assert_eq!(
        std::fs::read_to_string(&other).unwrap(),
        "[Default Applications]\nimage/png=viewer.desktop\n"
    );
    assert!(
        std::fs::read_to_string(&d.list)
            .unwrap()
            .contains("x-scheme-handler/https=firefox.desktop")
    );
}

#[test]
fn overriding_higher_precedence_list_fails_setup_verification() {
    let d = dirs(None);
    let high = d.tmp.path().join("config/cosmic-mimeapps.list");
    std::fs::create_dir_all(high.parent().unwrap()).unwrap();
    std::fs::write(
        &high,
        "[Default Applications]\nx-scheme-handler/https=google-chrome.desktop\n",
    )
    .unwrap();
    let err = set_default_browser(&d.list, &[high.clone(), d.list.clone()], &d.state).unwrap_err();
    assert!(matches!(err, SetupError::Verify));
    assert!(!is_default_browser(&[high, d.list.clone()]));
}

#[test]
fn restore_keeps_the_record_when_signpost_is_still_effective() {
    let d = dirs(None);
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    let high = d.tmp.path().join("config/cosmic-mimeapps.list");
    std::fs::write(
        &high,
        "[Default Applications]\nx-scheme-handler/https=com.lkbddh.signpost.desktop\n",
    )
    .unwrap();
    let err = restore_default_browser(&[high, d.list.clone()], &d.state).unwrap_err();
    assert!(matches!(err, SetupError::Verify));
    assert!(has_restore_point(&d.state), "record kept for a later retry");
}

#[test]
fn corrupt_record_is_an_error_and_is_preserved() {
    let d = dirs(None);
    let record = d.state.join("signpost/restore.json");
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(&record, "{ not json").unwrap();
    assert!(matches!(
        set_default_browser(&d.list, from_ref(&d.list), &d.state),
        Err(SetupError::Record(..))
    ));
    assert!(matches!(
        restore_default_browser(from_ref(&d.list), &d.state),
        Err(SetupError::Record(..))
    ));
    assert_eq!(std::fs::read_to_string(&record).unwrap(), "{ not json");
    assert!(!d.list.exists(), "nothing written");
}

#[test]
fn restore_without_record_is_an_error() {
    let d = dirs(None);
    assert!(matches!(
        restore_default_browser(from_ref(&d.list), &d.state),
        Err(SetupError::NoRestorePoint)
    ));
}

#[test]
fn failed_write_leaves_the_target_byte_identical_and_no_temp_file() {
    let d = dirs(Some("old contents\n"));
    let err = write_atomic_with(&d.list, "new contents\n", || {
        Err(std::io::Error::other("injected failure before rename"))
    });
    assert!(matches!(err, Err(SetupError::Write(..))));
    assert_eq!(std::fs::read_to_string(&d.list).unwrap(), "old contents\n");
    let names: Vec<_> = std::fs::read_dir(d.list.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("mimeapps.list")]);
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn one(app: &str) -> Vec<String> {
    vec![app.to_owned()]
}

#[test]
fn a_symlink_at_the_old_fixed_temp_name_never_reaches_the_target() {
    let d = dirs(Some("old contents\n"));
    let old_tmp = d.list.parent().unwrap().join(".mimeapps.list.signpost-tmp");
    std::os::unix::fs::symlink(&d.list, &old_tmp).unwrap();
    let err = write_atomic_with(&d.list, "new contents\n", || {
        Err(std::io::Error::other("injected failure before rename"))
    });
    assert!(matches!(err, Err(SetupError::Write(..))));
    assert_eq!(read(&d.list), "old contents\n");
    assert!(old_tmp.symlink_metadata().unwrap().is_symlink());
    write_atomic(&d.list, "new contents\n").unwrap();
    assert_eq!(read(&d.list), "new contents\n");
    assert!(!d.list.symlink_metadata().unwrap().is_symlink());
    assert!(old_tmp.symlink_metadata().unwrap().is_symlink());
}

#[test]
fn exclusive_temp_creation_skips_taken_names_and_never_writes_through_them() {
    let tmp = tempfile::tempdir().unwrap();
    let name = OsStr::new("mimeapps.list");
    let victim = tmp.path().join("victim");
    std::fs::write(&victim, "precious").unwrap();
    std::fs::write(tmp_path(tmp.path(), name, 5), "precious").unwrap();
    std::os::unix::fs::symlink(&victim, tmp_path(tmp.path(), name, 6)).unwrap();
    let (path, mut file) = create_exclusive(tmp.path(), name, 5).unwrap();
    assert_eq!(path, tmp_path(tmp.path(), name, 7));
    file.write_all(b"fresh").unwrap();
    assert_eq!(read(&tmp_path(tmp.path(), name, 5)), "precious");
    assert_eq!(read(&victim), "precious");
}

#[test]
fn exclusive_temp_creation_gives_up_after_bounded_retries() {
    let tmp = tempfile::tempdir().unwrap();
    let name = OsStr::new("mimeapps.list");
    for n in 0..16 {
        std::fs::write(tmp_path(tmp.path(), name, n), "precious").unwrap();
    }
    let err = create_exclusive(tmp.path(), name, 0).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert!(!tmp_path(tmp.path(), name, 16).exists());
    assert_eq!(read(&tmp_path(tmp.path(), name, 0)), "precious");
}

#[test]
fn a_list_linked_into_a_folder_setup_cannot_write_is_left_alone_and_set_once_it_can() {
    use std::os::unix::fs::PermissionsExt as _;
    let d = dirs(None);
    let folder = d.tmp.path().join("dotfiles");
    let target = folder.join("mimeapps.list");
    // Its defaults in the form setup writes them back in, so the restore can match byte for byte.
    let original = "[Added Associations]\ntext/html=firefox.desktop;\n\n\
                    [Default Applications]\nx-scheme-handler/https=firefox.desktop;\n";
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(&target, original).unwrap();
    std::fs::create_dir_all(d.list.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &d.list).unwrap();
    let lock = |mode| std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode));
    // As a Flatpak sees a folder outside its grants: there, but read-only.
    lock(0o555).unwrap();
    if std::fs::write(folder.join("probe"), "").is_ok() {
        // A process that writes past permissions, such as root's, cannot show this.
        return;
    }

    let refused = set_default_browser(&d.list, from_ref(&d.list), &d.state);

    assert!(matches!(refused, Err(SetupError::Write(..))), "{refused:?}");
    assert_eq!(read(&target), original, "the list is untouched");
    assert!(d.list.symlink_metadata().unwrap().is_symlink());
    assert!(
        has_restore_point(&d.state),
        "the record stays for the retry"
    );

    lock(0o755).unwrap();
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    assert!(is_default_browser(from_ref(&d.list)));
    restore_default_browser(from_ref(&d.list), &d.state).unwrap();
    assert_eq!(read(&target), original, "restored byte for byte");
    assert!(d.list.symlink_metadata().unwrap().is_symlink());
    assert!(!has_restore_point(&d.state));
}

#[test]
fn a_symlinked_list_stays_a_symlink_and_its_target_is_edited() {
    let d = dirs(None);
    let target = d.tmp.path().join("dotfiles/mimeapps.list");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(
        &target,
        "[Default Applications]\nx-scheme-handler/https=firefox.desktop\n",
    )
    .unwrap();
    std::fs::create_dir_all(d.list.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &d.list).unwrap();
    let is_link = || d.list.symlink_metadata().unwrap().is_symlink();
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    assert!(is_link(), "setup replaced the symlink");
    assert!(read(&target).contains("x-scheme-handler/https=com.lkbddh.signpost.desktop"));
    assert!(is_default_browser(from_ref(&d.list)));
    restore_default_browser(from_ref(&d.list), &d.state).unwrap();
    assert!(is_link(), "restore replaced the symlink");
    let restored = read(&target);
    assert!(
        restored.contains("x-scheme-handler/https=firefox.desktop")
            && !restored.contains("Signpost"),
        "{restored}"
    );
    for dir in [target.parent().unwrap(), d.list.parent().unwrap()] {
        assert_eq!(
            std::fs::read_dir(dir).unwrap().count(),
            1,
            "temp file left in {dir:?}"
        );
    }
}

fn https_to_ours(text: Option<&str>) -> String {
    let ours = [DESKTOP_FILE.to_owned()];
    edit_defaults(text.unwrap_or_default(), &[(HTTPS, Some(ours.as_slice()))])
}

#[test]
fn an_entry_another_program_saves_meanwhile_is_kept_and_the_edit_made_again_on_top() {
    let d = dirs(Some(
        "[Default Applications]\nx-scheme-handler/https=google-chrome.desktop\n",
    ));
    let mut saves = 0;
    edit_file_with(
        &d.list,
        |text| Ok(Some(https_to_ours(text))),
        || {
            if saves == 0 {
                let mut list = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&d.list)
                    .unwrap();
                list.write_all(b"text/html=editor.desktop\n").unwrap();
            }
            saves += 1;
        },
    )
    .unwrap();
    let text = read(&d.list);
    assert!(text.contains("text/html=editor.desktop"), "{text}");
    assert!(
        text.contains("x-scheme-handler/https=com.lkbddh.signpost.desktop"),
        "{text}"
    );
    assert_eq!(saves, 2, "read again once");
}

#[test]
fn a_list_that_keeps_changing_is_left_as_the_other_program_saved_it() {
    let d = dirs(Some("[Default Applications]\n"));
    let mut n = 0;
    let result = edit_file_with(
        &d.list,
        |text| Ok(Some(https_to_ours(text))),
        || {
            n += 1;
            std::fs::write(
                &d.list,
                format!("[Default Applications]\nx-{n}=a.desktop\n"),
            )
            .unwrap();
        },
    );
    assert!(
        matches!(&result, Err(SetupError::Write(_, e)) if e.kind() == std::io::ErrorKind::Interrupted),
        "{result:?}"
    );
    assert_eq!(
        read(&d.list),
        format!("[Default Applications]\nx-{n}=a.desktop\n")
    );
    assert_eq!(
        std::fs::read_dir(d.list.parent().unwrap()).unwrap().count(),
        1,
        "a temp file was left"
    );
}

/// `d.list` as a symlink to `dotfiles/<name>`, which holds `text`.
fn link_list(d: &Dirs, name: &str, text: &str) -> PathBuf {
    let target = d.tmp.path().join("dotfiles").join(name);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, text).unwrap();
    std::fs::create_dir_all(d.list.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&d.list);
    std::os::unix::fs::symlink(&target, &d.list).unwrap();
    target
}

#[test]
fn a_symlinked_list_pointed_elsewhere_since_setup_is_refused_until_it_points_back() {
    let d = dirs(None);
    let firefox = "[Default Applications]\nx-scheme-handler/https=firefox.desktop\n";
    let first = link_list(&d, "first.list", firefox);
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    link_list(
        &d,
        "second.list",
        "[Default Applications]\nx-scheme-handler/https=brave.desktop\n",
    );
    let before = snapshot(d.tmp.path());
    for result in [
        restore_default_browser(from_ref(&d.list), &d.state),
        set_default_browser(&d.list, from_ref(&d.list), &d.state),
    ] {
        assert!(
            matches!(&result, Err(SetupError::LinkChanged { recorded, current, .. })
                if *recorded == first.canonicalize().unwrap() && current.ends_with("second.list")),
            "{result:?}"
        );
    }
    assert_eq!(snapshot(d.tmp.path()), before, "something was written");

    std::fs::remove_file(&d.list).unwrap();
    std::os::unix::fs::symlink(&first, &d.list).unwrap();
    restore_default_browser(from_ref(&d.list), &d.state).unwrap();
    let restored = read(&first);
    assert!(
        restored.contains("x-scheme-handler/https=firefox.desktop")
            && !restored.contains(DESKTOP_FILE),
        "{restored}"
    );
}

/// A dotfile manager can link the whole config folder; pointed elsewhere since setup, it holds another list, which
/// restore leaves alone, as it does a plain file put in place of a linked list.
#[test]
fn a_list_whose_folder_now_leads_elsewhere_is_refused_and_nothing_is_written() {
    let d = dirs(None);
    let firefox = "[Default Applications]\nx-scheme-handler/https=firefox.desktop\n";
    let (first, second) = (d.tmp.path().join("first"), d.tmp.path().join("second"));
    for (folder, text) in [(&first, firefox), (&second, "[Default Applications]\n")] {
        std::fs::create_dir_all(folder).unwrap();
        std::fs::write(folder.join("mimeapps.list"), text).unwrap();
    }
    std::os::unix::fs::symlink(&first, d.list.parent().unwrap()).unwrap();
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    std::fs::remove_file(d.list.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&second, d.list.parent().unwrap()).unwrap();
    let before = snapshot(d.tmp.path());
    let refused = restore_default_browser(from_ref(&d.list), &d.state);
    assert!(
        matches!(&refused, Err(SetupError::LinkChanged { current, .. }) if current.starts_with(second.canonicalize().unwrap())),
        "{refused:?}"
    );
    assert_eq!(snapshot(d.tmp.path()), before, "something was written");

    let linked = dirs(None);
    let target = link_list(&linked, "first.list", firefox);
    set_default_browser(&linked.list, from_ref(&linked.list), &linked.state).unwrap();
    std::fs::remove_file(&linked.list).unwrap();
    std::fs::copy(&target, &linked.list).unwrap();
    assert!(matches!(
        restore_default_browser(from_ref(&linked.list), &linked.state),
        Err(SetupError::LinkChanged { .. })
    ));
}

/// Every file under `dir` (symlinks not followed) with its contents or link target.
fn snapshot(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let meta = path.symlink_metadata().unwrap();
        if meta.is_symlink() {
            let target = std::fs::read_link(&path).unwrap();
            out.push((path, format!("-> {}", target.display())));
        } else if meta.is_dir() {
            out.extend(snapshot(&path));
        } else {
            out.push((path.clone(), read(&path)));
        }
    }
    out.sort();
    out
}

#[test]
fn an_unresolvable_symlinked_list_is_refused_before_anything_is_written() {
    let d = dirs(None);
    std::fs::create_dir_all(d.list.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(d.tmp.path().join("gone/mimeapps.list"), &d.list).unwrap();
    let before = snapshot(d.tmp.path());
    assert!(matches!(
        set_default_browser(&d.list, from_ref(&d.list), &d.state),
        Err(SetupError::Unresolvable(..))
    ));
    assert_eq!(snapshot(d.tmp.path()), before, "setup wrote something");

    // Set up for real, then the list becomes a symlink loop: restore refuses and keeps the record.
    let d = dirs(Some(
        "[Default Applications]\nx-scheme-handler/https=firefox.desktop\n",
    ));
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    std::fs::remove_file(&d.list).unwrap();
    std::os::unix::fs::symlink(&d.list, &d.list).unwrap();
    let before = snapshot(d.tmp.path());
    assert!(matches!(
        restore_default_browser(from_ref(&d.list), &d.state),
        Err(SetupError::Unresolvable(..))
    ));
    assert_eq!(snapshot(d.tmp.path()), before, "restore wrote something");
    assert!(has_restore_point(&d.state));
}

#[test]
fn setup_and_restore_keep_the_lists_permission_bits() {
    use std::os::unix::fs::PermissionsExt;
    let d = dirs(Some(
        "[Default Applications]\nx-scheme-handler/https=firefox.desktop\n",
    ));
    std::fs::set_permissions(&d.list, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mode = || std::fs::metadata(&d.list).unwrap().permissions().mode() & 0o7777;
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    assert_eq!(mode(), 0o600, "after setup");
    restore_default_browser(from_ref(&d.list), &d.state).unwrap();
    assert_eq!(mode(), 0o600, "after restore");
}

#[test]
fn edit_defaults_removes_every_occurrence_and_inserts_after_the_first_header() {
    let text = "# c\n[Default Applications]\ntext/html=a.desktop\nx-scheme-handler/http=old.desktop\n\n[Other]\nx-scheme-handler/http=keep-me\n\n[Default Applications]\nx-scheme-handler/http=dup.desktop;\nx-scheme-handler/https=dup2.desktop\nimage/png=v.desktop\n";
    let out = edit_defaults(text, &[(HTTP, Some(&one("new.desktop"))), (HTTPS, None)]);
    assert_eq!(
        out,
        "# c\n[Default Applications]\nx-scheme-handler/http=new.desktop;\ntext/html=a.desktop\n\n[Other]\nx-scheme-handler/http=keep-me\n\n[Default Applications]\nimage/png=v.desktop\n"
    );
}

#[test]
fn edit_defaults_handles_a_bare_header_a_missing_group_and_nothing_to_remove() {
    let x = one("x.desktop");
    let set = [(HTTP, Some(x.as_slice()))];
    assert_eq!(
        edit_defaults("[Default Applications]", &set),
        "[Default Applications]\nx-scheme-handler/http=x.desktop;\n"
    );
    assert_eq!(
        edit_defaults("# note", &set),
        "# note\n\n[Default Applications]\nx-scheme-handler/http=x.desktop;\n"
    );
    assert_eq!(
        edit_defaults("", &set),
        "[Default Applications]\nx-scheme-handler/http=x.desktop;\n"
    );
    assert_eq!(edit_defaults("# note\n", &[(HTTP, None)]), "# note\n");
}

#[test]
fn setup_and_restore_preserve_comments_groups_and_unrelated_lines_byte_for_byte() {
    let original = "# my browsers\n[Added Associations]\nfoo/bar=baz.desktop;\n\n[Default Applications]\n# keep me\ntext/html=firefox.desktop\n\n[Custom Group]\nweird line without equals\nKey=Value\n";
    let d = dirs(Some(original));
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    let during = read(&d.list);
    for kept in [
        "# my browsers\n",
        "foo/bar=baz.desktop;\n",
        "# keep me\n",
        "text/html=firefox.desktop\n",
        "weird line without equals\n",
        "Key=Value\n",
    ] {
        assert!(during.contains(kept), "lost {kept:?}: {during}");
    }
    restore_default_browser(from_ref(&d.list), &d.state).unwrap();
    assert_eq!(read(&d.list), original);
}

#[test]
fn restore_removes_the_defaults_group_setup_appended() {
    for original in [
        "# note",
        "# note\n",
        "# note\n\n",
        "[Added Associations]\nfoo/bar=baz.desktop;\n",
    ] {
        let d = dirs(Some(original));
        set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
        restore_default_browser(from_ref(&d.list), &d.state).unwrap();
        assert_eq!(read(&d.list), original);
    }
}

#[test]
fn restore_keeps_an_appended_group_that_gained_other_keys() {
    let d = dirs(Some("# note\n"));
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    let mut text = read(&d.list);
    text.push_str("text/html=firefox.desktop\n");
    std::fs::write(&d.list, &text).unwrap();
    restore_default_browser(from_ref(&d.list), &d.state).unwrap();
    assert_eq!(
        read(&d.list),
        "# note\n\n[Default Applications]\ntext/html=firefox.desktop\n"
    );
}

#[test]
fn restore_keeps_an_originally_absent_file_that_gained_other_content() {
    for extra in ["# my note\n", "\n[Other]\nk=v\n"] {
        let d = dirs(None);
        set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
        let mut text = read(&d.list);
        text.push_str(extra);
        std::fs::write(&d.list, &text).unwrap();
        restore_default_browser(from_ref(&d.list), &d.state).unwrap();
        assert!(d.list.exists(), "{extra:?}: the file was deleted");
        let after = read(&d.list);
        assert!(
            after.contains(extra.trim()),
            "{extra:?} was deleted: {after}"
        );
        assert!(!after.contains("x-scheme-handler/http"), "{after}");
        assert!(!has_restore_point(&d.state));
    }
}

#[test]
fn incomplete_or_mistyped_records_are_errors_and_are_preserved() {
    let base = |mutate: fn(&mut serde_json::Value)| {
        let mut v = serde_json::json!({"mimeapps": {
            "path": "/nowhere/mimeapps.list", "existed": true, "added_defaults_group": null,
            "http": null, "https": ["firefox.desktop"],
            "effective_http": null, "effective_https": ["firefox.desktop"]
        }});
        mutate(&mut v);
        v.to_string()
    };
    let cases = [
        "{}".to_owned(),
        r#"{"mimeapps":null}"#.to_owned(),
        "[]".to_owned(),
        base(|v| {
            v["mimeapps"].as_object_mut().unwrap().remove("https");
        }),
        base(|v| v["mimeapps"]["existed"] = serde_json::json!("yes")),
        base(|v| {
            v["mimeapps"]
                .as_object_mut()
                .unwrap()
                .remove("added_defaults_group");
        }),
    ];
    for case in cases {
        let d = dirs(None);
        let record = d.state.join("signpost/restore.json");
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(&record, &case).unwrap();
        assert!(
            matches!(
                set_default_browser(&d.list, from_ref(&d.list), &d.state),
                Err(SetupError::Record(..))
            ),
            "set accepted {case}"
        );
        assert!(
            matches!(
                restore_default_browser(from_ref(&d.list), &d.state),
                Err(SetupError::Record(..))
            ),
            "restore accepted {case}"
        );
        assert_eq!(read(&record), case);
        assert!(has_restore_point(&d.state));
        assert!(!d.list.exists(), "nothing written for {case}");
        let names: Vec<_> = std::fs::read_dir(record.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("restore.json")]);
    }
}

#[test]
fn a_complete_record_with_explicit_nulls_loads() {
    let d = dirs(None);
    let record = d.state.join("signpost/restore.json");
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(&record, r#"{"mimeapps":{"path":"p","existed":false,"added_defaults_group":null,"http":null,"https":null,"effective_http":null,"effective_https":null}}"#).unwrap();
    assert!(load_record(&d.state).unwrap().mimeapps.is_some());
}

#[test]
fn setup_for_a_different_path_is_rejected_before_anything_is_written() {
    let d = dirs(Some(
        "[Default Applications]\nx-scheme-handler/https=firefox.desktop\n",
    ));
    set_default_browser(&d.list, from_ref(&d.list), &d.state).unwrap();
    let other = d.tmp.path().join("other-config/mimeapps.list");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::write(&other, "[Default Applications]\nimage/png=viewer.desktop\n").unwrap();
    let record = d.state.join("signpost/restore.json");
    let before = (read(&d.list), read(&other), read(&record));
    let err = set_default_browser(&other, &[other.clone(), d.list.clone()], &d.state).unwrap_err();
    assert!(
        matches!(&err, SetupError::PathChanged { recorded, current } if *recorded == d.list && *current == other),
        "{err:?}"
    );
    assert!(err.to_string().contains("restore"), "{err}");
    assert_eq!(before, (read(&d.list), read(&other), read(&record)));
}
