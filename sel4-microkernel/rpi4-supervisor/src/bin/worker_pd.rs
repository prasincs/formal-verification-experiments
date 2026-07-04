#![no_std]
#![no_main]

use core::arch::global_asm;
use core::convert::Infallible;

use rpi4_supervisor::protocol::{
    WorkRing, COMMAND_POISON, COMMAND_WATCHDOG_STALL, SUPERVISOR_CHANNEL_ID,
};
use sel4_microkit::{debug_println, protection_domain, Channel, ChannelSet, Handler};

const SUPERVISOR_CHANNEL: Channel = Channel::new(SUPERVISOR_CHANNEL_ID);

// Microkit 2.1's pinned Rust runtime has no restartable-stack macro. The
// trusted build extracts this symbol from the linked worker ELF and injects its
// address into the supervisor build; the child never supplies the restart PC.
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

struct Worker {
    ring: &'static WorkRing,
}

#[protection_domain]
fn init() -> Worker {
    let ring = unsafe { WorkRing::mapped() };
    let boot = ring
        .boot_generation()
        .expect("worker started during an odd reset generation");
    // The boot-generation marker is the first canonical ring entry.
    ring.publish_boot_generation(boot);
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
                debug_println!("POISON RING ENTRY RECEIVED");
                unsafe {
                    core::ptr::write_volatile(0x10usize as *mut u32, 1);
                }
            }
            COMMAND_WATCHDOG_STALL => {
                debug_println!("WATCHDOG STALL ARMED");
                // No notification is sent. A PL031 timer interrupt owned by the
                // supervisor independently detects the unchanged heartbeat.
                loop {
                    core::hint::spin_loop();
                }
            }
            _ => {}
        }

        Ok(())
    }
}
