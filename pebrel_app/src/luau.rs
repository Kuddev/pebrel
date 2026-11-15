//! Shared VM limits; callers own their VM, data conversion and host capabilities.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use mlua::{Compiler, Lua, LuaOptions, StdLib, VmState};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Limit {
    Interrupts,
    Deadline,
}

impl fmt::Display for Limit {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(match self {
            Self::Interrupts => "Luau interrupt budget exhausted",
            Self::Deadline => "Luau execution deadline reached",
        })
    }
}

impl std::error::Error for Limit {}

#[derive(Clone)]
pub(crate) struct Budget {
    deadline: Instant,
    max_interrupts: usize,
    interrupts: Arc<AtomicUsize>,
}

impl Budget {
    pub(crate) fn new(timeout: Duration, max_interrupts: usize) -> Self {
        Self {
            deadline: Instant::now() + timeout,
            max_interrupts,
            interrupts: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(crate) fn vm(&self, libraries: StdLib, memory_bytes: usize) -> mlua::Result<Lua> {
        // A quota stop must also escape pcall/xpcall and non-yieldable metamethods.
        // mlua transports Rust unwinds through the VM; run() catches only our
        // private Limit payload. resume_unwind does not invoke the panic hook.
        let lua = Lua::new_with(libraries, LuaOptions::default().catch_rust_panics(false))?;
        lua.set_memory_limit(memory_bytes)?;
        lua.set_compiler(Compiler::new().set_optimization_level(2));
        let budget = self.clone();
        lua.set_interrupt(move |_| {
            budget.interrupts.fetch_add(1, Ordering::Relaxed);
            if let Err(limit) = budget.remaining() {
                resume_unwind(Box::new(limit));
            }
            Ok(VmState::Continue)
        });
        Ok(lua)
    }

    pub(crate) fn interrupt_count(&self) -> usize {
        self.interrupts.load(Ordering::Relaxed)
    }

    pub(crate) fn remaining(&self) -> Result<Duration, Limit> {
        if self.interrupt_count() >= self.max_interrupts {
            return Err(Limit::Interrupts);
        }
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(Limit::Deadline)
    }

    /// Every entry from Rust into budgeted user code stays inside this boundary.
    pub(crate) fn run<T>(&self, call: impl FnOnce() -> mlua::Result<T>) -> mlua::Result<T> {
        self.remaining().map_err(mlua::Error::external)?;
        let result = match catch_unwind(AssertUnwindSafe(call)) {
            Ok(result) => result,
            Err(payload) => match payload.downcast::<Limit>() {
                Ok(limit) => Err(mlua::Error::external(*limit)),
                Err(payload) => resume_unwind(payload),
            },
        };
        self.remaining().map_err(mlua::Error::external)?;
        result
    }
}
