//! The page's explicit handle to the booted kernel host.

use std::rc::Rc;

use dioxus::prelude::*;
use ksee_host::machine::Machine;

#[derive(Clone, Copy)]
pub struct Host {
    machine: Signal<Option<Rc<Machine>>>,
}

impl Host {
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
    consume_context::<Host>().machine().expect("booted")
}
