#![no_std]
#![no_main]

use core::arch::global_asm;
use core::convert::Infallible;

use rpi4_supervisor::protocol::{
    WorkRing, COMMAND_POISON, COMMAND_WATCHDOG_STALL, SUPERVISOR_CHANNEL_ID,
};
use sel4_microkit::{debug_println, protection_domain, Channel, ChannelSet, Handler};

const SUPERVISOR_CHANNEL: Channel = Channel::new(SUPERVISOR_CHANNEL_ID);

// The repository-pinned rust-sel4 runtime predates its `stack_size` macro
// support. This trampoline supplies the same restart property locally: reset
// SP to a dedicated 16 KiB stack and branch to the generated Microkit main
// symbol. Global runtime and IPC-buffer initialization were completed by the
// initial boot and intentionally remain persistent across child restarts.
global_asm!(
    r#"
    .section .bss.worker_restart_stack,"aw",%nobits
    .balign 16
worker_restart_stack:
    .skip 16384
worker_restart_stack_top:

    .section .text.worker_restart_entry,"ax"
    .balign 16
    .global worker_restart_entry
    .type worker_restart_entry, %function
worker_restart_entry:
    adrp x9, worker_restart_stack_top
    add  x9, x9, :lo12:worker_restart_stack_top
    mov  sp, x9
    b    __sel4_microkit__main
    .size worker_restart_entry, .-worker_restart_entry
    "#
);

unsafe extern "C" {
    fn worker_restart_entry();
}

struct Worker {
    ring: &'static WorkRing,
}

#[protection_domain]
fn init() -> Worker {
    let ring = unsafe { WorkRing::mapped() };
    let restart_entry = worker_restart_entry as *const () as usize as u64;
    ring.publish_restart_entry(restart_entry);
    let boot = ring
        .boot_generation()
        .expect("worker started during an odd reset generation");
    let heartbeat = ring.publish_heartbeat();
    debug_println!("BOOT GEN {}", boot);
    debug_println!("WORKER HEARTBEAT {}", heartbeat);
    SUPERVISOR_CHANNEL.notify();
    Worker { ring }
}

impl Handler for Worker {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if !channels.contains(SUPERVISOR_CHANNEL) {
            return Ok(());
        }

        match self.ring.command() {
            COMMAND_POISON => {
                debug_println!("POISON RECEIVED");
                // Deliberately touch an unmapped low address. The resulting VM
                // fault is delivered to the parent supervisor by Microkit.
                unsafe {
                    core::ptr::write_volatile(0x10usize as *mut u32, 0xdead_beef);
                }
            }
            COMMAND_WATCHDOG_STALL => {
                debug_println!("WATCHDOG STALL ARMED");
                // This notification represents the watchdog deadline in the
                // deterministic demo. The heartbeat remains unchanged, then
                // the worker spins until the higher-priority parent suspends it.
                SUPERVISOR_CHANNEL.notify();
                loop {
                    core::hint::spin_loop();
                }
            }
            _ => {}
        }

        Ok(())
    }
}
