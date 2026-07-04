#![no_std]
#![no_main]

use core::fmt;

use rpi4_supervisor::installer::InstallerStub;
use rpi4_supervisor::lifecycle::{self, EndpointsStopped};
use rpi4_supervisor::protocol::{
    WorkRing, COMMAND_POISON, COMMAND_WATCHDOG_EXPIRE, COMMAND_WATCHDOG_STALL,
    SUPERVISOR_CHANNEL_ID,
};
use rpi4_supervisor::verifier::VerifierStub;
use sel4_microkit::{
    debug_println, protection_domain, Channel, ChannelSet, Child, Handler, MessageInfo,
};

const WORKER_CHANNEL: Channel = Channel::new(SUPERVISOR_CHANNEL_ID);
const WORKER_CHILD_ID: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    AwaitBoot1,
    AwaitPoisonFault,
    AwaitBoot2,
    AwaitWatchdogDeadline,
    AwaitWatchdogFault,
    AwaitBoot3,
    Complete,
}

struct Supervisor {
    ring: &'static WorkRing,
    stage: Stage,
    watchdog_snapshot: u32,
    _verifier: VerifierStub,
    _installer: InstallerStub,
}

#[derive(Debug)]
enum SupervisorError {
    Lifecycle(lifecycle::LifecycleError),
    UnexpectedFault,
    WrongChild(usize),
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lifecycle(error) => write!(f, "{error}"),
            Self::UnexpectedFault => write!(f, "unexpected child fault"),
            Self::WrongChild(child) => write!(f, "fault from unexpected child {child}"),
        }
    }
}

impl From<lifecycle::LifecycleError> for SupervisorError {
    fn from(value: lifecycle::LifecycleError) -> Self {
        Self::Lifecycle(value)
    }
}

#[protection_domain]
fn init() -> Supervisor {
    let ring = unsafe { WorkRing::mapped_mut() };
    ring.initialize();
    debug_println!("SUPERVISOR START");

    Supervisor {
        ring,
        stage: Stage::AwaitBoot1,
        watchdog_snapshot: 0,
        _verifier: VerifierStub::new(),
        _installer: InstallerStub::new(),
    }
}

impl Supervisor {
    fn on_worker_notification(&mut self) -> Result<(), SupervisorError> {
        let boot = self.ring.boot_generation().unwrap_or(0);
        let heartbeat = self.ring.heartbeat();

        match self.stage {
            Stage::AwaitBoot1 if boot == 1 && heartbeat > 0 => {
                debug_println!("HEARTBEAT GEN 1 {}", heartbeat);
                self.ring.set_command(COMMAND_POISON);
                self.stage = Stage::AwaitPoisonFault;
                WORKER_CHANNEL.notify();
            }
            Stage::AwaitBoot2 if boot == 2 && heartbeat > 0 => {
                debug_println!("POST-RESTART HEARTBEAT {}", heartbeat);
                self.watchdog_snapshot = heartbeat;
                self.ring.set_command(COMMAND_WATCHDOG_STALL);
                self.stage = Stage::AwaitWatchdogDeadline;
                WORKER_CHANNEL.notify();
            }
            Stage::AwaitWatchdogDeadline => {
                let current = self.ring.heartbeat();
                if current == self.watchdog_snapshot {
                    debug_println!("WATCHDOG STALL DETECTED {}", current);
                    self.ring.set_command(COMMAND_WATCHDOG_EXPIRE);
                    self.stage = Stage::AwaitWatchdogFault;
                    WORKER_CHANNEL.notify();
                }
            }
            Stage::AwaitBoot3 if boot == 3 && heartbeat > 0 => {
                debug_println!("POST-WATCHDOG HEARTBEAT {}", heartbeat);
                debug_println!("SUPDEMO PASS");
                self.stage = Stage::Complete;
            }
            Stage::Complete => {}
            _ => {
                debug_println!(
                    "SUPERVISOR EVENT stage={:?} boot={} heartbeat={}",
                    self.stage,
                    boot,
                    heartbeat
                );
            }
        }
        Ok(())
    }
}

impl Handler for Supervisor {
    type Error = SupervisorError;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if channels.contains(WORKER_CHANNEL) {
            self.on_worker_notification()?;
        }
        Ok(())
    }

    fn fault(
        &mut self,
        child: Child,
        _msg_info: MessageInfo,
    ) -> Result<Option<MessageInfo>, Self::Error> {
        if child.index() != WORKER_CHILD_ID {
            return Err(SupervisorError::WrongChild(child.index()));
        }
        if !matches!(
            self.stage,
            Stage::AwaitPoisonFault | Stage::AwaitWatchdogFault
        ) {
            return Err(SupervisorError::UnexpectedFault);
        }

        let watchdog = self.stage == Stage::AwaitWatchdogFault;
        if watchdog {
            debug_println!("WATCHDOG FAULT CAUGHT child={}", child.index());
        } else {
            debug_println!("FAULT CAUGHT child={}", child.index());
        }

        let restart_entry = self.ring.restart_entry();
        let stopped = unsafe { EndpointsStopped::new_unchecked() };
        let generation = lifecycle::reset_and_restart(
            child,
            self.ring,
            stopped,
            restart_entry,
        )?;

        if watchdog {
            debug_println!("WATCHDOG RESTART GEN {}", generation);
            self.stage = Stage::AwaitBoot3;
        } else {
            debug_println!("FAULT RESTART GEN {}", generation);
            self.stage = Stage::AwaitBoot2;
        }

        // The child was explicitly restarted, so do not reply to the fault.
        Ok(None)
    }
}
