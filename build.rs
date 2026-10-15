//! Embeds gmd, the Go side (see gmd/), built ahead for each target: downloaded from the
//! GitHub release named in gmd/release.txt and checked against its SHA-256 there.
//!
//! `GMD_PATH=/path/to/gmd` uses that file instead (offline builds, or a gmd built from
//! gmd/ with `go build`); it is not checked.

use std::path::{Path, PathBuf};
use std::{env, fs};

use sha2::{Digest, Sha256};

const RELEASES: &str = "https://github.com/louisrubet/gmessages-tui/releases/download";

fn main() {
    println!("cargo:rerun-if-changed=gmd/release.txt");
    println!("cargo:rerun-if-env-changed=GMD_PATH");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("gmd");

    let bytes = match env::var_os("GMD_PATH") {
        Some(path) => fs::read(&path)
            .unwrap_or_else(|e| panic!("cannot read GMD_PATH {}: {e}", Path::new(&path).display())),
        None => download(),
    };
    let sha = hex(&Sha256::digest(&bytes));
    fs::write(&out, &bytes).expect("cannot write gmd to OUT_DIR");
    println!("cargo:rustc-env=GMD_SHA256={sha}");
}

/// The gmd of the target, from the release, checked.
fn download() -> Vec<u8> {
    let asset = asset();
    let release = fs::read_to_string("gmd/release.txt").expect("cannot read gmd/release.txt");
    let mut lines = release.lines();
    let tag = lines.next().expect("gmd/release.txt is empty");
    let expected = lines
        .filter_map(|line| line.split_once(' '))
        .find(|(name, _)| *name == asset)
        .map(|(_, sha)| sha.trim())
        .unwrap_or_else(|| panic!("no {asset} in gmd/release.txt"));

    let url = format!("{RELEASES}/{tag}/{asset}");
    let bytes = ureq::get(&url)
        .call()
        .and_then(|mut response| {
            response
                .body_mut()
                .with_config()
                .limit(64 * 1024 * 1024)
                .read_to_vec()
        })
        .unwrap_or_else(|e| {
            panic!("cannot download {url}: {e}\nOffline? Build gmd from gmd/ and set GMD_PATH.")
        });
    let sha = hex(&Sha256::digest(&bytes));
    assert_eq!(
        sha, expected,
        "{url} does not match its SHA-256 in gmd/release.txt"
    );
    bytes
}

/// Name of the gmd built for the target.
fn asset() -> String {
    let os = match env::var("CARGO_CFG_TARGET_OS").unwrap().as_str() {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        other => panic!("no gmd for {other}: build it from gmd/ and set GMD_PATH"),
    };
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => panic!("no gmd for {other}: build it from gmd/ and set GMD_PATH"),
    };
    let ext = if os == "windows" { ".exe" } else { "" };
    format!("gmd-{os}-{arch}{ext}")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
