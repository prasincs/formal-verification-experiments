//! SPI Display Protection Domain
//!
//! This is the main entry point for the verified SPI display system.

#![no_std]
#![no_main]

use sel4_microkit::{protection_domain, Handler, MessageInfo};

use rpi4_spi_display::display::{Display, Rgb565};
use rpi4_spi_display::touch::TouchController;

/// Display Protection Domain state
struct DisplayPd {
    display: Option<Display>,
    touch: Option<TouchController>,
}

impl DisplayPd {
    const fn new() -> Self {
        Self {
            display: None,
            touch: None,
        }
    }
}

#[protection_domain]
fn init() -> impl Handler {
    // TODO: Initialize SPI peripheral
    // TODO: Initialize GPIO for DC/RST/BL pins
    // TODO: Initialize ILI9341 display
    // TODO: Initialize XPT2046 touch controller
    // TODO: Run touch calibration if needed

    DisplayPd::new()
}

impl Handler for DisplayPd {
    type Error = core::convert::Infallible;

    fn notified(&mut self, channel: sel4_microkit::Channel) -> Result<(), Self::Error> {
        match channel.index() {
            // Touch interrupt
            0 => {
                if let Some(ref mut touch) = self.touch {
                    // TODO: Read touch event and process
                }
            }
            // Refresh timer
            1 => {
                if let Some(ref mut display) = self.display {
                    // TODO: Refresh dirty regions
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn protected(
        &mut self,
        channel: sel4_microkit::Channel,
        msg: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        // Handle IPC from other protection domains
        // e.g., draw commands, touch event queries
        Ok(MessageInfo::default())
    }
}
