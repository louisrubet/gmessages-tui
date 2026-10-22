//! Gives the version: the last release tag (`v1.0.0`), followed by the commit
//! (`v1.0.0+65028`) when built beyond it, and `-dirty` with uncommitted changes; the
//! version of Cargo.toml out of git.

use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rustc-env=GMESSAGES_TUI_VERSION={}", version());
}

/// The last release tag, `+` the commit when beyond it or with uncommitted changes (then
/// `-dirty`).
fn version() -> String {
    // Built again on a commit, a tag, or a change to the sources.
    let branch = git(&["symbolic-ref", "--quiet", "HEAD"]);
    for path in ["HEAD", "index", "refs/tags", "packed-refs"]
        .into_iter()
        .chain(branch.as_deref())
    {
        if let Some(path) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    for path in ["build.rs", "Cargo.toml", "Cargo.lock", "src"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let Some(tag) = git(&["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"]) else {
        return format!("v{}", env::var("CARGO_PKG_VERSION").unwrap());
    };
    let beyond = git(&["rev-list", "--count", &format!("{tag}..HEAD")]).is_some_and(|n| n != "0");
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some();
    match git(&["rev-parse", "--short=5", "HEAD"]) {
        Some(commit) if dirty => format!("{tag}+{commit}-dirty"),
        Some(commit) if beyond => format!("{tag}+{commit}"),
        _ => tag,
    }
}

/// The output of `git args`, if it succeeds and says something.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    let out = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && !out.is_empty()).then_some(out)
}
