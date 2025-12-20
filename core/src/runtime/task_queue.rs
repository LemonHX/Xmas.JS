//! Task queue for spawned futures

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use std::{boxed::Box, vec::Vec};

use crossbeam_deque::Injector;
use parking_lot::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPoll {
    Empty,
    Pending,
    Progress,
    Done,
}

type BoxedTask = Pin<Box<dyn Future<Output = ()>>>;

pub struct TaskQueue {
    inner: TaskQueueInner,
}

struct TaskQueueInner {
    tasks: crossbeam_deque::Injector<BoxedTask>,
    waker: Mutex<Option<Waker>>,
}

impl TaskQueue {
    pub fn new() -> Self {
        TaskQueue {
            inner: TaskQueueInner {
                tasks: Injector::new(),
                waker: Mutex::new(None),
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.inner.tasks.is_empty()
    }

    /// # Safety
    /// Caller must ensure future lifetime is valid
    pub unsafe fn push<F: Future<Output = ()>>(&self, future: F) {
        let future: BoxedTask =
            core::mem::transmute(Box::pin(future) as Pin<Box<dyn Future<Output = ()> + '_>>);
        // let mut inner = self.inner.lock();
        self.inner.tasks.push(future);
        if let Some(mut w) = self.inner.waker.try_lock().take() {
            if let Some(waker) = w.take() {
                waker.wake();
            }
        }
    }

    pub fn listen(&self, waker: Waker) {
        *self.inner.waker.lock() = Some(waker);
    }

    /// Poll tasks - optimized to minimize lock contention
    pub fn poll(&self, cx: &mut Context) -> TaskPoll {
        // Take all tasks out in one lock acquisition
        if self.inner.tasks.is_empty() {
            return TaskPoll::Empty;
        }

        let w = crossbeam_deque::Worker::new_fifo();
        let mut steal = self.inner.tasks.steal_batch(&w);
        while let crossbeam_deque::Steal::Retry = steal {
            steal = self.inner.tasks.steal_batch(&w);
        }
        match steal {
            crossbeam_deque::Steal::Empty => {
                // Check if new tasks were spawned during polling
                let has_tasks = !self.inner.tasks.is_empty();
                if !has_tasks {
                    TaskPoll::Empty
                } else {
                    TaskPoll::Pending
                }
            }
            crossbeam_deque::Steal::Success(_) => {
                let mut made_progress = false;
                let mut pending = Vec::new();

                // Poll all tasks without holding the lock
                while let Some(mut task) = w.pop() {
                    match task.as_mut().poll(cx) {
                        Poll::Ready(()) => made_progress = true,
                        Poll::Pending => pending.push(task),
                    }
                }

                // Put pending tasks back in one lock acquisition
                for task in pending {
                    self.inner.tasks.push(task);
                }

                // Check if new tasks were spawned during polling
                let has_tasks = !self.inner.tasks.is_empty();

                if !has_tasks {
                    if made_progress {
                        TaskPoll::Done
                    } else {
                        TaskPoll::Empty
                    }
                } else if made_progress {
                    TaskPoll::Progress
                } else {
                    TaskPoll::Pending
                }
            }
            _ => unreachable!(),
        }
    }
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for TaskQueue {}
unsafe impl Sync for TaskQueue {}
