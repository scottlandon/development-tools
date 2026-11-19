//! Incremental, parallel transcript scanner: the only module that reads the filesystem.
//!
//! Claude Code appends one JSON object per line to `<projects>/<proj>/<session>.jsonl`
//! (plus `<session>/subagents/*.jsonl`). We look only at `*.jsonl` files and only at lines carrying
//! `message.usage`, and from those lines we keep nothing but numbers, the model name, a timestamp,
//! two ids used to de-duplicate, the project's folder name and a fast-mode yes/no flag. Message text and tool
//! input/output are skipped over, never stored. Per file we remember how many bytes were consumed, so a rescan
//! only parses what was appended. Everything lives in memory; nothing is written anywhere.

use crate::cast;
use memchr::memmem;
use rayon::prelude::*;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::timeutil::parse_iso;

/// One deduplicated assistant message's usage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rec {
    pub key: u64,
    pub ts: i64,
    pub model: u32,
    pub i: u32,
    pub o: u32,
    pub cr: u32,
    pub cw5: u32,
    pub cw1: u32,
    /// Sent in fast mode (`usage.speed == "fast"`).
    pub fast: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FileEntry {
    pub size: u64,
    pub mtime_ns: i64,
    /// Bytes of the file already parsed (always ends on a line boundary).
    pub offset: u64,
    /// First path component under the root (Claude's encoded project dir).
    pub project: String,
    pub session: String,
    /// Display name of the project: its folder name (`~` for the home directory).
    pub label: Option<String>,
    pub models: Vec<String>,
    pub recs: Vec<Rec>,
}

// ---------------------------------------------------------------- parsing

#[derive(Deserialize)]
struct Line<'a> {
    #[serde(borrow, default)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow, default, rename = "requestId")]
    request_id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    cwd: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    message: Option<Msg<'a>>,
}

#[derive(Deserialize)]
struct Msg<'a> {
    #[serde(borrow, default)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    model: Option<Cow<'a, str>>,
    #[serde(default)]
    usage: Option<Usage<'a>>,
}

#[derive(Deserialize)]
struct Usage<'a> {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation: Option<CacheCreation>,
    /// `"fast"` or `"standard"`; absent in older transcripts (standard).
    #[serde(borrow, default)]
    speed: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct CacheCreation {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

/// FNV-1a: a tiny, fast hash for message identity.
fn fnv(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for &b in *p {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn sat32(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Parse complete lines from `buf`; returns the number of bytes consumed
/// (up to and including the last newline). A trailing partial line is left
/// for the next scan, since Claude may be mid-write.
pub fn parse_chunk(buf: &[u8], entry: &mut FileEntry) -> usize {
    let usage_finder = memmem::Finder::new(b"\"usage\"");
    let mut consumed = 0;
    let mut start = 0;
    while let Some(nl) = memchr::memchr(b'\n', &buf[start..]) {
        let end = start + nl;
        let line = &buf[start..end];
        start = end + 1;
        consumed = start;
        if line.len() < 20 || usage_finder.find(line).is_none() {
            continue;
        }
        let Ok(l) = serde_json::from_slice::<Line>(line) else {
            continue;
        };
        let Some(msg) = l.message else { continue };
        let (Some(usage), Some(id), Some(model)) = (msg.usage, msg.id, msg.model) else {
            continue;
        };
        if model.starts_with('<') {
            continue; // "<synthetic>" local-only messages
        }
        let Some(ts) = l.timestamp.as_deref().and_then(parse_iso) else {
            continue;
        };

        let fast = usage.speed.as_deref() == Some("fast");
        let i = usage.input_tokens.unwrap_or(0);
        let o = usage.output_tokens.unwrap_or(0);
        let cr = usage.cache_read_input_tokens.unwrap_or(0);
        let cw = usage.cache_creation_input_tokens.unwrap_or(0);
        let (mut cw5, mut cw1) = (cw, 0);
        if let Some(cc) = usage.cache_creation {
            let e5 = cc.ephemeral_5m_input_tokens.unwrap_or(0);
            let e1 = cc.ephemeral_1h_input_tokens.unwrap_or(0);
            if e5 + e1 > 0 {
                (cw5, cw1) = (e5, e1);
            }
        }
        if i + o + cr + cw5 + cw1 == 0 {
            continue;
        }
        if entry.label.is_none() {
            entry.label = l.cwd.as_deref().and_then(label);
        }
        let midx = cast::to_u32(if let Some(p) = entry.models.iter().position(|m| m == model.as_ref()) {
            p
        } else {
            entry.models.push(model.to_string());
            entry.models.len() - 1
        });
        let rid = l.request_id.as_deref().unwrap_or("");
        entry.recs.push(Rec {
            key: fnv(&[id.as_bytes(), rid.as_bytes()]),
            ts,
            model: midx,
            i: sat32(i),
            o: sat32(o),
            cr: sat32(cr),
            cw5: sat32(cw5),
            cw1: sat32(cw1),
            fast,
        });
    }
    consumed
}

/// Claude writes the same message several times while streaming (one line per
/// content block); usage is cumulative, so keep the copy with the most output.
pub fn dedupe(recs: &mut Vec<Rec>) {
    let mut seen: HashMap<u64, usize> = HashMap::with_capacity(recs.len());
    let mut out: Vec<Rec> = Vec::with_capacity(recs.len());
    for r in recs.drain(..) {
        if let Some(&ix) = seen.get(&r.key) {
            if r.o > out[ix].o {
                out[ix] = r;
            }
        } else {
            seen.insert(r.key, out.len());
            out.push(r);
        }
    }
    *recs = out;
}

// --------------------------------------------------------------- scanning

/// Collect `*.jsonl` files under `dir`. Anything else (credentials, settings, saved tool output,
/// `.env` files) is never opened, and symlinks are never followed, so a link can't lead us out of
/// the transcript directory.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() {
            walk(&p, out);
        } else if ft.is_file() && p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
}

fn mtime_ns(m: &std::fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

/// (project dir name, session id) from a path relative to the root.
fn identify(root: &Path, path: &Path) -> (String, String) {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let comps: Vec<_> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let project = comps.first().cloned().unwrap_or_default();
    let session = match comps.get(1) {
        Some(s) if comps.len() > 2 => s.clone(),
        Some(s) => s.trim_end_matches(".jsonl").to_string(),
        None => String::new(),
    };
    (project, session)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// `/Users/me/src/app` -> `app`; the home directory itself -> `~`.
fn label(cwd: &str) -> Option<String> {
    label_in(cwd, &home())
}

fn label_in(cwd: &str, home: &Path) -> Option<String> {
    let p = Path::new(cwd);
    if p == home {
        return Some("~".into());
    }
    p.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Where Claude Code keeps transcripts (`$CLAUDE_CONFIG_DIR/projects`, else `~/.claude/projects`),
/// plus the Xcode coding assistant's copy. Only directories that exist are returned.
pub fn default_roots() -> Vec<PathBuf> {
    let claude = std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(|| home().join(".claude"), PathBuf::from);
    [
        claude.join("projects"),
        home().join("Library/Developer/Xcode/CodingAssistant/ClaudeAgentConfig/projects"),
    ]
    .into_iter()
    .filter(|p| p.is_dir())
    .collect()
}

pub struct Scanner {
    roots: Vec<PathBuf>,
    pub files: BTreeMap<String, FileEntry>,
}

impl Scanner {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Scanner {
            roots,
            files: BTreeMap::new(),
        }
    }

    /// Bring the in-memory state up to date with disk. Returns true if anything changed.
    pub fn scan(&mut self) -> bool {
        let mut found: Vec<(&Path, PathBuf)> = Vec::new(); // (root, file)
        for root in &self.roots {
            let mut v = Vec::new();
            walk(root, &mut v);
            found.extend(v.into_iter().map(|f| (root.as_path(), f)));
        }

        let known = &self.files;
        let updates: Vec<(String, FileEntry)> = found
            .par_iter()
            .filter_map(|(root, path)| {
                let key = path.to_string_lossy().into_owned();
                let md = std::fs::metadata(path).ok()?;
                let (size, mt) = (md.len(), mtime_ns(&md));
                let prev = known.get(&key);
                if let Some(p) = prev
                    && p.size == size
                    && p.mtime_ns == mt
                {
                    return None;
                }
                let mut entry = match prev {
                    Some(p) if size >= p.offset => p.clone(),
                    _ => {
                        let (project, session) = identify(root, path);
                        FileEntry {
                            project,
                            session,
                            ..Default::default()
                        }
                    }
                };
                let mut f = std::fs::File::open(path).ok()?;
                f.seek(SeekFrom::Start(entry.offset)).ok()?;
                let mut buf = Vec::with_capacity(usize::try_from(size - entry.offset.min(size)).unwrap_or(0));
                f.read_to_end(&mut buf).ok()?;
                let used = parse_chunk(&buf, &mut entry);
                entry.offset += used as u64;
                entry.size = size;
                entry.mtime_ns = mt;
                dedupe(&mut entry.recs);
                Some((key, entry))
            })
            .collect();

        let live: HashSet<String> = found.iter().map(|(_, p)| p.to_string_lossy().into_owned()).collect();
        let changed = !updates.is_empty();
        self.files.extend(updates);
        // Drop entries whose files vanished.
        let before = self.files.len();
        self.files.retain(|k, _| live.contains(k));
        changed || self.files.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, rid: &str, out: u64, ts: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","requestId":"{rid}","cwd":"/tmp/x","message":{{"id":"{id}","model":"claude-sonnet-5-5","content":[{{"type":"text","text":"he said \"usage\""}}],"usage":{{"input_tokens":2,"cache_creation_input_tokens":100,"cache_read_input_tokens":50,"output_tokens":{out},"cache_creation":{{"ephemeral_5m_input_tokens":40,"ephemeral_1h_input_tokens":60}}}}}}}}"#
        )
    }

    #[test]
    fn parses_and_dedupes_streaming_copies() {
        let mut data = String::new();
        data += &line("m1", "r1", 5, "2026-11-05T10:00:00.000Z");
        data += "\n";
        data += &line("m1", "r1", 99, "2026-11-05T10:00:01.000Z");
        data += "\n";
        data += &line("m2", "r2", 7, "2026-11-05T10:00:02.000Z");
        data += "\n";
        data += r#"{"type":"user","message":{"role":"user","content":"hello usage"}}"#;
        data += "\n";
        data += r#"{"type":"assistant","message":{"id":"x","model":"<synthetic>","usage":{"input_tokens":1,"output_tokens":1}}}"#;
        data += "\n";
        let mut e = FileEntry::default();
        let used = parse_chunk(data.as_bytes(), &mut e);
        assert_eq!(used, data.len());
        dedupe(&mut e.recs);
        assert_eq!(e.recs.len(), 2);
        let m1 = e.recs.iter().find(|r| r.o == 99).expect("kept the larger copy");
        assert_eq!((m1.i, m1.cr, m1.cw5, m1.cw1), (2, 50, 40, 60));
        assert_eq!(e.label.as_deref(), Some("x"));
        assert_eq!(e.models, vec!["claude-sonnet-5-5"]);
    }

    #[test]
    fn leaves_partial_trailing_line() {
        let full = line("m1", "r1", 5, "2026-11-05T10:00:00Z");
        let data = format!("{full}\n{}", &full[..full.len() / 2]);
        let mut e = FileEntry::default();
        let used = parse_chunk(data.as_bytes(), &mut e);
        assert_eq!(used, full.len() + 1);
        assert_eq!(e.recs.len(), 1);
    }

    #[test]
    fn no_cache_breakdown_means_all_5m() {
        let l = r#"{"timestamp":"2026-11-05T10:00:00Z","message":{"id":"a","model":"claude-opus-5-5","usage":{"input_tokens":1,"output_tokens":2,"cache_creation_input_tokens":30,"cache_read_input_tokens":null}}}"#;
        let mut e = FileEntry::default();
        parse_chunk(format!("{l}\n").as_bytes(), &mut e);
        assert_eq!((e.recs[0].cw5, e.recs[0].cw1, e.recs[0].cr), (30, 0, 0));
    }

    #[test]
    fn identify_paths() {
        let root = Path::new("/r");
        assert_eq!(
            identify(root, Path::new("/r/-p/abc.jsonl")),
            ("-p".into(), "abc".into())
        );
        assert_eq!(
            identify(root, Path::new("/r/-p/abc/subagents/agent-1.jsonl")),
            ("-p".into(), "abc".into())
        );
    }

    #[test]
    fn incremental_scan_on_disk() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("cc-usage-test-{}", std::process::id()));
        let proj = dir.join("-proj");
        std::fs::create_dir_all(&proj).unwrap();
        let f = proj.join("s1.jsonl");
        std::fs::write(&f, line("m1", "r1", 5, "2026-11-05T10:00:00Z") + "\n").unwrap();
        let mut sc = Scanner::new(vec![dir.clone()]);
        assert!(sc.scan());
        assert!(!sc.scan(), "unchanged file is skipped");
        let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        writeln!(h, "{}", line("m2", "r2", 9, "2026-11-05T10:01:00Z")).unwrap();
        drop(h);
        assert!(sc.scan());
        let e = sc.files.values().next().unwrap();
        assert_eq!(e.recs.len(), 2);
        assert_eq!(e.session, "s1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A scratch directory unique to this test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cc-usage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_jsonl_files_are_ever_opened() {
        let dir = scratch("walk");
        let proj = dir.join("-proj");
        std::fs::create_dir_all(proj.join("sess/tool-results")).unwrap();
        let bait = r#"{"message":{"id":"x","model":"claude-opus-5-5","usage":{"output_tokens":9}},"timestamp":"2026-11-05T10:00:00Z"}"#;
        for f in [
            ".env",
            "credentials.json",
            "settings.json",
            "history.jsonl.bak",
            "sess/tool-results/out.txt",
            "sess/tool-results/out.json",
        ] {
            std::fs::write(proj.join(f), bait).unwrap();
        }
        std::fs::write(
            proj.join("real.jsonl"),
            line("m1", "r1", 5, "2026-11-05T10:00:00Z") + "\n",
        )
        .unwrap();
        std::fs::write(
            proj.join("sess/subagent.jsonl"),
            line("m2", "r2", 5, "2026-11-05T10:00:00Z") + "\n",
        )
        .unwrap();

        let mut sc = Scanner::new(vec![dir.clone()]);
        sc.scan();
        let mut names: Vec<_> = sc
            .files
            .keys()
            .map(|k| k.rsplit('/').next().unwrap().to_string())
            .collect();
        names.sort();
        assert_eq!(names, ["real.jsonl", "subagent.jsonl"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let dir = scratch("symlink");
        let outside = scratch("symlink-outside");
        std::fs::write(
            outside.join("secret.jsonl"),
            line("m9", "r9", 5, "2026-11-05T10:00:00Z") + "\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("linked-dir")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.jsonl"), dir.join("linked-file.jsonl")).unwrap();

        let mut sc = Scanner::new(vec![dir.clone()]);
        assert!(!sc.scan(), "nothing reachable except through links");
        assert!(sc.files.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// The transcript line holds the whole conversation. Whatever else is in it, the only things
    /// that may survive parsing are the numbers, the model, the ids' hash, the folder name and the fast-mode flag.
    #[test]
    fn message_content_is_never_retained() {
        let secret = "sk-ant-api03-TOPSECRET";
        let l = format!(
            r#"{{"type":"assistant","timestamp":"2026-11-05T10:00:00Z","requestId":"r1","cwd":"/home/me/proj-x","gitBranch":"{secret}","message":{{"id":"m1","model":"claude-sonnet-5-5","content":[{{"type":"text","text":"password={secret}"}},{{"type":"tool_use","name":"Bash","input":{{"command":"export KEY={secret}"}}}}],"usage":{{"input_tokens":1,"output_tokens":2,"cache_read_input_tokens":3}}}}}}"#
        );
        let mut e = FileEntry::default();
        parse_chunk(format!("{l}\n").as_bytes(), &mut e);
        assert_eq!(e.recs.len(), 1);
        assert_eq!(e.label.as_deref(), Some("proj-x"));
        assert!(!format!("{e:?}").contains("SECRET"), "{e:?}");
        assert!(!format!("{e:?}").contains("/home/me"), "only the folder name is kept");
    }

    #[test]
    fn only_complete_well_formed_usage_lines_count() {
        let ok = line("m1", "r1", 5, "2026-11-05T10:00:00Z");
        let cases = [
            ("synthetic model", ok.replace("claude-sonnet-5-5", "<synthetic>")),
            (
                "no timestamp",
                ok.replace("\"timestamp\":\"2026-11-05T10:00:00Z\",", ""),
            ),
            ("bad timestamp", ok.replace("2026-11-05T10:00:00Z", "yesterday-ish")),
            ("no message id", ok.replace("\"id\":\"m1\",", "")),
            ("no model", ok.replace("\"model\":\"claude-sonnet-5-5\",", "")),
            ("null usage", ok.replace("\"usage\":{", "\"usage\":null,\"x\":{")),
            ("truncated", ok[..ok.len() - 12].to_string()),
            (
                "all-zero counts",
                line("m2", "r2", 0, "2026-11-05T10:00:00Z")
                    .replace("\"input_tokens\":2", "\"input_tokens\":0")
                    .replace(
                        "\"cache_creation_input_tokens\":100",
                        "\"cache_creation_input_tokens\":0",
                    )
                    .replace("\"cache_read_input_tokens\":50", "\"cache_read_input_tokens\":0")
                    .replace("\"ephemeral_5m_input_tokens\":40", "\"ephemeral_5m_input_tokens\":0")
                    .replace("\"ephemeral_1h_input_tokens\":60", "\"ephemeral_1h_input_tokens\":0"),
            ),
        ];
        for (name, l) in cases {
            let mut e = FileEntry::default();
            parse_chunk(format!("{l}\n").as_bytes(), &mut e);
            assert!(e.recs.is_empty(), "{name} must be skipped: {l}");
        }
        let mut e = FileEntry::default();
        parse_chunk(format!("{ok}\n").as_bytes(), &mut e);
        assert_eq!(e.recs.len(), 1, "the unmodified line does count");
    }

    #[test]
    fn counts_and_cache_split_are_read_exactly() {
        let mut e = FileEntry::default();
        parse_chunk(
            format!("{}\n", line("m1", "r1", 77, "2026-11-05T10:00:00Z")).as_bytes(),
            &mut e,
        );
        let r = e.recs[0];
        // line(): input 2, output 77, cache read 50, cache creation 100 split 40 (5m) + 60 (1h).
        assert_eq!((r.i, r.o, r.cr, r.cw5, r.cw1), (2, 77, 50, 40, 60));
        assert_eq!(r.ts, parse_iso("2026-11-05T10:00:00Z").unwrap());
        assert_eq!(e.models, ["claude-sonnet-5-5"]);
    }

    #[test]
    fn same_message_id_with_different_request_ids_are_different_messages() {
        let mut data = line("m1", "r1", 5, "2026-11-05T10:00:00Z");
        data += "\n";
        data += &line("m1", "r2", 5, "2026-11-05T10:00:01Z");
        data += "\n";
        let mut e = FileEntry::default();
        parse_chunk(data.as_bytes(), &mut e);
        dedupe(&mut e.recs);
        assert_eq!(e.recs.len(), 2);
    }

    #[test]
    fn several_models_in_one_file_are_indexed_once_each() {
        let a = line("m1", "r1", 1, "2026-11-05T10:00:00Z");
        let b = line("m2", "r2", 1, "2026-11-05T10:00:01Z").replace("sonnet-5-5", "opus-5-5");
        let c = line("m3", "r3", 1, "2026-11-05T10:00:02Z");
        let mut e = FileEntry::default();
        parse_chunk(format!("{a}\n{b}\n{c}\n").as_bytes(), &mut e);
        assert_eq!(e.models, ["claude-sonnet-5-5", "claude-opus-5-5"]);
        assert_eq!(e.recs.iter().map(|r| r.model).collect::<Vec<_>>(), [0, 1, 0]);
    }

    #[test]
    fn project_labels() {
        let home = Path::new("/Users/me");
        assert_eq!(label_in("/Users/me/src/app", home).as_deref(), Some("app"));
        assert_eq!(label_in("/Users/me", home).as_deref(), Some("~"), "home itself");
        assert_eq!(label_in("/Users/other", home).as_deref(), Some("other"));
        assert_eq!(label_in("/", home), None, "no folder name to show");
    }

    #[test]
    fn identify_edge_cases() {
        let root = Path::new("/r");
        assert_eq!(identify(root, Path::new("/r")), (String::new(), String::new()));
        assert_eq!(identify(root, Path::new("/r/-p/s.jsonl")), ("-p".into(), "s".into()));
    }

    #[test]
    fn vanished_files_drop_out_and_shrunk_files_are_reparsed() {
        let dir = scratch("vanish");
        let proj = dir.join("-proj");
        std::fs::create_dir_all(&proj).unwrap();
        let (a, b) = (proj.join("a.jsonl"), proj.join("b.jsonl"));
        let two_lines =
            line("m1", "r1", 5, "2026-11-05T10:00:00Z") + "\n" + &line("m3", "r3", 5, "2026-11-05T10:00:01Z") + "\n";
        std::fs::write(&a, &two_lines).unwrap();
        std::fs::write(&b, line("m2", "r2", 5, "2026-11-05T10:00:00Z") + "\n").unwrap();
        let mut sc = Scanner::new(vec![dir.clone()]);
        assert!(sc.scan());
        assert_eq!(sc.files.len(), 2);

        std::fs::remove_file(&b).unwrap();
        assert!(sc.scan(), "a deleted transcript counts as a change");
        assert_eq!(sc.files.len(), 1);

        // Rewritten shorter (truncated and restarted): parse from the top, don't trust the old offset.
        // (Transcripts are append-only, so a same-size rewrite is deliberately treated as unchanged.)
        std::fs::write(&a, line("m9", "r9", 7, "2026-11-05T11:00:00Z") + "\n").unwrap();
        assert!(sc.scan());
        let e = sc.files.values().next().unwrap();
        assert_eq!(e.recs.len(), 1);
        assert_eq!(e.recs[0].o, 7);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_half_written_last_line_is_picked_up_once_it_is_complete() {
        let dir = scratch("partial");
        let proj = dir.join("-proj");
        std::fs::create_dir_all(&proj).unwrap();
        let f = proj.join("s.jsonl");
        let full = line("m1", "r1", 5, "2026-11-05T10:00:00Z");
        let (head, tail) = full.split_at(full.len() / 2);
        std::fs::write(&f, head).unwrap();
        let mut sc = Scanner::new(vec![dir.clone()]);
        sc.scan();
        assert!(
            sc.files.values().next().unwrap().recs.is_empty(),
            "nothing yet: the line isn't finished"
        );
        std::fs::write(&f, format!("{head}{tail}\n")).unwrap();
        assert!(sc.scan());
        assert_eq!(sc.files.values().next().unwrap().recs.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn message_keys_are_stable_and_collision_resistant() {
        let ids: Vec<String> = (0..2000).map(|n| format!("msg_{n:05}")).collect();
        let keys: HashSet<u64> = ids.iter().map(|id| fnv(&[id.as_bytes(), b"req"])).collect();
        assert_eq!(keys.len(), ids.len(), "no collisions among 2000 ids");
        assert_ne!(
            fnv(&[b"ab", b"c"]),
            fnv(&[b"a", b"bc"]),
            "the boundary between id and request id matters"
        );
        assert_eq!(fnv(&[b"m1", b"r1"]), fnv(&[b"m1", b"r1"]));
        assert_ne!(fnv(&[b"m1", b"r1"]), fnv(&[b"m1", b"r2"]));
    }

    fn usage_line(usage: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-11-05T10:00:00Z","requestId":"r","message":{{"id":"m","model":"claude-sonnet-5-5","usage":{usage}}}}}"#
        )
    }

    fn parsed(usage: &str) -> Option<Rec> {
        let mut e = FileEntry::default();
        parse_chunk(format!("{}\n", usage_line(usage)).as_bytes(), &mut e);
        e.recs.first().copied()
    }

    #[test]
    fn a_message_counts_if_any_one_token_field_is_nonzero() {
        for field in [
            "input_tokens",
            "output_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ] {
            assert!(
                parsed(&format!(r#"{{"{field}":7}}"#)).is_some(),
                "{field} alone must count"
            );
        }
        assert!(
            parsed(r#"{"cache_creation":{"ephemeral_1h_input_tokens":7}}"#).is_some(),
            "1h writes alone"
        );
        assert!(
            parsed(r#"{"cache_creation":{"ephemeral_5m_input_tokens":7}}"#).is_some(),
            "5m writes alone"
        );
        assert!(parsed(r#"{"input_tokens":0,"output_tokens":0}"#).is_none());
    }

    #[test]
    fn the_cache_write_breakdown_wins_over_the_total_when_present() {
        let r = parsed(
            r#"{"output_tokens":1,"cache_creation_input_tokens":100,"cache_creation":{"ephemeral_5m_input_tokens":30,"ephemeral_1h_input_tokens":0}}"#,
        )
        .unwrap();
        assert_eq!(
            (r.cw5, r.cw1),
            (30, 0),
            "the breakdown is authoritative when it has anything in it"
        );
        let r = parsed(
            r#"{"output_tokens":1,"cache_creation_input_tokens":100,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0}}"#,
        )
        .unwrap();
        assert_eq!(
            (r.cw5, r.cw1),
            (100, 0),
            "an all-zero breakdown falls back to the total, as 5-minute writes"
        );
        let r = parsed(r#"{"output_tokens":1,"cache_creation_input_tokens":100}"#).unwrap();
        assert_eq!((r.cw5, r.cw1), (100, 0));
        let r = parsed(
            r#"{"output_tokens":1,"cache_creation":{"ephemeral_5m_input_tokens":4,"ephemeral_1h_input_tokens":6}}"#,
        )
        .unwrap();
        assert_eq!((r.cw5, r.cw1), (4, 6));
    }

    #[test]
    fn speed_fast_is_parsed_and_anything_else_is_standard() {
        assert!(parsed(r#"{"output_tokens":1,"speed":"fast"}"#).unwrap().fast);
        assert!(!parsed(r#"{"output_tokens":1,"speed":"standard"}"#).unwrap().fast);
        assert!(
            !parsed(r#"{"output_tokens":1}"#).unwrap().fast,
            "older transcripts omit it"
        );
        assert!(!parsed(r#"{"output_tokens":1,"speed":null}"#).unwrap().fast);
    }

    #[test]
    fn huge_counts_saturate_instead_of_wrapping() {
        let r = parsed(r#"{"output_tokens":99999999999}"#).unwrap();
        assert_eq!(r.o, u32::MAX);
    }

    #[test]
    fn touching_a_file_is_noticed_and_scanning_again_is_quiet() {
        let dir = scratch("touch");
        let proj = dir.join("-proj");
        std::fs::create_dir_all(&proj).unwrap();
        let f = proj.join("s1.jsonl");
        std::fs::write(&f, line("m1", "r1", 5, "2026-11-05T10:00:00Z") + "\n").unwrap();
        let mut sc = Scanner::new(vec![dir.clone()]);
        assert!(sc.scan());
        assert_eq!(
            (
                sc.files.values().next().unwrap().project.as_str(),
                sc.files.values().next().unwrap().session.as_str()
            ),
            ("-proj", "s1")
        );
        assert!(!sc.scan());

        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(
            sc.scan(),
            "a changed modification time means the file is looked at again"
        );
        assert_eq!(
            sc.files.values().next().unwrap().recs.len(),
            1,
            "but nothing is double counted"
        );
        assert!(!sc.scan(), "and then it is quiet again");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
