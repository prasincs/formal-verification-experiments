use core::fmt;
use core::sync::atomic::Ordering;

use sel4::UserContext;
use sel4_microkit::Child;

use crate::build_constants::WORKER_RESTART_ENTRY;
use crate::protocol::WorkRing;

#[derive(Debug)]
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
    MissingBuildRestartEntry,
    Kernel(sel4::Error),
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OddGeneration(value) => write!(f, "ring reset already in progress ({value})"),
            Self::MissingBuildRestartEntry => {
                write!(f, "trusted build did not supply the worker restart entry")
            }
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

/// Re-establish IC-1 while the child endpoint is quiescent. The lifecycle PD is
/// the other endpoint and is executing this function, so it cannot publish.
pub fn quiescent_reset(
    ring: &WorkRing,
    stopped: EndpointsStopped,
) -> Result<u32, LifecycleError> {
    let current = ring.generation.load(Ordering::Acquire);
    if current & 1 != 0 {
        return Err(LifecycleError::OddGeneration(current));
    }

    let odd = current.wrapping_add(1);
    let next_even = if current >= u32::MAX - 1 {
        2
    } else {
        current + 2
    };

    let _consumed = stopped;
    ring.generation.store(odd, Ordering::Release);
    ring.write_idx.store(0, Ordering::Release);
    ring.read_idx.store(0, Ordering::Release);
    for entry in &ring.entries {
        entry.store(0, Ordering::Relaxed);
    }
    ring.generation.store(next_even, Ordering::Release);
    Ok(next_even)
}

pub fn restart(child: Child) -> Result<(), LifecycleError> {
    if WORKER_RESTART_ENTRY == 0 {
        return Err(LifecycleError::MissingBuildRestartEntry);
    }
    let mut context = UserContext::default();
    *context.pc_mut() = WORKER_RESTART_ENTRY;
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
