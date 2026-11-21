//! A work queue that runs jobs on a fixed set of threads.
//!
//! Resolving a dependency graph and installing its packages are the same shape: a job that
//! discovers more jobs. So there is one queue, some number of threads draining it, and a job
//! may push onto it while it runs. The queue counts what is still queued or running, which is
//! what tells the threads when there is nothing left to wait for.

use color_eyre::{Report, Result};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    thread,
};

type Job = Box<dyn FnOnce(&Pool) -> Result<()> + Send + 'static>;

struct State {
    queue: VecDeque<Job>,
    pending: usize,
    error: Option<Report>,
}

pub struct Pool {
    state: Arc<(Mutex<State>, Condvar)>,
    threads: usize,
}

impl Pool {
    pub fn new(threads: usize) -> Self {
        Self {
            state: Arc::new((
                Mutex::new(State {
                    queue: VecDeque::new(),
                    pending: 0,
                    error: None,
                }),
                Condvar::new(),
            )),
            threads: threads.max(1),
        }
    }

    /// How many jobs a network-bound pool runs at once: enough to keep a link busy, few enough
    /// that a large tree does not become a thousand threads.
    pub fn workers() -> usize {
        std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(4)
            .clamp(4, 16)
    }

    /// Queue one job. A pool that has already failed takes nothing more.
    pub fn spawn<F>(&self, job: F)
    where
        F: FnOnce(&Pool) -> Result<()> + Send + 'static,
    {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
        if state.error.is_some() {
            return;
        }
        state.pending += 1;
        state.queue.push_back(Box::new(job));
        wake.notify_one();
    }

    /// Run until the queue is empty, then report the first failure.
    pub fn run(self) -> Result<()> {
        let threads = self.threads;
        thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| self.work());
            }
        });
        let (lock, _) = &*self.state;
        match lock
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .error
            .take()
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn work(&self) {
        let (lock, wake) = &*self.state;
        loop {
            let job = {
                let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                loop {
                    if let Some(job) = state.queue.pop_front() {
                        break Some(job);
                    }
                    // Nothing queued and nothing running: this pool is done. A failed pool
                    // drains instead of waiting, because no thread will ever queue more.
                    if state.pending == 0 || state.error.is_some() {
                        break None;
                    }
                    state = wake.wait(state).unwrap_or_else(|error| error.into_inner());
                }
            };

            let Some(job) = job else { return };
            let result = job(self);

            let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
            state.pending -= 1;
            if let Err(error) = result {
                if state.error.is_none() {
                    state.error = Some(error);
                }
                state.queue.clear();
                state.pending = 0;
            }
            drop(state);
            wake.notify_all();
        }
    }
}
