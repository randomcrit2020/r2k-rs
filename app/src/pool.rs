//! A work pool with one worker thread per processor core: jobs are handed out one at a time from
//! a shared counter, so a long job doesn't hold up the others and the computer is never asked to
//! run more threads than it has cores (the load cases, the biaxial and two-direction shear
//! diagrams use it).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Threads of this program busy with analysis work right now (pool workers, and the extra mesh
/// runs of analysis::with_mesh_retry): nested work runs side by side only when cores are free.
pub static BUSY: AtomicUsize = AtomicUsize::new(0);

/// How many cores are free for more work.
pub fn spare() -> usize {
    workers().saturating_sub(BUSY.load(Ordering::Relaxed))
}

/// How many workers: the processor's cores (logical), at least 1.
pub fn workers() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).max(1)
}

/// Run `f` on every item with one worker per core. `order` is the order the jobs are handed out
/// (put the longest first so the last ones to finish are short); the results come back in the
/// items' own order. `progress` gets the share done after each job.
pub fn map<T: Sync, R: Send>(items: &[T], order: Option<&[usize]>, f: &(dyn Fn(&T) -> R + Sync), progress: &(dyn Fn(f32) + Sync)) -> Vec<R> {
    let n = items.len();
    if n == 0 {
        return vec![];
    }
    let seq: Vec<usize> = match order {
        Some(o) if o.len() == n => o.to_vec(),
        _ => (0..n).collect(),
    };
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<R>>> = Mutex::new((0..n).map(|_| None).collect());
    let w = workers().min(n);
    std::thread::scope(|s| {
        for _ in 0..w {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                if k >= n {
                    break;
                }
                let i = seq[k];
                BUSY.fetch_add(1, Ordering::Relaxed);
                let r = f(&items[i]);
                BUSY.fetch_sub(1, Ordering::Relaxed);
                if let Ok(mut v) = slots.lock() {
                    v[i] = Some(r);
                }
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress(d as f32 / n as f32);
            });
        }
    });
    slots.into_inner().unwrap_or_default().into_iter().map(|r| r.expect("every job ran")).collect()
}
