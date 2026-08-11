//! Unix detached-thread adapter.

use crate::threading::ThreadTask;
use std::{io, thread};

#[inline(never)]
pub(crate) fn spawn_named_detached(name: &'static str, task: ThreadTask) -> io::Result<()> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || task())
        .map(drop)
}

#[cfg(test)]
pub(crate) fn current_name() -> Option<String> {
    thread::current().name().map(str::to_owned)
}
