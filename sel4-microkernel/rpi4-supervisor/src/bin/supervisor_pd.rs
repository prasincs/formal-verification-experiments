#![no_std]
#![no_main]

use core::fmt;
use core::ptr::{read_volatile, write_volatile};

use rpi4_supervisor::installer::InstallerStub;
use rpi4_supervisor::lifecycle::{self, EndpointsStopped};
use rpi4_supervisor::protocol::{
    WorkRing, COMMAND_POISON, COMMAND_WATCHDOG_STALL, SUPERVISOR_CHANNEL_ID,
    WATCHDOG_IRQ_CHANNEL_ID,
};
use rpi4_supervisor::verifier::VerifierStub;
use sel4_microkit::{
    debug_println, protection_domain, Channel, ChannelSet, Child, Handler, MessageInfo,
};

const WORKER_CHANNEL: Channel = Channel::new(SUPERVISOR_CHANNEL_ID);
const WATCHDOG_CHANNEL: Channel = Channel::new(WATCHDOG_IRQ_CHANNEL_ID);
const WORKER_CHILD_ID: usize = 1;

const PL031_VADDR: usize = 0x5_0500_0000;
const RTC_DR: usize = 0x000;
const RTC_MR: usize = 0x004;
const RTC_CR: usize = 0x00c;
const RTC_IMSC: usize = 0x010;
const RTC_ICR: usize = 0x01c;
const WATCHDOG_SECONDS: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    AwaitBoot1,
    AwaitPoisonFault,
    AwaitBoot2,
    AwaitWatchdogDeadline,
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
    IrqAck,
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lifecycle(error) => write!(f, "{error}"),
            Self::UnexpectedFault => write!(f, "unexpected child fault"),
            Self::WrongChild(child) => write!(f, "fault from unexpected child {child}"),
            Self::IrqAck => write!(f, "watchdog IRQ acknowledgement failed"),
        }
    }
}

impl From<lifecycle::LifecycleError> for SupervisorError {
    fn from(value: lifecycle::LifecycleError) -> Self {
        Self::Lifecycle(value)
    }
}

fn rtc_register(offset: usize) -> *mut u32 {
    (PL031_VADDR + offset) as *mut u32
}

fn arm_watchdog() {
    unsafe {
        write_volatile(rtc_register(RTC_ICR), 1);
        let now = read_volatile(rtc_register(RTC_DR));
        write_volatile(rtc_register(RTC_MR), now.wrapping_add(WATCHDOG_SECONDS));
        write_volatile(rtc_register(RTC_CR), 1);
        write_volatile(rtc_register(RTC_IMSC), 1);
    }
}

fn disarm_watchdog() {
    unsafe {
        write_volatile(rtc_register(RTC_IMSC), 0);
        write_volatile(rtc_register(RTC_ICR), 1);
    }
}

#[protection_domain]
fn init() -> Supervisor {
    let ring = unsafe { WorkRing::mapped_mut() };
    ring.initialize();
    disarm_watchdog();
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
    fn on_worker_notification(&mut self) {
        let boot = self.ring.observed_boot_generation();
        let heartbeat = self.ring.heartbeat();

        match self.stage {
            Stage::AwaitBoot1 if boot == 1 && heartbeat > 0 => {
                debug_println!("HEARTBEAT GEN 1 {}", heartbeat);
                // Crash-on-demand is an actual ring entry consumed by worker.
                self.ring.set_command(COMMAND_POISON);
                self.stage = Stage::AwaitPoisonFault;
                WORKER_CHANNEL.notify();
            }
            Stage::AwaitBoot2 if boot == 2 && heartbeat > 0 => {
                debug_println!("POST-RESTART HEARTBEAT {}", heartbeat);
                self.watchdog_snapshot = heartbeat;
                arm_watchdog();
                self.ring.set_command(COMMAND_WATCHDOG_STALL);
                self.stage = Stage::AwaitWatchdogDeadline;
                WORKER_CHANNEL.notify();
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
    }

    fn on_watchdog_irq(&mut self) -> Result<(), SupervisorError> {
        disarm_watchdog();
        if self.stage == Stage::AwaitWatchdogDeadline {
            let current = self.ring.heartbeat();
            if current == self.watchdog_snapshot {
                debug_println!("WATCHDOG TIMER EXPIRED {}", current);
                let child = Child::new(WORKER_CHILD_ID);
                let stopped = lifecycle::stop(child)?;
                let generation = lifecycle::reset_and_restart(child, self.ring, stopped)?;
                debug_println!("WATCHDOG RESTART GEN {}", generation);
                self.stage = Stage::AwaitBoot3;
            } else {
                self.watchdog_snapshot = current;
                arm_watchdog();
            }
        }
        WATCHDOG_CHANNEL
            .irq_ack()
            .map_err(|_| SupervisorError::IrqAck)
    }
}

impl Handler for Supervisor {
    type Error = SupervisorError;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if channels.contains(WORKER_CHANNEL) {
            self.on_worker_notification();
        }
        if channels.contains(WATCHDOG_CHANNEL) {
            self.on_watchdog_irq()?;
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
        if self.stage != Stage::AwaitPoisonFault {
            return Err(SupervisorError::UnexpectedFault);
        }

        debug_println!("FAULT CAUGHT child={}", child.index());
        let stopped = unsafe { EndpointsStopped::new_unchecked() };
        let generation = lifecycle::reset_and_restart(child, self.ring, stopped)?;
        debug_println!("FAULT RESTART GEN {}", generation);
        self.stage = Stage::AwaitBoot2;
        Ok(None)
    }
}
