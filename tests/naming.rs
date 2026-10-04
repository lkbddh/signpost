//! Signpost goes by one name, `com.lkbddh.signpost`, and the repository is fit to publish: git tracks only the
//! published tree, and no tracked file carries planning shorthand that means nothing to its readers.

use std::path::{Path, PathBuf};
use std::process::Command;

use signpost::{bus, index, setup};

#[test]
fn the_app_has_one_name() {
    assert_eq!(bus::APP_ID, "com.lkbddh.signpost");
    assert_eq!(bus::APP_PATH, "/com/lkbddh/signpost");
    assert_eq!(index::SELF_ID, bus::APP_ID);
    assert_eq!(setup::DESKTOP_FILE, format!("{}.desktop", bus::APP_ID));
}

/// Everything at the top of the published tree. Anything else git tracks there was added by mistake.
const PUBLISHED: [&str; 25] = [
    ".github",
    ".gitignore",
    "CODE_OF_CONDUCT.md",
    "CONTRIBUTING.md",
    "Cargo.lock",
    "Cargo.toml",
    "LICENSE",
    "README.md",
    "SECURITY.md",
    "THIRD_PARTY_LICENSES.md",
    "THIRD_PARTY_NOTICES.md",
    "build.rs",
    "cargo-sources.json",
    "com.lkbddh.signpost.yml",
    "data",
    "i18n",
    "i18n.toml",
    "justfile",
    "licenses",
    "resources",
    "rust-toolchain.toml",
    "scripts",
    "src",
    "tests",
    "vendor",
];

/// Each path git tracks under `root` outside the published tree.
fn stray(root: &Path) -> Vec<String> {
    tracked(root)
        .iter()
        .filter(|path| {
            let top = path
                .components()
                .next()
                .map(|part| part.as_os_str().to_string_lossy().into_owned());
            !top.is_some_and(|top| PUBLISHED.contains(&top.as_str()))
        })
        .map(|path| path.display().to_string())
        .collect()
}

#[test]
fn only_the_published_tree_is_tracked() {
    let Some(root) = checkout() else { return };
    let found = stray(root);
    assert!(
        found.is_empty(),
        "{} tracked outside the published tree:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn a_tracked_folder_outside_the_published_tree_is_stray() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let project = scratch.path();
    std::fs::create_dir_all(project.join("notes")).expect("a folder");
    std::fs::write(project.join("notes/draft.md"), "hay\n").expect("a note");
    std::fs::write(project.join("README.md"), "hay\n").expect("a readme");
    git(project, &["init", "--quiet"]);
    git(project, &["add", "notes/draft.md", "README.md"]);
    assert_eq!(stray(project), ["notes/draft.md"]);
}

/// Planning shorthand, the section, plan and task numbers that only mean something to someone holding the plans.
/// Spelled at run time, so this file does not hold them.
fn pointers() -> Vec<String> {
    let mut pointers = vec![["spec ", "§"].concat()];
    pointers.extend((1..=9).map(|n| format!("plan {n}")));
    pointers.extend((1..=9).map(|n| format!("task {n}")));
    pointers
}

/// Each place in the files git tracks under `root` that holds planning shorthand, with the shorthand found.
fn pointers_in(root: &Path) -> Vec<String> {
    pointers()
        .iter()
        .flat_map(|pointer| {
            scan(root, pointer, Start::OfWord)
                .into_iter()
                .map(move |at| format!("{at} ({pointer})"))
        })
        .collect()
}

#[test]
fn no_tracked_file_holds_planning_shorthand() {
    let Some(root) = checkout() else { return };
    let found = pointers_in(root);
    assert!(
        found.is_empty(),
        "{} pointers:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn shorthand_in_a_tracked_file_is_found_by_its_line() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let project = scratch.path();
    let pointer = ["plan ", "3"].concat();
    std::fs::write(
        project.join("README"),
        format!("hay\nsee {pointer} for why\n"),
    )
    .expect("a tracked file");
    git(project, &["init", "--quiet"]);
    git(project, &["add", "README"]);
    assert_eq!(pointers_in(project), [format!("README:2 ({pointer})")]);
}

#[test]
fn a_pointer_counts_only_where_a_word_starts() {
    let pointer = ["task ", "4"].concat();
    assert!(holds(&format!("see {pointer}"), &pointer, Start::OfWord));
    assert!(holds(&format!("({pointer})"), &pointer, Start::OfWord));
    assert!(holds(&pointer, &pointer, Start::OfWord));
    assert!(!holds(
        &format!("`async-{pointer}.7.1`"),
        &pointer,
        Start::OfWord
    ));
    assert!(!holds(&format!("sub{pointer}"), &pointer, Start::OfWord));
    assert!(holds(&format!("sub{pointer}"), &pointer, Start::Anywhere));
}

/// The project root when it is a git checkout; outside one there are no tracked files to hold to these rules.
fn checkout() -> Option<&'static Path> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    if root.join(".git").exists() {
        return Some(root);
    }
    eprintln!("not a git checkout: there are no tracked files to hold to this");
    None
}

#[test]
fn the_scan_reads_tracked_files_only_and_never_through_a_link() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (project, outside) = (
        scratch.path().join("project"),
        scratch.path().join("outside"),
    );
    std::fs::create_dir_all(&project).expect("the project folder");
    std::fs::create_dir_all(&outside).expect("a folder outside it");
    std::fs::write(outside.join("private"), "needle\n").expect("a file outside");
    std::fs::write(project.join("tracked"), "hay\nneedle\n").expect("a tracked file");
    std::fs::write(project.join("untracked"), "needle\n").expect("an untracked file");
    std::os::unix::fs::symlink(outside.join("private"), project.join("link"))
        .expect("a link out of the project");
    git(&project, &["init", "--quiet"]);
    git(&project, &["add", "tracked", "link"]);
    assert_eq!(mentions(&project, "needle"), ["tracked:2"]);
}

/// Where a match may begin: anywhere, or only where a word starts, so a numbered pointer that ends a crate
/// name and its version (`async-` then the pointer) is not one, while the same words after a space or `(` are.
#[derive(Clone, Copy)]
enum Start {
    Anywhere,
    OfWord,
}

/// Whether `haystack` holds `needle` (both lowercase) beginning at an allowed `start`.
fn holds(haystack: &str, needle: &str, start: Start) -> bool {
    haystack.match_indices(needle).any(|(at, _)| match start {
        Start::Anywhere => true,
        Start::OfWord => haystack[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !(before.is_alphanumeric() || before == '-' || before == '_')),
    })
}

/// Each `path:line` of a file git tracks under `root` that holds `needle`, any case, and each tracked path
/// that names it. A tracked link, or a file reached through a linked folder, is judged by its name and never
/// read; any failure to list or read a file fails the test rather than leaving that file unchecked.
fn mentions(root: &Path, needle: &str) -> Vec<String> {
    scan(root, needle, Start::Anywhere)
}

/// [`mentions`], with matches allowed to begin only at `start`.
fn scan(root: &Path, needle: &str, start: Start) -> Vec<String> {
    let needle = needle.to_lowercase();
    let mut found = Vec::new();
    for relative in tracked(root) {
        let shown = relative.display().to_string();
        if holds(&shown.to_lowercase(), &needle, start) {
            found.push(shown.clone());
        }
        if through_a_link(root, &relative) {
            continue;
        }
        let path = root.join(&relative);
        let meta = std::fs::symlink_metadata(&path)
            .unwrap_or_else(|e| panic!("cannot inspect {shown}: {e}"));
        if !meta.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {shown}: {e}"));
        for (number, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
            if holds(
                &String::from_utf8_lossy(line).to_lowercase(),
                &needle,
                start,
            ) {
                found.push(format!("{shown}:{}", number + 1));
            }
        }
    }
    found
}

/// Whether `relative`, or any folder on its way from `root`, is a link: reading it would leave the project.
fn through_a_link(root: &Path, relative: &Path) -> bool {
    let mut walked = PathBuf::new();
    relative.components().any(|part| {
        walked.push(part);
        std::fs::symlink_metadata(root.join(&walked))
            .unwrap_or_else(|e| panic!("cannot inspect {}: {e}", walked.display()))
            .file_type()
            .is_symlink()
    })
}

/// The files git tracks under `root`, relative to it.
fn tracked(root: &Path) -> Vec<PathBuf> {
    let listed = git(root, &["ls-files", "-z"]);
    listed
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(String::from_utf8(path.to_vec()).expect("a UTF-8 path")))
        .collect()
}

/// Runs git in `dir` with no user or system configuration, and returns what it printed.
fn git(dir: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn the_scan_never_reads_through_a_linked_folder() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (project, outside) = (
        scratch.path().join("project"),
        scratch.path().join("outside"),
    );
    std::fs::create_dir_all(project.join("nested")).expect("a nested folder");
    std::fs::create_dir_all(&outside).expect("a folder outside it");
    std::fs::write(project.join("nested/private"), "hay\n").expect("a tracked nested file");
    git(&project, &["init", "--quiet"]);
    git(&project, &["add", "nested/private"]);
    // The tracked folder is swapped for a link to a folder outside that holds the same name.
    std::fs::remove_dir_all(project.join("nested")).expect("the folder goes");
    std::fs::write(outside.join("private"), "needle\n").expect("a file outside");
    std::os::unix::fs::symlink(&outside, project.join("nested")).expect("a linked folder");
    assert_eq!(mentions(&project, "needle"), Vec::<String>::new());
}
