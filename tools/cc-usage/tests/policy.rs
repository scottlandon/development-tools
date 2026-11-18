//! Guards for the promises in the README: cc-usage only *reads* usage, writes nothing, talks to no
//! network, runs no other programs, and touches the filesystem in exactly one module.
//!
//! These scan the production source (everything above each file's `#[cfg(test)]`) and the
//! dependency list. If one fails, either the change breaks a promise or the promise, the README and
//! this test need to change together, on purpose.
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests")]

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") && p.file_name().is_some_and(|n| n != "tests.rs") {
            out.push(p);
        }
    }
}

/// (file name, production code with comment lines removed).
fn production_sources() -> Vec<(String, String)> {
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    assert!(files.len() > 10, "found the sources");
    files
        .into_iter()
        .map(|p| {
            let text = fs::read_to_string(&p).unwrap();
            let code: Vec<&str> = text
                .lines()
                .take_while(|l| l.trim() != "#[cfg(test)]")
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            (p.file_name().unwrap().to_string_lossy().into_owned(), code.join("\n"))
        })
        .collect()
}

fn assert_absent(what: &str, needles: &[&str]) {
    for (file, code) in production_sources() {
        for n in needles {
            assert!(!code.contains(n), "{file} uses `{n}`: {what}");
        }
    }
}

#[test]
fn never_writes_to_disk() {
    assert_absent(
        "cc-usage must not create, modify, move or delete files",
        &[
            "fs::write",
            "File::create",
            "create_dir",
            "remove_file",
            "remove_dir",
            "fs::rename",
            "fs::copy",
            "OpenOptions",
            "set_permissions",
            "hard_link",
            "symlink",
            "set_len",
        ],
    );
}

#[test]
fn never_uses_the_network() {
    assert_absent(
        "cc-usage must not open sockets",
        &[
            "std::net",
            "TcpStream",
            "TcpListener",
            "UdpSocket",
            "UnixStream",
            "UnixListener",
            "reqwest",
            "hyper",
            "ureq",
            "tokio",
        ],
    );
}

#[test]
fn never_runs_other_programs_or_unsafe_code() {
    assert_absent(
        "cc-usage must not spawn processes",
        &["Command::new", "process::Command"],
    );
    assert_absent("no unsafe code", &["unsafe "]);
}

#[test]
fn only_the_scanner_touches_the_filesystem() {
    for (file, code) in production_sources() {
        if file == "scan.rs" {
            continue;
        }
        for n in [
            "std::fs",
            "fs::",
            "File::open",
            "read_dir",
            "read_to_string",
            "BufReader",
        ] {
            assert!(!code.contains(n), "{file} uses `{n}`: only scan.rs may read files");
        }
    }
}

#[test]
fn only_these_environment_variables_are_read() {
    let allowed = ["HOME", "CLAUDE_CONFIG_DIR", "COLORTERM"];
    for (file, code) in production_sources() {
        assert!(!code.contains("env::vars"), "{file} enumerates the environment");
        for call in ["var_os(", "env::var("] {
            for (at, _) in code.match_indices(call) {
                let rest = &code[at + call.len()..];
                let name = rest
                    .strip_prefix('"')
                    .and_then(|r| r.split('"').next())
                    .unwrap_or_else(|| panic!("{file}: environment variable name must be a literal"));
                assert!(
                    allowed.contains(&name),
                    "{file} reads ${name}, which is not on the allow-list"
                );
            }
        }
    }
}

#[test]
fn dependencies_are_exactly_the_reviewed_set() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let mut names: Vec<&str> = manifest
        .split("[dependencies]")
        .nth(1)
        .expect("a [dependencies] section")
        .lines()
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter_map(|l| l.split('=').next())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["chrono", "memchr", "ratatui", "rayon", "serde", "serde_json"],
        "a new dependency is a new thing that can read, write or phone home: review it, then update \
         this list, deny.toml and the README together"
    );
}
