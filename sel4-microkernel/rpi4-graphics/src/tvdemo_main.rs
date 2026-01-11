//! TV Demo for HDMI on Raspberry Pi 4
//!
//! This Protection Domain runs the TV demo application using the HDMI framebuffer.

#![no_std]
#![no_main]

use sel4_microkit::{debug_println, protection_domain, Handler, ChannelSet};
use core::fmt;

use rpi4_graphics::{Mailbox, Framebuffer, HdmiBackend, MAILBOX_BASE};
use rpi4_tvdemo::backend::ScaledDisplay;
use rpi4_tvdemo::TvDemo;
use rpi4_input::InputEvent;

/// Screen dimensions (HDMI 720p)
const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 720;

/// Virtual screen for demo (will be scaled up)
const VIRTUAL_WIDTH: u32 = 320;
const VIRTUAL_HEIGHT: u32 = 240;

struct TvDemoHandler {
    fb: Option<Framebuffer>,
    demo: Option<TvDemo>,
    frame_count: u32,
}

impl TvDemoHandler {
    const fn new() -> Self {
        Self {
            fb: None,
            demo: None,
            frame_count: 0,
        }
    }

    fn init_framebuffer(&mut self) {
        debug_println!("Initializing HDMI framebuffer...");

        let mailbox = unsafe { Mailbox::new(MAILBOX_BASE) };

        match mailbox.get_firmware_revision(&mut [0u32; 36]) {
            Ok(rev) => debug_println!("Firmware revision: 0x{:08x}", rev),
            Err(_) => debug_println!("Failed to get firmware revision"),
        }

        match unsafe { Framebuffer::new(&mailbox, SCREEN_WIDTH, SCREEN_HEIGHT) } {
            Ok(fb) => {
                let info = fb.info();
                debug_println!(
                    "Framebuffer: {}x{} @ 0x{:08x}",
                    info.width, info.height, info.base
                );
                self.fb = Some(fb);
            }
            Err(e) => {
                debug_println!("Failed to allocate framebuffer: {:?}", e);
            }
        }
    }

    fn init_demo(&mut self) {
        debug_println!("Initializing TV demo...");
        // Use virtual resolution - demo expects 320x240
        self.demo = Some(TvDemo::new(VIRTUAL_WIDTH, VIRTUAL_HEIGHT));
        debug_println!("TV demo ready!");
    }

    fn update(&mut self) {
        let fb = match self.fb.as_mut() {
            Some(fb) => fb,
            None => return,
        };

        let demo = match self.demo.as_mut() {
            Some(demo) => demo,
            None => return,
        };

        // Create HDMI backend and wrap with scaling
        let hdmi = HdmiBackend::new(fb);
        let mut display = ScaledDisplay::new(hdmi, VIRTUAL_WIDTH, VIRTUAL_HEIGHT);

        // Update and render demo
        demo.update();
        demo.render(&mut display);

        self.frame_count += 1;
    }

    fn handle_input(&mut self, event: InputEvent) {
        if let Some(demo) = self.demo.as_mut() {
            demo.handle_input(event);
        }
    }
}

#[protection_domain]
fn init() -> TvDemoHandler {
    debug_println!("");
    debug_println!("========================================");
    debug_println!("  TV Demo - HDMI Output                ");
    debug_println!("========================================");
    debug_println!("");

    let mut handler = TvDemoHandler::new();
    handler.init_framebuffer();
    handler.init_demo();

    // Initial render
    handler.update();

    debug_println!("");
    debug_println!("TV Demo running!");
    debug_println!("Use keyboard/IR remote to navigate");
    debug_println!("");

    handler
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
        // Handle timer tick for animation updates
        self.update();
        Ok(())
    }
}
