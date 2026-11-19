//! End to end: run the real binary on synthetic transcripts and check what it prints and touches.
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_cc-usage");

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cc-usage-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Yesterday (UTC), so fixture messages are always recent but never in the future.
fn yesterday() -> String {
    (chrono::Utc::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string()
}

/// Copy the fixture transcripts to `dest`, filling in the `{DATE}` placeholder.
fn copy_fixtures(from: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), dest.join(e.file_name()));
        if src.is_dir() {
            copy_fixtures(&src, &dst);
        } else if src.extension().is_some_and(|x| x == "jsonl") {
            fs::write(dst, fs::read_to_string(&src).unwrap().replace("{DATE}", &yesterday())).unwrap();
        }
    }
}

fn fixture_projects(name: &str) -> PathBuf {
    let dest = scratch(name).join("projects");
    copy_fixtures(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projects"),
        &dest,
    );
    dest
}

/// Run with a minimal environment: only what the test sets, so nothing leaks in from the machine.
fn run(args: &[&str], env: &[(&str, &Path)]) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.args(args).env_clear();
    // Lets `cargo llvm-cov` see the child process; absent in normal runs.
    if let Some(p) = std::env::var_os("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", p);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn report(args: &[&str], projects: &Path) -> String {
    let mut a = vec!["--print", "--days", "3", "--projects-dir", projects.to_str().unwrap()];
    a.extend_from_slice(args);
    let out = run(&a, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    stdout(&out)
}

/// The whitespace-separated cells of the first line starting with `label`.
fn row(report: &str, label: &str) -> Vec<String> {
    report
        .lines()
        .find(|l| l.starts_with(label))
        .unwrap_or_else(|| panic!("no `{label}` row in:\n{report}"))
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// Every file under `dir` with its size, to prove a run changed nothing.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, u64> {
    fn walk(d: &Path, out: &mut BTreeMap<PathBuf, u64>) {
        for e in fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            out.insert(p.clone(), e.metadata().unwrap().len());
            if p.is_dir() {
                walk(&p, out);
            }
        }
    }
    let mut m = BTreeMap::new();
    walk(dir, &mut m);
    m
}

#[test]
fn report_matches_hand_computed_totals() {
    let projects = fixture_projects("totals");
    let r = report(&[], &projects);

    // m1 (streamed twice; the copy with 40k output wins) + m2 (opus) + an unpriced model + m3 (subagent file).
    // Skipped: the user line, the <synthetic> model, the truncated line and the all-zero message.
    assert_eq!(
        row(&r, "TOTAL"),
        [
            "TOTAL",
            "1,160,000",
            "1,260,000",
            "500,000",
            "12,000,000",
            "14,920,000",
            "$8.77",
            "4"
        ]
    );
    // sonnet: m1 = 2.35 and m3 = 0.22 (m1's 5m and 1h cache writes billed at 1.25x and 2x input)
    assert_eq!(
        row(&r, "sonnet-5-5"),
        [
            "sonnet-5-5",
            "110,000",
            "60,000",
            "500,000",
            "2,000,000",
            "2,670,000",
            "$2.57",
            "2"
        ]
    );
    assert_eq!(
        row(&r, "opus-5-5"),
        [
            "opus-5-5",
            "50,000",
            "200,000",
            "0",
            "10,000,000",
            "10,250,000",
            "$6.20",
            "1"
        ]
    );
    // An unknown model still counts its tokens, but is never priced at $0.
    assert_eq!(
        row(&r, "mystery-model-1"),
        [
            "mystery-model-1",
            "1,000,000",
            "1,000,000",
            "0",
            "0",
            "2,000,000",
            "n/a",
            "1"
        ]
    );
    assert!(r.contains("1 messages use models with no known price"), "{r}");
    assert!(!r.contains("transcript files but no usage"), "{r}");
}

#[test]
fn finds_transcripts_in_the_default_location_and_changes_nothing() {
    let home = scratch("home");
    copy_fixtures(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projects"),
        &home.join(".claude/projects"),
    );
    let before = snapshot(&home);

    let out = run(&["--print", "--days", "3"], &[("HOME", &home)]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(row(&stdout(&out), "TOTAL")[5], "14,920,000");

    // CLAUDE_CONFIG_DIR moves the transcripts; HOME no longer matters.
    let elsewhere = scratch("cfgdir");
    copy_fixtures(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projects"),
        &elsewhere.join("projects"),
    );
    let elsewhere_before = snapshot(&elsewhere);
    let empty_home = scratch("emptyhome");
    let out = run(
        &["--print", "--days", "3"],
        &[("HOME", &empty_home), ("CLAUDE_CONFIG_DIR", &elsewhere)],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(row(&stdout(&out), "TOTAL")[5], "14,920,000");

    assert_eq!(
        snapshot(&home),
        before,
        "a run must not create, change or delete any file"
    );
    assert_eq!(
        snapshot(&elsewhere),
        elsewhere_before,
        "transcripts untouched, nothing added"
    );
    assert!(snapshot(&empty_home).is_empty(), "nothing written to HOME either");
}

#[test]
fn says_so_when_transcripts_have_no_usage_records() {
    let dir = scratch("drift");
    let proj = dir.join("-proj");
    fs::create_dir_all(&proj).unwrap();
    // A log format this version doesn't understand: no `message.usage` anywhere.
    fs::write(
        proj.join("s.jsonl"),
        "{\"type\":\"assistant\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"message\":{\"id\":\"a\",\"tokens\":{\"in\":5,\"out\":7}}}\n",
    )
    .unwrap();
    let r = report(&[], &dir);
    assert!(r.contains("found 1 transcript files but no usage records"), "{r}");
    assert_eq!(row(&r, "TOTAL")[5], "0");
}

#[test]
fn an_empty_transcript_directory_is_not_an_error() {
    let dir = scratch("empty");
    let r = report(&[], &dir);
    assert_eq!(row(&r, "TOTAL")[5], "0");
    assert!(
        !r.contains("transcript files"),
        "no files means nothing to warn about:\n{r}"
    );
}

#[test]
fn without_any_transcript_directory_it_says_where_it_looked() {
    let home = scratch("nohome");
    let out = run(&["--print"], &[("HOME", &home)]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("no transcript directory found"),
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), "");
}

#[test]
fn demo_needs_no_transcripts_and_touches_nothing() {
    let home = scratch("demohome");
    let out = run(&["--demo", "--print", "--days", "7"], &[("HOME", &home)]);
    assert!(out.status.success(), "{}", stderr(&out));
    let r = stdout(&out);
    assert!(row(&r, "TOTAL")[7].parse::<u32>().is_ok(), "{r}");
    assert!(r.contains("By model"), "{r}");
    assert!(snapshot(&home).is_empty());
}

#[test]
fn bad_arguments_exit_2_with_a_useful_message() {
    for (args, want) in [
        (&["--range", "bogus"][..], "unknown range 'bogus'"),
        (
            &["--theme", "neon"],
            "choose from: mainframe, amber, catppuccin, strawberry-milk, daylight",
        ),
        (&["--color", "rainbow"], "unknown --color 'rainbow'"),
        (&["--days", "x"], "--days needs a number"),
        (&["--refresh"], "--refresh needs a value"),
        (&["--frobnicate"], "unknown argument '--frobnicate'"),
    ] {
        let out = run(args, &[]);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(stderr(&out).contains(want), "{args:?}: {}", stderr(&out));
    }
}

#[test]
fn non_finite_refresh_is_rejected() {
    for v in ["nan", "inf", "-inf", "NaN"] {
        let out = run(&["--refresh", v, "--demo", "--print"], &[]);
        assert_eq!(out.status.code(), Some(2), "{v}");
        assert!(
            stderr(&out).contains("--refresh needs a number"),
            "{v}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn huge_days_is_clamped() {
    let started = std::time::Instant::now();
    let out = run(&["--demo", "--print", "--days", "1000000000"], &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(started.elapsed().as_secs() < 30, "took {:?}", started.elapsed());
    let n = stdout(&out).lines().count();
    assert!(n <= 3650 + 100, "{n} lines");
    assert!(n > 100, "{n} lines");
}

#[test]
fn help_and_version() {
    let out = run(&["--help"], &[]);
    assert!(out.status.success());
    let h = stdout(&out);
    assert!(h.contains("Read-only"), "{h}");
    assert!(
        h.contains("built-in prices were last checked 20"),
        "price date is filled in:\n{h}"
    );
    assert!(!h.contains("{PRICES}"), "placeholder left in help");
    for flag in [
        "--range",
        "--print",
        "--days",
        "--projects-dir",
        "--refresh",
        "--theme",
        "--color",
        "--demo",
    ] {
        assert!(h.contains(flag), "help is missing {flag}");
    }
    let out = run(&["-V"], &[]);
    assert_eq!(stdout(&out).trim(), format!("cc-usage {}", env!("CARGO_PKG_VERSION")));
}

/// Drive the real TUI through a pseudo-terminal: it draws, reacts to keys, quits on `q`, and puts the
/// terminal back. Uses BSD `script` to get a pty, so this runs on macOS only; Linux's `script` has a
/// different command line, and the in-memory render tests in `src/ui/tests.rs` cover the drawing there.
#[cfg(target_os = "macos")]
#[test]
fn the_tui_draws_handles_keys_and_restores_the_terminal() {
    use std::io::Write;
    use std::process::Stdio;
    use std::time::Duration;

    fn strip_ansi(bytes: &[u8]) -> String {
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'[') {
                i += 2;
                while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                    i += 1;
                }
            } else {
                out.push(bytes[i]);
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    let attempt = || {
        let mut cmd = Command::new("script");
        cmd.args([
            "-q",
            "/dev/null",
            "sh",
            "-c",
            &format!("stty rows 30 cols 100; exec '{BIN}' --demo"),
        ])
        .env_clear()
        .env("TERM", "xterm-256color")
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
        if let Some(p) = std::env::var_os("LLVM_PROFILE_FILE") {
            cmd.env("LLVM_PROFILE_FILE", p);
        }
        let mut child = cmd.spawn().unwrap();
        let mut keys = child.stdin.take().unwrap();
        std::thread::sleep(Duration::from_millis(600));
        keys.write_all(b"5").unwrap(); // Live tab
        keys.flush().unwrap();
        std::thread::sleep(Duration::from_millis(800));
        keys.write_all(b"q").unwrap();
        drop(keys);
        let out = child.wait_with_output().unwrap();
        (out.status.success(), out.stdout)
    };

    // Keys can occasionally coalesce on a loaded machine; allow a couple of tries before failing.
    let mut last = String::new();
    for _ in 0..3 {
        let (ok, raw) = attempt();
        let screen = strip_ansi(&raw);
        let enter = raw.windows(8).any(|w| w == b"\x1b[?1049h");
        let leave = raw.windows(8).any(|w| w == b"\x1b[?1049l");
        if ok && enter && leave && screen.contains("1 Daily") && screen.contains("Tokens / min") {
            return;
        }
        last = format!("ok={ok} enter={enter} leave={leave}\n{screen}");
    }
    panic!("TUI session didn't look right:\n{last}");
}
