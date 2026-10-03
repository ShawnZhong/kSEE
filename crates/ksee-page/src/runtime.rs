//! The page's explicit handle to the booted runtime Machine.

use std::rc::Rc;

use dioxus::prelude::*;
use ksee_runtime::machine::Machine;

#[derive(Clone, Copy)]
pub struct Runtime {
    machine: Signal<Option<Rc<Machine>>>,
}

impl Runtime {
    pub fn new() -> Self {
        Self { machine: Signal::new(None) }
    }

    pub fn install(mut self, machine: Rc<Machine>) {
        self.machine.set(Some(machine));
    }

    pub fn machine(&self) -> Option<Rc<Machine>> {
        self.machine.peek().clone()
    }
}

/// The booted Machine. Components and scope-owned tasks call this only after
/// the kernel status is ready.
pub fn machine() -> Rc<Machine> {
    consume_context::<Runtime>().machine().expect("booted")
}
