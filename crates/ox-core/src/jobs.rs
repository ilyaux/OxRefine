//! Thread-local work stages, progress and cancellation, shared by desktop and HTTP hosts.
//! Completed jobs stay available briefly so a client can collect the final stage and outcome.
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    io::Read,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const CANCELLED: &str = "Cancelled";
const KEEP: Duration = Duration::from_secs(60);
const FINISHED_LIMIT: usize = 20;

struct Stage {
    label: String,
    unit: &'static str,
    started: Instant,
    elapsed: Option<Duration>,
    state: &'static str,
    done: u64,
    total: u64,
}

struct State {
    stages: Vec<Stage>,
    state: &'static str,
    finished: Option<Instant>,
}

struct Job {
    id: u64,
    owner: Option<String>,
    started: Instant,
    state: Mutex<State>,
    done: AtomicU64,
    /// 0 while the size is unknown
    total: AtomicU64,
    cancel: AtomicBool,
}

impl Job {
    fn finish(&self, outcome: &'static str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finished.is_some() {
            return;
        }
        let now = Instant::now();
        let stage = state.stages.last_mut().unwrap();
        if stage.elapsed.is_none() {
            stage.done = self.done.load(Relaxed);
            stage.total = self.total.load(Relaxed);
            stage.elapsed = Some(now.duration_since(stage.started));
            stage.state = outcome;
        }
        state.state = outcome;
        state.finished = Some(now);
    }

    fn snapshot(&self) -> Value {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let stages: Vec<_> = state.stages.iter().map(|s| {
            let active = s.elapsed.is_none();
            json!({ "label": s.label, "unit": s.unit, "state": s.state,
                "elapsedMs": s.elapsed.unwrap_or_else(|| s.started.elapsed()).as_secs_f64() * 1000.0,
                "done": if active { self.done.load(Relaxed) } else { s.done },
                "total": if active { self.total.load(Relaxed) } else { s.total } })
        }).collect();
        let last = stages.last().unwrap();
        json!({ "id": self.id, "label": last["label"], "unit": last["unit"],
            "done": last["done"], "total": last["total"], "state": state.state,
            "elapsedMs": state.finished.unwrap_or_else(Instant::now).duration_since(self.started).as_secs_f64() * 1000.0,
            "cancelRequested": self.cancel.load(Relaxed), "stages": stages })
    }
}

static JOBS: Mutex<Vec<Arc<Job>>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);
thread_local!(static CURRENT: RefCell<Option<Arc<Job>>> = const { RefCell::new(None) });
thread_local!(static OWNER: RefCell<Option<String>> = const { RefCell::new(None) });

/// Associate this request's jobs with its browser page, without locking the project during polling.
pub fn with_owner<T>(owner: Option<&str>, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore { fn drop(&mut self) { OWNER.with(|o| *o.borrow_mut() = self.0.take()); } }
    let _restore = Restore(OWNER.with(|o| o.replace(owner.map(str::to_owned))));
    work()
}

fn jobs() -> std::sync::MutexGuard<'static, Vec<Arc<Job>>> {
    JOBS.lock().unwrap_or_else(|e| e.into_inner())
}

fn prune(jobs: &mut Vec<Arc<Job>>) {
    let mut kept = 0;
    // Keep running work and the most recent bounded set of finished jobs.
    for i in (0..jobs.len()).rev() {
        let expired = {
            let state = jobs[i].state.lock().unwrap_or_else(|e| e.into_inner());
            state.finished.is_some_and(|t| { kept += 1; t.elapsed() > KEEP || kept > FINISHED_LIMIT })
        };
        if expired {
            jobs.remove(i);
        }
    }
}

fn current<T>(f: impl FnOnce(&Job) -> T) -> Option<T> {
    CURRENT.with(|c| c.borrow().as_deref().map(f))
}

/// Run work as a job. Call `run_result` when its return value represents success or failure.
pub fn run<T>(label: &str, work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let job = Arc::new(Job {
        id: NEXT.fetch_add(1, Relaxed), owner: OWNER.with(|o| o.borrow().clone()), started,
        state: Mutex::new(State { stages: vec![Stage { label: label.into(), unit: "items", started, elapsed: None, state: "running", done: 0, total: 0 }], state: "running", finished: None }),
        done: 0.into(), total: 0.into(), cancel: false.into(),
    });
    {
        let mut list = jobs();
        prune(&mut list);
        list.push(job.clone());
    }
    let outer = CURRENT.with(|c| c.replace(Some(job.clone())));
    struct Done(Arc<Job>, Option<Arc<Job>>);
    impl Drop for Done {
        fn drop(&mut self) {
            self.0.finish(if std::thread::panicking() { "failed" } else { "completed" });
            CURRENT.with(|c| *c.borrow_mut() = self.1.take());
            prune(&mut jobs());
        }
    }
    let _done = Done(job, outer);
    work()
}

/// Preserve a command's error/cancellation outcome instead of reporting successful completion.
pub fn run_result<T>(label: &str, work: impl FnOnce() -> crate::Result<T>) -> crate::Result<T> {
    run(label, || {
        let result = work();
        current(|j| j.finish(outcome(&result)));
        result
    })
}

fn outcome<T>(result: &crate::Result<T>) -> &'static str {
    match result {
        Ok(_) => "completed",
        Err(e) if is_cancelled(e) => "cancelled",
        Err(_) => "failed",
    }
}

/// Cancellation may include a recipe step and an explanation of already-applied changes.
pub fn is_cancelled(error: &str) -> bool {
    let reason = error.split_once(';').map_or(error, |(reason, _)| reason);
    reason == CANCELLED || reason.ends_with(": Cancelled")
}

/// Close a phase before subsequent cleanup/save work, preserving its actual outcome.
pub fn finish_stage<T>(result: &crate::Result<T>) {
    current(|j| {
        let mut state = j.state.lock().unwrap_or_else(|e| e.into_inner());
        let stage = state.stages.last_mut().unwrap();
        if stage.elapsed.is_none() {
            stage.done = j.done.load(Relaxed);
            stage.total = j.total.load(Relaxed);
            stage.elapsed = Some(stage.started.elapsed());
            stage.state = outcome(result);
        }
    });
}

/// Start a phase with a count of items (0 = unknown). Byte readers use `progress_units`.
pub fn progress(label: &str, total: u64) {
    progress_units(label, total, "items");
}

/// Start an actual work phase, keeping previous stages and their elapsed time.
pub fn progress_units(label: &str, total: u64, unit: &'static str) {
    current(|j| {
        let mut state = j.state.lock().unwrap_or_else(|e| e.into_inner());
        let previous = state.stages.last_mut().unwrap();
        if previous.label != label || previous.unit != unit || previous.elapsed.is_some() {
            let now = Instant::now();
            if previous.elapsed.is_none() {
                previous.done = j.done.load(Relaxed);
                previous.total = j.total.load(Relaxed);
                previous.elapsed = Some(now.duration_since(previous.started));
                previous.state = "completed";
            }
            // "Working" is the dispatch placeholder, not a measurable import stage.
            if state.stages.len() == 1 && state.stages[0].label == "Working" {
                state.stages.clear();
            }
            state.stages.push(Stage { label: label.into(), unit, started: now, elapsed: None, state: "running", done: 0, total });
        }
        j.total.store(total, Relaxed);
        j.done.store(0, Relaxed);
    });
}

/// Count `n` more units; Err once the job was cancelled.
pub fn tick(n: u64) -> crate::Result<()> {
    match current(|j| {
        j.done.fetch_add(n, Relaxed);
        j.cancel.load(Relaxed)
    }) {
        Some(true) => Err(CANCELLED.into()),
        _ => Ok(()),
    }
}

/// Running and recent finished jobs, including stage counts, outcomes and elapsed milliseconds.
pub fn list() -> Value {
    let mut list = jobs();
    prune(&mut list);
    json!(list.iter().map(|j| j.snapshot()).collect::<Vec<_>>())
}

pub fn list_owned(owner: &str) -> Value {
    let mut list = jobs();
    prune(&mut list);
    json!(list.iter().filter(|j| j.owner.as_deref() == Some(owner)).map(|j| j.snapshot()).collect::<Vec<_>>())
}

/// A scoped cancel never cancels another page's work, including the all-jobs shorthand.
pub fn cancel_owned(owner: &str, id: u64) -> bool {
    let list = jobs();
    let found = id == 0 || list.iter().any(|j| j.id == id && j.owner.as_deref() == Some(owner));
    list.iter().filter(|j| j.owner.as_deref() == Some(owner) && (id == 0 || j.id == id)).for_each(|j| {
        if j.state.lock().unwrap_or_else(|e| e.into_inner()).finished.is_none() { j.cancel.store(true, Relaxed); }
    });
    found
}

/// Ask a running job (or every job for id 0) to stop at its next cancellation check.
pub fn cancel(id: u64) {
    jobs().iter().filter(|j| id == 0 || j.id == id).for_each(|j| {
        if j.state.lock().unwrap_or_else(|e| e.into_inner()).finished.is_none() {
            j.cancel.store(true, Relaxed);
        }
    });
}

pub fn is_cancel(e: &std::io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.to_string() == CANCELLED)
}

/// A reader that counts bytes and checks cancellation.
pub struct Counted<R>(pub R);

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.0.read(buf)?;
        // not ErrorKind::Interrupted: std readers retry that forever
        tick(n as u64).map_err(std::io::Error::other)?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by_id(id: u64) -> Value {
        list().as_array().unwrap().iter().find(|j| j["id"] == id).cloned().unwrap()
    }

    #[test]
    fn stage_outcomes_and_cancel() {
        let mut id = 0;
        let result: crate::Result<()> = run_result("Working", || {
            id = current(|j| j.id).unwrap();
            progress_units("Reading", 10, "bytes");
            tick(4)?;
            progress("Parsing", 2);
            tick(1)?;
            let me = by_id(id);
            assert_eq!(me["stages"][0]["state"], "completed");
            assert_eq!(me["stages"][0]["done"], 4);
            assert_eq!(me["stages"][0]["unit"], "bytes");
            assert_eq!(me["stages"][1]["state"], "running");
            cancel(id);
            assert_eq!(by_id(id)["cancelRequested"], true);
            let mut reader = Counted(&b"abc"[..]);
            assert!(reader.read(&mut [0; 8]).is_err());
            tick(0)
        });
        assert_eq!(result.unwrap_err(), CANCELLED);
        let me = by_id(id);
        assert_eq!(me["state"], "cancelled");
        assert_eq!(me["stages"][1]["state"], "cancelled");
        assert!(me["elapsedMs"].as_f64().unwrap() >= me["stages"][0]["elapsedMs"].as_f64().unwrap());
        assert!(tick(1).is_ok(), "outside a job no count or cancellation is inherited");

        let mut failed_id = 0;
        let _: crate::Result<()> = run_result("Saving", || {
            failed_id = current(|j| j.id).unwrap();
            Err("disk full".into())
        });
        assert_eq!(by_id(failed_id)["state"], "failed");
        assert_eq!(by_id(failed_id)["stages"][0]["state"], "failed");

        let mut cleanup_id = 0;
        let _: crate::Result<()> = run_result("Working", || {
            cleanup_id = current(|j| j.id).unwrap();
            progress("Applying recipe", 2);
            let result: crate::Result<()> = Err("step 2: missing column".into());
            finish_stage(&result);
            progress("Saving the project", 0);
            finish_stage(&Ok(()));
            result
        });
        let me = by_id(cleanup_id);
        assert_eq!(me["state"], "failed");
        assert_eq!(me["stages"][0]["state"], "failed");
        assert_eq!(me["stages"][1]["state"], "completed", "successful cleanup is not a failed save");
    }

    #[test]
    fn import_stages_and_cancelled_reader() {
        let csv = std::env::temp_dir().join(format!("oxrefine-jobs-{}.csv", std::process::id()));
        std::fs::write(&csv, "a,b\n1,2\n").unwrap();
        for quotes in [true, false] {
            let o = crate::import::ImportOptions { process_quotes: quotes, guess_cell_value_types: true, ..Default::default() };
            let result = run_result("Working", || {
                cancel(current(|j| j.id).unwrap());
                crate::import::import(&csv, None, &o)
            });
            assert!(result.err().is_some_and(|e| e.contains(CANCELLED)), "processQuotes={quotes}");
        }
        let mut id = 0;
        run_result("Working", || {
            id = current(|j| j.id).unwrap();
            crate::import::import(&csv, None, &Default::default())
        }).unwrap();
        let me = by_id(id);
        assert_eq!(me["state"], "completed");
        assert!(me["stages"].as_array().unwrap().iter().all(|s| s["state"] == "completed"));
        assert!(me["stages"].as_array().unwrap().iter().any(|s| s["label"].as_str().unwrap().to_ascii_lowercase().contains("parsing")));
        std::fs::remove_file(csv).unwrap();
    }

    #[test]
    fn indexed_cancellation_keeps_its_outcome() {
        assert!(is_cancelled("Cancelled"));
        assert!(is_cancelled("step 3: Cancelled; 2 earlier steps remain applied"));
        assert!(!is_cancelled("missing column Cancelled"));
        assert!(!is_cancelled("step 3: missing column; input named Cancelled"));
    }
}
