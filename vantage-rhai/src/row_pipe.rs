//! `RowPipe` — one long-lived script that rows stream through.
//!
//! The script runs on a thread of its own. It sets up once, then loops:
//!
//! ```rhai
//! let zones = [120, 140, 155];          // setup: runs once per (re)start
//! loop {
//!     let row = next_row();             // blocks until a row arrives; () on shutdown
//!     if row == () { break; }
//!     emit(#{ zone: zones.filter(|z| row.hr >= z).len() });
//! }
//! ```
//!
//! [`RowPipe::process`] sends a batch and blocks until every row is
//! answered, in order. A row the script moves past without `emit` answers
//! empty. A throw fails the current row and restarts the script from the
//! top. The operation allowance of the builder's [`crate::Limits`] applies
//! per row, not over the thread's lifetime.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use rhai::{Dynamic, EvalAltResult, Map};

use crate::{Block, Env, HostBuilder, Result};

/// One row's answer: the emitted map, or why it has none.
pub type RowAnswer = std::result::Result<Map, String>;

struct Job {
    rows: Vec<Map>,
    reply: mpsc::Sender<Vec<RowAnswer>>,
}

/// Handle to a running pipe. Dropping it ends the script: `next_row()`
/// returns `()`.
pub struct RowPipe {
    tx: mpsc::Sender<Job>,
}

/// Operation counts, for the per-row allowance.
struct Budget {
    last: AtomicU64,
    base: AtomicU64,
    per_row: u64,
}

/// The worker thread's side of the pipe, reached from the host functions.
struct Worker {
    rx: mpsc::Receiver<Job>,
    queue: VecDeque<Map>,
    answers: Vec<RowAnswer>,
    expected: usize,
    reply: Option<mpsc::Sender<Vec<RowAnswer>>>,
    /// A row was handed out and not yet answered.
    outstanding: bool,
    /// This run of the script has called `next_row()`.
    took_row: bool,
    /// The handle was dropped.
    closed: bool,
}

thread_local! {
    static WORKER: RefCell<Option<Worker>> = const { RefCell::new(None) };
}

impl Worker {
    fn answer(&mut self, answer: RowAnswer) {
        if self.outstanding {
            self.answers.push(answer);
            self.outstanding = false;
        }
    }

    /// Send the batch back once every row has its answer.
    fn flush(&mut self) {
        if self.answers.len() == self.expected
            && let Some(reply) = self.reply.take()
        {
            let _ = reply.send(std::mem::take(&mut self.answers));
        }
    }

    /// Answer the current row and every queued one with `msg`.
    fn fail_all(&mut self, msg: &str) {
        self.answer(Err(msg.to_string()));
        while self.queue.pop_front().is_some() {
            self.answers.push(Err(msg.to_string()));
        }
        self.flush();
    }

    fn take_job(&mut self, job: Job) {
        self.expected = job.rows.len();
        self.answers = Vec::with_capacity(self.expected);
        self.queue = job.rows.into();
        self.reply = Some(job.reply);
    }
}

fn next_row(budget: &Budget) -> Dynamic {
    WORKER.with(|w| {
        let mut w = w.borrow_mut();
        let w = w.as_mut().expect("next_row() outside a RowPipe");
        w.took_row = true;
        w.answer(Ok(Map::new()));
        loop {
            if let Some(row) = w.queue.pop_front() {
                w.outstanding = true;
                budget
                    .base
                    .store(budget.last.load(Ordering::Relaxed), Ordering::Relaxed);
                return Dynamic::from_map(row);
            }
            w.flush();
            match w.rx.recv() {
                Ok(job) => w.take_job(job),
                Err(_) => {
                    w.closed = true;
                    return Dynamic::UNIT;
                }
            }
        }
    })
}

fn emit(map: Map) -> std::result::Result<(), Box<EvalAltResult>> {
    WORKER.with(|w| {
        let mut w = w.borrow_mut();
        let w = w.as_mut().expect("emit() outside a RowPipe");
        if !w.outstanding {
            return Err("emit() without a row from next_row()".into());
        }
        w.answer(Ok(map));
        Ok(())
    })
}

impl RowPipe {
    /// Compile `script` on a host built from `builder` and start its thread.
    pub fn spawn(builder: HostBuilder, script: &str) -> Result<RowPipe> {
        let budget = Arc::new(Budget {
            last: AtomicU64::new(0),
            base: AtomicU64::new(0),
            per_row: builder.limits.max_operations(),
        });
        let (for_next, for_progress) = (budget.clone(), budget.clone());
        let host = builder
            .vocab_fn(move |engine| {
                // The allowance is per row: the lifetime limit is lifted and
                // `on_progress` measures from the last `next_row()`.
                engine.set_max_operations(0);
                engine.on_progress(move |ops| {
                    for_progress.last.store(ops, Ordering::Relaxed);
                    let spent = ops.saturating_sub(for_progress.base.load(Ordering::Relaxed));
                    (spent > for_progress.per_row)
                        .then(|| Dynamic::from("row exceeded its operation allowance"))
                });
                engine.register_fn("next_row", move || next_row(&for_next));
                engine.register_fn("emit", emit);
            })
            .build();
        let compiled = host.compile_uncached(&Block::from(script))?;

        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("rhai-row-pipe".into())
            .spawn(move || {
                WORKER.with(|w| {
                    *w.borrow_mut() = Some(Worker {
                        rx,
                        queue: VecDeque::new(),
                        answers: Vec::new(),
                        expected: 0,
                        reply: None,
                        outstanding: false,
                        took_row: false,
                        closed: false,
                    })
                });
                let env = Env::new();
                loop {
                    WORKER.with(|w| w.borrow_mut().as_mut().unwrap().took_row = false);
                    budget.base.store(0, Ordering::Relaxed);
                    budget.last.store(0, Ordering::Relaxed);
                    let result = compiled.run(&env);
                    let stop = WORKER.with(|w| {
                        let mut w = w.borrow_mut();
                        let w = w.as_mut().unwrap();
                        if w.closed {
                            return true;
                        }
                        match (&result, w.took_row) {
                            (Err(e), true) => w.answer(Err(e.to_string())),
                            (Ok(()), true) => w.answer(Ok(Map::new())),
                            // Never reached next_row(), so it never will:
                            // every batch gets an error until the handle drops.
                            (outcome, false) => {
                                let why = match outcome {
                                    Err(e) => format!("script failed before next_row(): {e}"),
                                    Ok(()) => "script ended without calling next_row()".to_string(),
                                };
                                loop {
                                    w.fail_all(&why);
                                    match w.rx.recv() {
                                        Ok(job) => w.take_job(job),
                                        Err(_) => return true,
                                    }
                                }
                            }
                        }
                        false
                    });
                    if stop {
                        break;
                    }
                }
            })
            .expect("spawn rhai-row-pipe thread");
        Ok(RowPipe { tx })
    }

    /// Send `rows` through the script and wait for their answers, in order.
    pub fn process(&self, rows: Vec<Map>) -> Vec<RowAnswer> {
        let n = rows.len();
        if n == 0 {
            return Vec::new();
        }
        let stopped = || vec![Err("row pipe stopped".to_string()); n];
        let (reply, answers) = mpsc::channel();
        if self.tx.send(Job { rows, reply }).is_err() {
            return stopped();
        }
        answers.recv().unwrap_or_else(|_| stopped())
    }
}
