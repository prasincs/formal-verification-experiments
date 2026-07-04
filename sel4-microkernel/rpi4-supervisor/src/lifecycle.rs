use core::fmt;
use core::sync::atomic::Ordering;

use sel4::UserContext;
use sel4_microkit::Child;

use crate::protocol::{WorkRing, COMMAND_NONE};

/// Microkit places the PD image at 0x200000. This is the same restart entry
/// used by the upstream Microkit 2.1 hierarchy example. The worker's first
/// instruction is the runtime entry stub, which re-establishes its stack and
/// calls `init` again.
pub const WORKER_ENTRY_POINT: usize = 0x20_0000;

#[derive(Clone, Copy, Debug)]
pub struct EndpointsStopped(());

impl EndpointsStopped {
    /// # Safety
    /// The child endpoint must be fault-stopped or explicitly suspended, and
    /// the caller must be the supervisor endpoint currently executing reset.
    pub unsafe fn new_unchecked() -> Self {
        Self(())
    }
}

#[derive(Debug)]
pub enum LifecycleError {
    OddGeneration(u32),
    Kernel(sel4::Error),
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OddGeneration(value) => write!(f, "ring reset already in progress ({value})"),
            Self::Kernel(error) => write!(f, "seL4 lifecycle invocation failed: {error:?}"),
        }
    }
}

impl From<sel4::Error> for LifecycleError {
    fn from(value: sel4::Error) -> Self {
        Self::Kernel(value)
    }
}

pub fn stop(child: Child) -> Result<EndpointsStopped, LifecycleError> {
    child.tcb().tcb_suspend()?;
    Ok(unsafe { EndpointsStopped::new_unchecked() })
}

/// Re-establish the ring invariant while both endpoints are quiescent.
pub fn quiescent_reset(
    ring: &WorkRing,
    _stopped: EndpointsStopped,
) -> Result<u32, LifecycleError> {
    let current = ring.generation.load(Ordering::Acquire);
    if current & 1 != 0 {
        return Err(LifecycleError::OddGeneration(current));
    }

    let odd = current.wrapping_add(1);
    let mut next_even = odd.wrapping_add(1);
    if next_even == 0 {
        next_even = 2;
    }

    ring.generation.store(odd, Ordering::Release);
    ring.write_idx.store(0, Ordering::Release);
    ring.read_idx.store(0, Ordering::Release);
    ring.heartbeat.store(0, Ordering::Release);
    ring.command.store(COMMAND_NONE, Ordering::Release);
    ring.command_sequence.store(0, Ordering::Release);
    ring.reserved.store(0, Ordering::Release);
    for entry in &ring.entries {
        entry.store(0, Ordering::Relaxed);
    }
    ring.generation.store(next_even, Ordering::Release);
    Ok(next_even)
}

pub fn restart(child: Child) -> Result<(), LifecycleError> {
    let mut context = UserContext::default();
    *context.pc_mut() = WORKER_ENTRY_POINT;
    child.tcb().tcb_write_registers(true, 1, &mut context)?;
    Ok(())
}

pub fn reset_and_restart(
    child: Child,
    ring: &WorkRing,
    stopped: EndpointsStopped,
) -> Result<u32, LifecycleError> {
    let generation = quiescent_reset(ring, stopped)?;
    restart(child)?;
    Ok(generation)
}
