//! What a shell's `rebind` joined. [`Transaction::wrap`] runs the rebind
//! inside [`joining`], and the participants it joins
//! ([`Transaction::participant`]) and a [`direct`](super::direct) shell it
//! makes are noted on this thread meanwhile. `rebind` is synchronous, so
//! they are all made on the thread that called it.

use std::cell::RefCell;
use std::sync::Arc;

use super::{Transaction, TxKey};

/// The participants one rebind joined, or that it made a direct shell.
#[derive(Clone, Default)]
pub(crate) struct Joined {
    pub(crate) keys: Vec<TxKey>,
    pub(crate) direct: bool,
}

struct Frame {
    tx: usize,
    joined: Joined,
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

/// Pops its frame when dropped, also on a panic inside the rebind.
struct Pop;

impl Drop for Pop {
    fn drop(&mut self) {
        FRAMES.with(|frames| frames.borrow_mut().pop());
    }
}

fn id(tx: &Transaction) -> usize {
    Arc::as_ptr(&tx.0) as usize
}

/// `rebind()`'s result, and what it joined of `tx`.
pub(super) fn joining<T>(tx: &Transaction, rebind: impl FnOnce() -> T) -> (T, Joined) {
    FRAMES.with(|frames| {
        frames.borrow_mut().push(Frame {
            tx: id(tx),
            joined: Joined::default(),
        })
    });
    let pop = Pop;
    let out = rebind();
    let joined = FRAMES.with(|frames| {
        frames
            .borrow_mut()
            .last_mut()
            .map(|frame| std::mem::take(&mut frame.joined))
            .unwrap_or_default()
    });
    drop(pop);
    (out, joined)
}

pub(super) fn note_key(tx: &Transaction, key: TxKey) {
    note(tx, |joined| {
        if !joined.keys.contains(&key) {
            joined.keys.push(key);
        }
    });
}

/// A rebind that returns a shell already in `tx` hands on what that shell
/// joined.
pub(super) fn note_joined(tx: &Transaction, from: &Joined) {
    note(tx, |joined| {
        for key in &from.keys {
            if !joined.keys.contains(key) {
                joined.keys.push(*key);
            }
        }
        joined.direct |= from.direct;
    });
}

/// A direct shell made inside a rebind marks that rebind direct.
pub(super) fn note_direct() {
    FRAMES.with(|frames| {
        if let Some(frame) = frames.borrow_mut().last_mut() {
            frame.joined.direct = true;
        }
    });
}

fn note(tx: &Transaction, change: impl FnOnce(&mut Joined)) {
    let tx = id(tx);
    FRAMES.with(|frames| {
        let mut frames = frames.borrow_mut();
        if let Some(frame) = frames.iter_mut().rev().find(|frame| frame.tx == tx) {
            change(&mut frame.joined);
        }
    });
}
