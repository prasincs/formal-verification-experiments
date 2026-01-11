//! TV Demo for HDMI on Raspberry Pi 4
//!
//! Displays animated graphics on HDMI using direct framebuffer access.
//! Uses rpi4-tvdemo for portable animation code with scaling from 320x240
//! virtual resolution to 1920x1080 physical display.

#![no_std]
#![no_main]

use sel4_microkit::{debug_println, protection_domain, Handler, ChannelSet};
use core::fmt;

use rpi4_graphics::DirectHdmiBackend;
use rpi4_tvdemo::backend::{DisplayBackend, ScaledDisplay};
use rpi4_tvdemo::TvDemo;

/// Virtual resolution for the demo (scaled up to 1920x1080)
const VIRTUAL_WIDTH: u32 = 320;
const VIRTUAL_HEIGHT: u32 = 240;

/// GPIO virtual address (mapped in tvdemo.system)
const GPIO_BASE: usize = 0x5_0200_0000;

struct TvDemoHandler {
    frame_count: u32,
}

impl TvDemoHandler {
    const fn new() -> Self {
        Self {
            frame_count: 0,
        }
    }
}

/// Blink the activity LED to prove seL4 is running
fn blink_activity_led() {
    debug_println!("Blinking activity LED...");

    const GPFSEL4: usize = GPIO_BASE + 0x10;
    const GPSET1: usize = GPIO_BASE + 0x20;
    const GPCLR1: usize = GPIO_BASE + 0x2C;
    const BLINK_DELAY: u32 = 3_000_000;

    unsafe {
        core::arch::asm!("dsb sy");

        // Set GPIO 42 as output
        let gpfsel4 = GPFSEL4 as *mut u32;
        let mut val = gpfsel4.read_volatile();
        val &= !(7 << 6);
        val |= 1 << 6;
        gpfsel4.write_volatile(val);

        core::arch::asm!("dsb sy");

        // Blink 3 times
        for _ in 0..3 {
            (GPSET1 as *mut u32).write_volatile(1 << 10);
            for _ in 0..BLINK_DELAY { core::hint::spin_loop(); }
            (GPCLR1 as *mut u32).write_volatile(1 << 10);
            for _ in 0..BLINK_DELAY { core::hint::spin_loop(); }
        }

        core::arch::asm!("dsb sy");
    }
    debug_println!("LED done!");
}

/// Delay helper for animation timing
#[inline]
fn delay(count: u32) {
    for _ in 0..count {
        core::hint::spin_loop();
    }
}

/// Run the TV demo with animations
fn run_demo() {
    debug_println!("Initializing TV Demo...");

    // Create the display backend (direct framebuffer access)
    let hdmi = DirectHdmiBackend::new();
    debug_println!("DirectHdmiBackend created: {}x{}", hdmi.width(), hdmi.height());

    // Wrap with scaled display for virtual 320x240 resolution
    let mut display = ScaledDisplay::new(hdmi, VIRTUAL_WIDTH, VIRTUAL_HEIGHT);
    let (scale_x, scale_y) = display.scale();
    debug_println!("ScaledDisplay created: {}x{} virtual, scale {}x{}",
        VIRTUAL_WIDTH, VIRTUAL_HEIGHT, scale_x, scale_y);

    // Create the TV demo application
    let mut demo = TvDemo::new(VIRTUAL_WIDTH, VIRTUAL_HEIGHT);
    debug_println!("TvDemo created");

    // Start playing the bouncing ball animation automatically
    debug_println!("Starting animation...");

    // Simulate selecting "Play Animation" to start with bouncing ball
    use rpi4_tvdemo::{InputEvent, KeyEvent, KeyCode, KeyState, KeyModifiers};

    // Navigate to "Play Animation" and select it
    demo.handle_input(InputEvent::Key(KeyEvent {
        key: KeyCode::Enter,
        state: KeyState::Pressed,
        modifiers: KeyModifiers::default(),
    }));

    debug_println!("Animation loop starting...");

    // Animation frame timing
    const FRAME_DELAY: u32 = 500_000; // ~60fps at 1.5GHz

    // Run animation loop (infinite)
    loop {
        // Update animation state
        demo.update();

        // Render current frame
        demo.render(&mut display);

        // Frame delay
        delay(FRAME_DELAY);
    }
}

#[protection_domain]
fn init() -> TvDemoHandler {
    debug_println!("");
    debug_println!("========================================");
    debug_println!("  TV Demo - HDMI Animation             ");
    debug_println!("========================================");
    debug_println!("");

    // Blink LED to show we're running
    blink_activity_led();

    // Run the demo (this blocks with animation loop)
    run_demo();

    // This is never reached due to infinite loop above
    TvDemoHandler::new()
}

/// Handler error type
#[derive(Debug)]
pub struct HandlerError;

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TvDemo handler error")
    }
}

impl Handler for TvDemoHandler {
    type Error = HandlerError;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        self.frame_count += 1;
        Ok(())
    }
}
