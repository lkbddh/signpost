//! Names the commit Signpost is built from, `-dirty` when tracked files differ from it, so that
//! `signpost --version` says which build it is and `just install` can refuse a stale one.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    let commit = git(&["rev-parse", "--short=12", "HEAD"]).map_or_else(
        || "unknown".to_owned(),
        |hash| {
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .is_some_and(|changes| !changes.is_empty());
            if dirty {
                return format!("{hash}-dirty");
            }
            hash
        },
    );
    println!("cargo:rustc-env=SIGNPOST_COMMIT={commit}");
    // What the binary is built from, so an edit or a commit names the build again.
    for path in [
        "src",
        "data",
        "i18n",
        "resources",
        "vendor",
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    // The commit: what HEAD names, the branch ref it points to, and the index that `-dirty` compares.
    let branch = git(&["symbolic-ref", "-q", "HEAD"]);
    let watched = ["HEAD", "index", "packed-refs"]
        .into_iter()
        .map(str::to_owned)
        .chain(branch);
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}
