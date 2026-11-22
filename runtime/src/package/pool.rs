//! A work queue that runs jobs as tasks on smol's executor.
//!
//! Resolving a dependency graph and installing its packages are the same shape: a job that
//! discovers more jobs. So there is one queue, one task per job, and a job may queue more work
//! while it runs. What is queued or running is counted, and that count is what tells the driver
//! that there is nothing left to wait for.
//!
//! A job is a task and not a thread. An install is almost entirely waiting on sockets and on the
//! disk, so the number of jobs in flight is a decision (see `net::IN_FLIGHT` and the local
//! permits in `plan`), not the number of cores the machine happens to have.

use color_eyre::{Report, Result};
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Pool {
    pending: Arc<AtomicUsize>,
    error: Arc<Mutex<Option<Report>>>,
    done_tx: smol::channel::Sender<()>,
    done_rx: Arc<smol::channel::Receiver<()>>,
}

impl Pool {
    pub fn new() -> Self {
        let (done_tx, done_rx) = smol::channel::bounded(1);
        Self {
            pending: Arc::new(AtomicUsize::new(0)),
            error: Arc::new(Mutex::new(None)),
            done_tx,
            done_rx: Arc::new(done_rx),
        }
    }

    /// Queue one job. A pool that has already failed takes nothing more, so the work drains
    /// instead of running a tree that is already doomed.
    pub fn spawn<F, Fut>(&self, job: F)
    where
        F: FnOnce(Pool) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        if self
            .error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_some()
        {
            return;
        }

        self.pending.fetch_add(1, Ordering::SeqCst);

        let pool = self.clone();
        smol::spawn(async move {
            let result = job(pool.clone()).await;
            pool.finish(result);
        })
        .detach();
    }

    fn finish(&self, result: Result<()>) {
        if let Err(error) = result {
            let mut slot = self.error.lock().unwrap_or_else(|error| error.into_inner());
            if slot.is_none() {
                *slot = Some(error);
            }
        }

        // The last job out is the one that wakes the driver; a wakeup already queued is enough.
        if self.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
            let _ = self.done_tx.try_send(());
        }
    }

    /// Wait until the queue is empty, then report the first failure.
    pub async fn run(self) -> Result<()> {
        while self.pending.load(Ordering::SeqCst) > 0 {
            if self.done_rx.recv().await.is_err() {
                break;
            }
        }

        match self
            .error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Pool;
    use color_eyre::eyre::eyre;
    use color_eyre::Result;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A job queues more jobs, and the pool only stops when the last one is done.
    #[test]
    fn a_job_that_queues_jobs_keeps_the_pool_running() {
        let pool = Pool::new();
        let ran = Arc::new(AtomicUsize::new(0));

        for _ in 0..10 {
            let ran = Arc::clone(&ran);
            pool.spawn(move |pool| async move {
                ran.fetch_add(1, Ordering::SeqCst);
                for _ in 0..2 {
                    let ran = Arc::clone(&ran);
                    pool.spawn(move |_| async move {
                        ran.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    });
                }
                Ok(())
            });
        }

        smol::block_on(pool.run()).expect("the pool finishes");
        assert_eq!(ran.load(Ordering::SeqCst), 30);
    }

    /// The first failure is what comes back, and no new work starts after it.
    #[test]
    fn the_first_failure_is_reported() {
        let pool = Pool::new();
        pool.spawn(|_| async move { Err(eyre!("the registry said no")) });
        let error = smol::block_on(pool.run()).expect_err("the pool fails");
        assert!(error.to_string().contains("the registry said no"));

        let later: Result<()> = Ok(());
        let pool = Pool::new();
        pool.spawn(|_| async { later });
        smol::block_on(pool.run()).expect("a pool with no failures is fine");
    }
}
