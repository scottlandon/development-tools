//! Flattens the per-file scan results into one deduplicated, time-sorted row table
//! with interned model/project/session ids and precomputed local day + cost.

use crate::cast;
use crate::pricing::{self, Price};
use crate::scan::FileEntry;
use crate::timeutil::LocalDays;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub ts: i64,
    pub day: i32,
    pub model: u16,
    pub project: u16,
    pub session: u32,
    pub i: u32,
    pub o: u32,
    pub cr: u32,
    pub cw5: u32,
    pub cw1: u32,
    /// API-equivalent USD; 0 when the model has no known price.
    pub cost: f64,
    pub priced: bool,
}

#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub short: String,
    pub price: Option<Price>,
}

#[derive(Default, Debug)]
pub struct Dataset {
    pub rows: Vec<Row>,
    pub models: Vec<ModelInfo>,
    pub projects: Vec<String>,
    /// Number of distinct sessions (ids in `Row::session` run `0..sessions`).
    pub sessions: u32,
    pub files: usize,
}

/// `claude-sonnet-5-5-20260101` -> `sonnet-5-5`.
pub fn short_model(name: &str) -> String {
    let s = name.strip_prefix("claude-").unwrap_or(name);
    match s.rsplit_once('-') {
        Some((head, tail)) if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit()) => head.to_string(),
        _ => s.to_string(),
    }
}

impl Dataset {
    /// Transcript files were found but none yielded a usage record. Either those sessions are
    /// brand new or Claude Code changed its log format; better to say so than to show zeros.
    pub fn looks_unparsed(&self) -> bool {
        self.files > 0 && self.rows.is_empty()
    }

    pub fn build(files: &BTreeMap<String, FileEntry>) -> Dataset {
        let mut ds = Dataset {
            files: files.len(),
            ..Default::default()
        };
        let mut model_ix: HashMap<&str, u16> = HashMap::new();
        let mut project_ix: HashMap<&str, u16> = HashMap::new();
        let mut session_ix: HashMap<(u16, &str), u32> = HashMap::new();
        let mut seen: HashMap<u64, usize> = HashMap::new();
        let mut days = LocalDays::default();

        for entry in files.values() {
            let pi = *project_ix.entry(entry.project.as_str()).or_insert_with(|| {
                let name = entry.label.clone();
                ds.projects
                    .push(name.unwrap_or_else(|| entry.project.trim_start_matches('-').to_string()));
                cast::wrapping_u16_id(ds.projects.len() - 1)
            });
            let si = *session_ix.entry((pi, entry.session.as_str())).or_insert_with(|| {
                ds.sessions += 1;
                ds.sessions - 1
            });
            let remap: Vec<u16> = entry
                .models
                .iter()
                .map(|m| {
                    *model_ix.entry(m.as_str()).or_insert_with(|| {
                        ds.models.push(ModelInfo {
                            short: short_model(m),
                            price: pricing::lookup(m),
                        });
                        cast::wrapping_u16_id(ds.models.len() - 1)
                    })
                })
                .collect();

            for r in &entry.recs {
                let model = remap[r.model as usize];
                if let Some(&ix) = seen.get(&r.key) {
                    // Same message seen in another file (resumed/forked
                    // session): keep first attribution, max output.
                    let row = &mut ds.rows[ix];
                    if r.o > row.o {
                        row.o = r.o;
                    }
                } else {
                    seen.insert(r.key, ds.rows.len());
                    ds.rows.push(Row {
                        ts: r.ts,
                        day: days.day(r.ts),
                        model,
                        project: pi,
                        session: si,
                        i: r.i,
                        o: r.o,
                        cr: r.cr,
                        cw5: r.cw5,
                        cw1: r.cw1,
                        cost: 0.0,
                        priced: false,
                    });
                }
            }
        }

        for r in &mut ds.rows {
            if let Some(p) = ds.models[r.model as usize].price {
                r.cost = p.cost(
                    u64::from(r.i),
                    u64::from(r.o),
                    u64::from(r.cr),
                    u64::from(r.cw5),
                    u64::from(r.cw1),
                );
                r.priced = true;
            }
        }
        ds.rows.sort_unstable_by_key(|r| r.ts);
        ds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_model_names() {
        assert_eq!(short_model("claude-sonnet-5-5"), "sonnet-5-5");
        assert_eq!(short_model("claude-haiku-4-5-20251001"), "haiku-4-5");
        assert_eq!(short_model("weird"), "weird");
    }

    use crate::scan::Rec;

    fn rec(key: u64, ts: i64, o: u32) -> Rec {
        Rec {
            key,
            ts,
            model: 0,
            i: 10,
            o,
            cr: 0,
            cw5: 0,
            cw1: 0,
        }
    }

    fn file(project: &str, session: &str, label: Option<&str>, model: &str, recs: Vec<Rec>) -> FileEntry {
        FileEntry {
            project: project.into(),
            session: session.into(),
            label: label.map(String::from),
            models: vec![model.into()],
            recs,
            ..Default::default()
        }
    }

    #[test]
    fn a_message_repeated_in_a_resumed_session_counts_once_with_the_most_output() {
        let mut files = BTreeMap::new();
        // Same message key in two files (a forked session copies history); the later copy has more output.
        files.insert(
            "a".into(),
            file("-p", "s1", Some("p"), "claude-sonnet-5-5", vec![rec(7, 100, 5)]),
        );
        files.insert(
            "b".into(),
            file(
                "-p",
                "s2",
                Some("p"),
                "claude-sonnet-5-5",
                vec![rec(7, 100, 9), rec(8, 200, 1)],
            ),
        );
        let ds = Dataset::build(&files);
        assert_eq!(ds.rows.len(), 2, "keys 7 and 8");
        let dup = ds.rows.iter().find(|r| r.ts == 100).unwrap();
        assert_eq!(dup.o, 9, "kept the larger output");
        assert_eq!(dup.session, 0, "attributed to the first file seen");
    }

    #[test]
    fn projects_models_and_sessions_are_interned_and_named() {
        let mut files = BTreeMap::new();
        files.insert(
            "a".into(),
            file(
                "-Users-me-app",
                "s1",
                Some("app"),
                "claude-sonnet-5-5-20260101",
                vec![rec(1, 10, 1)],
            ),
        );
        files.insert(
            "b".into(),
            file(
                "-Users-me-app",
                "s1",
                Some("app"),
                "claude-opus-5-5",
                vec![rec(2, 20, 1)],
            ),
        );
        files.insert(
            "c".into(),
            file(
                "-Users-me-app",
                "s2",
                Some("app"),
                "claude-sonnet-5-5-20260101",
                vec![rec(3, 30, 1)],
            ),
        );
        files.insert(
            "d".into(),
            file(
                "-Users-me-lib",
                "s1",
                None,
                "claude-sonnet-5-5-20260101",
                vec![rec(4, 40, 1)],
            ),
        );
        let ds = Dataset::build(&files);
        assert_eq!(ds.files, 4);
        assert_eq!(
            ds.projects,
            ["app", "Users-me-lib"],
            "label if known, else the directory name minus its leading dash"
        );
        assert_eq!(
            ds.models.iter().map(|m| m.short.as_str()).collect::<Vec<_>>(),
            ["sonnet-5-5", "opus-5-5"]
        );
        assert_eq!(
            ds.sessions, 3,
            "s1 and s2 under app, s1 under lib; the same id in one project is one session"
        );
        assert_eq!(ds.rows.len(), 4);
        assert!(ds.rows.windows(2).all(|w| w[0].ts <= w[1].ts), "rows are time-sorted");
        assert!(!ds.looks_unparsed());
    }

    #[test]
    fn rows_are_priced_unless_the_model_is_unknown() {
        let mut files = BTreeMap::new();
        files.insert(
            "a".into(),
            file("-p", "s", None, "claude-sonnet-5-5", vec![rec(1, 10, 1_000_000)]),
        );
        files.insert(
            "b".into(),
            file("-p", "t", None, "mystery", vec![rec(2, 20, 1_000_000)]),
        );
        let ds = Dataset::build(&files);
        let (sonnet, mystery) = (&ds.rows[0], &ds.rows[1]);
        assert!(sonnet.priced);
        // 10 input tokens at $2/M plus 1M output at $10/M.
        assert!(
            (sonnet.cost - (10.0 * 2.0 + 1_000_000.0 * 10.0) / 1e6).abs() < 1e-9,
            "{}",
            sonnet.cost
        );
        assert!(!mystery.priced);
        assert_eq!(mystery.cost, 0.0);
    }

    #[test]
    fn unparsed_means_files_but_no_rows() {
        assert!(!Dataset::default().looks_unparsed(), "no files at all is just empty");
        let mut files = BTreeMap::new();
        files.insert("a".into(), file("-p", "s", None, "m", vec![]));
        assert!(Dataset::build(&files).looks_unparsed());
    }
}
