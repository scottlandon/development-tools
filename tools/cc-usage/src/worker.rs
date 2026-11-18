//! Background scanner thread. The UI never blocks on disk: it receives
//! immutable `Dataset` snapshots over a channel.

use crate::data::Dataset;
use crate::scan::Scanner;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::Duration;

pub enum Msg {
    Data(Arc<Dataset>),
    Scanning(bool),
}

pub struct Worker {
    pub rx: Receiver<Msg>,
    tx: Sender<()>,
}

impl Worker {
    pub fn spawn(roots: Vec<PathBuf>, refresh: Duration) -> Worker {
        let (mtx, rx) = mpsc::channel();
        let (tx, crx) = mpsc::channel();
        std::thread::spawn(move || run(roots, refresh, mtx, crx));
        Worker { rx, tx }
    }

    /// Ask for an immediate rescan.
    pub fn rescan(&self) {
        let _ = self.tx.send(());
    }
}

/// Scan, publish a snapshot if anything changed, then wait for the next tick or a rescan request.
/// Ends when the UI side hangs up (the process is exiting).
#[expect(
    clippy::needless_pass_by_value,
    reason = "the channel ends are owned by, and moved into, the worker thread"
)]
fn run(roots: Vec<PathBuf>, refresh: Duration, tx: Sender<Msg>, rx: Receiver<()>) {
    let mut sc = Scanner::new(roots);
    let mut first = true;
    loop {
        let _ = tx.send(Msg::Scanning(true));
        let changed = sc.scan();
        if (changed || first) && tx.send(Msg::Data(Arc::new(Dataset::build(&sc.files)))).is_err() {
            return;
        }
        first = false;
        let _ = tx.send(Msg::Scanning(false));
        match rx.recv_timeout(refresh) {
            Err(RecvTimeoutError::Disconnected) => return,
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
        }
        // The UI asks for a rescan every second; if a scan outlasts that, requests queue up.
        // Coalesce whatever is pending into the one scan we are about to do.
        loop {
            match rx.try_recv() {
                Ok(()) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    fn line(id: &str, out: u64) -> String {
        format!(
            r#"{{"timestamp":"2026-11-05T10:00:00Z","requestId":"r-{id}","message":{{"id":"{id}","model":"claude-sonnet-5-5","usage":{{"input_tokens":1,"output_tokens":{out}}}}}}}"#
        ) + "\n"
    }

    fn setup(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("cc-usage-worker-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("-proj")).unwrap();
        let file = dir.join("-proj/s.jsonl");
        std::fs::write(&file, line("m1", 5)).unwrap();
        (dir, file)
    }

    fn append(file: &Path, text: &str) {
        std::fs::OpenOptions::new()
            .append(true)
            .open(file)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    /// Messages of one scan cycle, as short tags, up to and including `Scanning(false)`.
    fn one_cycle(w: &Worker) -> (Vec<&'static str>, Option<Arc<Dataset>>) {
        let (mut tags, mut data) = (Vec::new(), None);
        loop {
            match w.rx.recv_timeout(Duration::from_secs(10)).expect("worker went quiet") {
                Msg::Scanning(true) => tags.push("scanning"),
                Msg::Data(d) => {
                    tags.push("data");
                    data = Some(d);
                }
                Msg::Scanning(false) => {
                    tags.push("idle");
                    return (tags, data);
                }
            }
        }
    }

    #[test]
    fn publishes_a_snapshot_on_the_first_scan_and_after_each_change() {
        let (dir, file) = setup("publish");
        // A long interval, so only `rescan()` triggers further scans.
        let w = Worker::spawn(vec![dir.clone()], Duration::from_secs(3600));

        let (tags, data) = one_cycle(&w);
        assert_eq!(tags, ["scanning", "data", "idle"]);
        assert_eq!(data.unwrap().rows.len(), 1);

        append(&file, &line("m2", 9));
        w.rescan();
        let (tags, data) = one_cycle(&w);
        assert_eq!(tags, ["scanning", "data", "idle"]);
        assert_eq!(data.unwrap().rows.len(), 2, "the appended message shows up");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rescan_with_nothing_new_sends_no_snapshot() {
        let (dir, _file) = setup("quiet");
        let w = Worker::spawn(vec![dir.clone()], Duration::from_secs(3600));
        one_cycle(&w);
        w.rescan();
        let (tags, data) = one_cycle(&w);
        assert_eq!(tags, ["scanning", "idle"], "no change, so no Data");
        assert!(data.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_burst_of_rescan_requests_causes_one_scan_not_one_per_request() {
        let (dir, _file) = setup("burst");
        // Many files, so a scan is not instantaneous. A long interval, so only `rescan()` triggers scans.
        for i in 0..100 {
            std::fs::write(dir.join(format!("-proj/extra-{i}.jsonl")), line(&format!("x{i}"), 1)).unwrap();
        }
        let w = Worker::spawn(vec![dir.clone()], Duration::from_secs(3600));
        one_cycle(&w);

        for _ in 0..20 {
            w.rescan();
        }
        // The burst must yield at least one more cycle (generous timeout)...
        one_cycle(&w);
        // ...then count any further ones until the worker has been quiet for a while.
        let mut extra = 0;
        while let Ok(msg) = w.rx.recv_timeout(Duration::from_millis(500)) {
            extra += usize::from(matches!(msg, Msg::Scanning(true)));
        }
        assert!(
            extra <= 2,
            "20 queued requests caused {} scans, expected about one",
            extra + 1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rescans_on_its_own_at_the_refresh_interval() {
        let (dir, file) = setup("tick");
        let w = Worker::spawn(vec![dir.clone()], Duration::from_millis(100));
        one_cycle(&w);
        append(&file, &line("m2", 9));
        // No rescan() call: the interval alone must pick the change up.
        let mut rows = 1;
        for _ in 0..50 {
            if let (_, Some(d)) = one_cycle(&w) {
                rows = d.rows.len();
                break;
            }
        }
        assert_eq!(rows, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
