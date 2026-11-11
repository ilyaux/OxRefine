//! Long-running work with progress and cancel, like OpenRefine's get-processes / cancel-processes.
//! A host runs a command inside `run`; code deep inside (downloads, file reads, recipe steps) calls
//! `progress`/`tick` for the job on its thread, and stops with "Cancelled" once it is cancelled.
//! `list` and `cancel` are answered from any thread without the session.
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    io::Read,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
        Arc, Mutex,
    },
};

pub const CANCELLED: &str = "Cancelled";

struct Job {
    id: u64,
    label: Mutex<String>,
    done: AtomicU64,
    /// 0 while the size is unknown
    total: AtomicU64,
    cancel: AtomicBool,
}

static JOBS: Mutex<Vec<Arc<Job>>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);
thread_local!(static CURRENT: RefCell<Option<Arc<Job>>> = const { RefCell::new(None) });

fn jobs() -> std::sync::MutexGuard<'static, Vec<Arc<Job>>> {
    JOBS.lock().unwrap_or_else(|e| e.into_inner())
}

fn current<T>(f: impl FnOnce(&Job) -> T) -> Option<T> {
    CURRENT.with(|c| c.borrow().as_deref().map(f))
}

/// Run `work` as a job named `label`; it is listed until `work` returns.
pub fn run<T>(label: &str, work: impl FnOnce() -> T) -> T {
    let job = Arc::new(Job { id: NEXT.fetch_add(1, Relaxed), label: Mutex::new(label.into()), done: 0.into(), total: 0.into(), cancel: false.into() });
    jobs().push(job.clone());
    let outer = CURRENT.with(|c| c.replace(Some(job.clone())));
    // remove the job even if `work` panics
    struct Done(u64, Option<Arc<Job>>);
    impl Drop for Done {
        fn drop(&mut self) {
            CURRENT.with(|c| *c.borrow_mut() = self.1.take());
            jobs().retain(|j| j.id != self.0);
        }
    }
    let _done = Done(job.id, outer);
    work()
}

/// Start a new phase of the current job: what it does now, and how much there is (0 = unknown).
pub fn progress(label: &str, total: u64) {
    current(|j| {
        *j.label.lock().unwrap_or_else(|e| e.into_inner()) = label.into();
        j.total.store(total, Relaxed);
        j.done.store(0, Relaxed);
    });
}

/// Count `n` more units of work; Err once the job was cancelled.
pub fn tick(n: u64) -> crate::Result<()> {
    match current(|j| {
        j.done.fetch_add(n, Relaxed);
        j.cancel.load(Relaxed)
    }) {
        Some(true) => Err(CANCELLED.into()),
        _ => Ok(()),
    }
}

/// Running jobs: `[{id, label, done, total}]`.
pub fn list() -> Value {
    json!(jobs()
        .iter()
        .map(|j| json!({ "id": j.id, "label": *j.label.lock().unwrap_or_else(|e| e.into_inner()), "done": j.done.load(Relaxed), "total": j.total.load(Relaxed) }))
        .collect::<Vec<_>>())
}

/// Ask a job (or every job, for id 0) to stop; it ends at its next `tick`.
pub fn cancel(id: u64) {
    jobs().iter().filter(|j| id == 0 || j.id == id).for_each(|j| j.cancel.store(true, Relaxed));
}

/// Whether an I/O error is a cancelled job's.
pub fn is_cancel(e: &std::io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.to_string() == CANCELLED)
}

/// A reader that counts its bytes into the current job and stops when it is cancelled.
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

    #[test]
    fn progress_and_cancel() {
        run("outer", || {
            progress("reading", 10);
            tick(4).unwrap();
            let me = list().as_array().unwrap().iter().find(|j| j["label"] == "reading").cloned().unwrap();
            assert_eq!((me["done"].as_u64(), me["total"].as_u64()), (Some(4), Some(10)));
            cancel(me["id"].as_u64().unwrap());
            assert_eq!(tick(1).unwrap_err(), CANCELLED);
            let mut r = Counted(&b"abc"[..]);
            assert!(r.read(&mut [0u8; 8]).is_err(), "a cancelled job stops its reads");
        });
        // a cancelled import stops with "Cancelled", whichever reader it uses
        let csv = std::env::temp_dir().join("oxrefine-jobs-test.csv");
        std::fs::write(&csv, "a,b\n1,2\n").unwrap();
        for quotes in [true, false] {
            let o = crate::import::ImportOptions { process_quotes: quotes, ..Default::default() };
            let e = run("import", || {
                cancel(0);
                crate::import::import(&csv, None, &o).err()
            });
            assert!(e.is_some_and(|e| e.contains(CANCELLED)), "processQuotes={quotes}");
        }
        assert!(tick(1).is_ok(), "outside a job nothing is counted or cancelled");
        assert!(!list().as_array().unwrap().iter().any(|j| j["label"] == "reading"), "finished jobs are not listed");
    }
}
