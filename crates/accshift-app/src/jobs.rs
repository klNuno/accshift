//! Backend calls off the UI thread.
//!
//! A small fixed pool of threads runs closures against the shared backend.
//! Each call hands back a [`Pending`] the UI polls once per frame; when a
//! result lands, the pool calls the wake callback so an idle UI repaints and
//! picks it up (the egui shell passes `Context::request_repaint`). No async
//! runtime: every backend method blocks, so threads are the whole story.

use crate::backend::Backend;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

type Task = Box<dyn FnOnce(&dyn Backend) + Send + 'static>;
type Wake = Arc<dyn Fn() + Send + Sync + 'static>;

/// The pool. Dropping it lets queued jobs finish, then joins the threads.
pub struct Jobs {
    backend: Arc<dyn Backend>,
    queue: Option<Sender<Task>>,
    workers: Vec<JoinHandle<()>>,
    wake: Wake,
}

impl Jobs {
    /// `threads` workers (at least one) serving `backend`. `wake` runs on a
    /// worker thread after each result is delivered.
    pub fn new(
        backend: Arc<dyn Backend>,
        threads: usize,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (queue, tasks) = mpsc::channel::<Task>();
        let tasks = Arc::new(Mutex::new(tasks));
        let workers = (0..threads.max(1))
            .map(|index| {
                let tasks = Arc::clone(&tasks);
                let backend = Arc::clone(&backend);
                std::thread::Builder::new()
                    .name(format!("accshift-job-{index}"))
                    .spawn(move || loop {
                        // The lock is held only while taking a task, never
                        // while running it.
                        let next = tasks.lock().map(|rx| rx.recv());
                        match next {
                            // A panicking call drops its sender, so its
                            // Pending reads Gone; the worker lives on.
                            Ok(Ok(task)) => {
                                let _ =
                                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                        task(backend.as_ref())
                                    }));
                            }
                            _ => break,
                        }
                    })
                    .expect("spawn job worker")
            })
            .collect();
        Self {
            backend,
            queue: Some(queue),
            workers,
            wake: Arc::new(wake),
        }
    }

    /// The backend the pool serves, for a synchronous call where blocking is
    /// fine (tests, a CLI, shutdown).
    pub fn backend(&self) -> &Arc<dyn Backend> {
        &self.backend
    }

    /// Queues `call` and returns its pending result.
    pub fn run<T, F>(&self, call: F) -> Pending<T>
    where
        T: Send + 'static,
        F: FnOnce(&dyn Backend) -> T + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let wake = Arc::clone(&self.wake);
        let task: Task = Box::new(move |backend| {
            // A dropped Pending means nobody waits for this result any more.
            if tx.send(call(backend)).is_ok() {
                wake();
            }
        });
        if let Some(queue) = &self.queue {
            // Workers only stop once the queue is dropped, in Drop.
            let _ = queue.send(task);
        }
        Pending { rx, done: false }
    }
}

impl Drop for Jobs {
    fn drop(&mut self) {
        self.queue = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// What a polled [`Pending`] holds.
#[derive(Debug, PartialEq, Eq)]
pub enum Poll<T> {
    /// Still running.
    Waiting,
    /// The result, handed out once.
    Ready(T),
    /// Already taken, or the worker died before answering (the call panicked).
    Gone,
}

/// A backend call in flight. Poll it from the UI thread.
#[derive(Debug)]
pub struct Pending<T> {
    rx: Receiver<T>,
    done: bool,
}

impl<T> Pending<T> {
    /// Non-blocking: the result once, then [`Poll::Gone`].
    pub fn poll(&mut self) -> Poll<T> {
        if self.done {
            return Poll::Gone;
        }
        match self.rx.try_recv() {
            Ok(value) => {
                self.done = true;
                Poll::Ready(value)
            }
            Err(TryRecvError::Empty) => Poll::Waiting,
            Err(TryRecvError::Disconnected) => {
                self.done = true;
                Poll::Gone
            }
        }
    }

    /// Blocks until the result arrives. `None` if the call panicked.
    pub fn wait(self) -> Option<T> {
        if self.done {
            return None;
        }
        self.rx.recv().ok()
    }
}
