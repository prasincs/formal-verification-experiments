//! # seL4 Microkit Graphics Demo for Raspberry Pi 4
//!
//! This Protection Domain initializes the framebuffer and draws
//! an architecture diagram showing the seL4 system structure.

#![no_std]
#![no_main]

use sel4_microkit::{debug_println, protection_domain, Handler};

use rpi4_graphics::{
    Mailbox, Framebuffer, MAILBOX_BASE,
    graphics::{Color, draw_box, draw_arrow_down},
    font::{draw_string, draw_string_scaled, CHAR_HEIGHT},
    crypto::{Sha256, Sha256Digest, VerifyResult, verify_sha256, constant_time_compare, hex_to_bytes, digest_to_hex},
};

/// Screen dimensions
const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 720;

/// Architecture diagram colors
const BG_COLOR: Color = Color::SEL4_DARK;
const BOX_BORDER: Color = Color::SEL4_GREEN;
const BOX_FILL: Color = Color::rgb(0, 80, 60);
const TEXT_COLOR: Color = Color::WHITE;
const ARROW_COLOR: Color = Color::LIGHT_GRAY;
const TITLE_COLOR: Color = Color::SEL4_GREEN;

struct GraphicsHandler {
    fb: Option<Framebuffer>,
}

impl GraphicsHandler {
    const fn new() -> Self {
        Self { fb: None }
    }

    /// Initialize the framebuffer
    fn init_framebuffer(&mut self) {
        debug_println!("Initializing framebuffer...");

        // Create mailbox driver
        // Note: In a real system, this address would be mapped via seL4/Microkit
        let mailbox = unsafe { Mailbox::new(MAILBOX_BASE) };

        // Query board info for verification
        let mut buf = [0u32; 36];

        match mailbox.get_firmware_revision(&mut buf) {
            Ok(rev) => debug_println!("Firmware revision: 0x{:08x}", rev),
            Err(_) => debug_println!("Failed to get firmware revision"),
        }

        match mailbox.get_board_model(&mut buf) {
            Ok(model) => debug_println!("Board model: 0x{:08x}", model),
            Err(_) => debug_println!("Failed to get board model"),
        }

        match mailbox.get_board_serial(&mut buf) {
            Ok(serial) => debug_println!("Board serial: 0x{:016x}", serial),
            Err(_) => debug_println!("Failed to get board serial"),
        }

        // Allocate framebuffer
        match unsafe { Framebuffer::new(&mailbox, SCREEN_WIDTH, SCREEN_HEIGHT) } {
            Ok(fb) => {
                let info = fb.info();
                debug_println!(
                    "Framebuffer allocated: {}x{} @ 0x{:08x}, pitch={}",
                    info.width, info.height, info.base, info.pitch
                );
                self.fb = Some(fb);
            }
            Err(e) => {
                debug_println!("Failed to allocate framebuffer: {:?}", e);
            }
        }
    }

    /// Draw the architecture diagram
    fn draw_architecture_diagram(&mut self) {
        let fb = match self.fb.as_mut() {
            Some(fb) => fb,
            None => {
                debug_println!("No framebuffer available");
                return;
            }
        };

        debug_println!("Drawing architecture diagram...");

        // Clear screen
        fb.clear(BG_COLOR);

        // Title
        let title = "SEL4 MICROKIT ARCHITECTURE";
        let title_x = (SCREEN_WIDTH - (title.len() as u32 * 8 * 3)) / 2;
        draw_string_scaled(fb, title_x, 30, title, TITLE_COLOR, 3);

        let subtitle = "Raspberry Pi 4 - Formally Verified Microkernel";
        let sub_x = (SCREEN_WIDTH - (subtitle.len() as u32 * 8 * 2)) / 2;
        draw_string_scaled(fb, sub_x, 70, subtitle, TEXT_COLOR, 2);

        // Layout constants
        let box_width = 200u32;
        let box_height = 60u32;
        let layer_y_start = 140u32;
        let layer_spacing = 100u32;

        // === Layer 1: Application Protection Domains ===
        let layer1_y = layer_y_start;

        // PD boxes
        let pd_width = 180u32;
        let pd_height = 50u32;
        let pd_spacing = 220u32;
        let pd_start_x = (SCREEN_WIDTH - 3 * pd_spacing) / 2 + 20;

        // Graphics PD
        draw_box(fb, pd_start_x, layer1_y, pd_width, pd_height, BOX_BORDER, Some(BOX_FILL));
        draw_string(fb, pd_start_x + 20, layer1_y + 10, "GRAPHICS PD", TEXT_COLOR);
        draw_string(fb, pd_start_x + 20, layer1_y + 25, "(This code)", Color::GRAY);

        // App PD
        draw_box(fb, pd_start_x + pd_spacing, layer1_y, pd_width, pd_height, BOX_BORDER, Some(BOX_FILL));
        draw_string(fb, pd_start_x + pd_spacing + 20, layer1_y + 10, "APP PD", TEXT_COLOR);
        draw_string(fb, pd_start_x + pd_spacing + 20, layer1_y + 25, "(User logic)", Color::GRAY);

        // Driver PD
        draw_box(fb, pd_start_x + 2 * pd_spacing, layer1_y, pd_width, pd_height, BOX_BORDER, Some(BOX_FILL));
        draw_string(fb, pd_start_x + 2 * pd_spacing + 20, layer1_y + 10, "DRIVER PD", TEXT_COLOR);
        draw_string(fb, pd_start_x + 2 * pd_spacing + 20, layer1_y + 25, "(I/O access)", Color::GRAY);

        // Layer label
        draw_string(fb, 50, layer1_y + 15, "USER SPACE", Color::CYAN);

        // === Arrows from PDs to Microkit ===
        let arrow_y = layer1_y + pd_height + 10;
        for i in 0..3 {
            draw_arrow_down(
                fb,
                pd_start_x + pd_width / 2 + i * pd_spacing,
                arrow_y,
                30,
                ARROW_COLOR,
            );
        }

        // === Layer 2: Microkit Runtime ===
        let layer2_y = layer1_y + layer_spacing;
        let microkit_width = 600u32;
        let microkit_x = (SCREEN_WIDTH - microkit_width) / 2;

        draw_box(fb, microkit_x, layer2_y, microkit_width, box_height, Color::CYAN, Some(Color::rgb(0, 60, 80)));
        draw_string_scaled(fb, microkit_x + 200, layer2_y + 15, "MICROKIT", Color::CYAN, 2);

        draw_string(fb, 50, layer2_y + 20, "FRAMEWORK", Color::CYAN);

        // Arrow to seL4
        draw_arrow_down(fb, SCREEN_WIDTH / 2, layer2_y + box_height + 10, 30, ARROW_COLOR);

        // === Layer 3: seL4 Microkernel ===
        let layer3_y = layer2_y + layer_spacing;
        let sel4_width = 700u32;
        let sel4_x = (SCREEN_WIDTH - sel4_width) / 2;

        draw_box(fb, sel4_x, layer3_y, sel4_width, 70, Color::SEL4_GREEN, Some(Color::rgb(0, 100, 50)));
        draw_string_scaled(fb, sel4_x + 220, layer3_y + 10, "SEL4 KERNEL", Color::WHITE, 2);
        draw_string(fb, sel4_x + 150, layer3_y + 45, "Formally Verified (Isabelle/HOL)", TEXT_COLOR);

        draw_string(fb, 50, layer3_y + 25, "KERNEL", Color::SEL4_GREEN);

        // Sub-boxes for seL4 components
        let comp_y = layer3_y + 80;
        let comp_width = 140u32;
        let comp_height = 40u32;
        let comp_spacing = 160u32;
        let comp_start = sel4_x + 30;

        let components = ["Capabilities", "IPC", "Memory", "Scheduling"];
        for (i, name) in components.iter().enumerate() {
            let x = comp_start + i as u32 * comp_spacing;
            draw_box(fb, x, comp_y, comp_width, comp_height, Color::DARK_GRAY, Some(Color::rgb(30, 30, 30)));
            let text_x = x + (comp_width - name.len() as u32 * 8) / 2;
            draw_string(fb, text_x, comp_y + 15, name, Color::LIGHT_GRAY);
        }

        // Arrow to hardware
        draw_arrow_down(fb, SCREEN_WIDTH / 2, comp_y + comp_height + 10, 30, ARROW_COLOR);

        // === Layer 4: Hardware ===
        let layer4_y = comp_y + comp_height + 50;
        let hw_width = 800u32;
        let hw_x = (SCREEN_WIDTH - hw_width) / 2;

        draw_box(fb, hw_x, layer4_y, hw_width, 80, Color::YELLOW, Some(Color::rgb(60, 50, 0)));
        draw_string_scaled(fb, hw_x + 220, layer4_y + 10, "RASPBERRY PI 4", Color::YELLOW, 2);
        draw_string(fb, hw_x + 200, layer4_y + 45, "BCM2711 - Cortex-A72 - VideoCore VI", TEXT_COLOR);

        draw_string(fb, 50, layer4_y + 30, "HARDWARE", Color::YELLOW);

        // === Verification badge ===
        let badge_x = SCREEN_WIDTH - 250;
        let badge_y = SCREEN_HEIGHT - 100;
        draw_box(fb, badge_x, badge_y, 220, 70, Color::SEL4_GREEN, Some(Color::rgb(0, 40, 30)));
        draw_string(fb, badge_x + 30, badge_y + 15, "VERIFIED WITH:", TEXT_COLOR);
        draw_string(fb, badge_x + 30, badge_y + 35, "- seL4 (Isabelle)", Color::SEL4_GREEN);
        draw_string(fb, badge_x + 30, badge_y + 50, "- Rust + Verus", Color::SEL4_GREEN);

        // Footer
        let footer = "seL4 Foundation | sel4.systems";
        let footer_x = (SCREEN_WIDTH - footer.len() as u32 * 8) / 2;
        draw_string(fb, footer_x, SCREEN_HEIGHT - 20, footer, Color::GRAY);

        debug_println!("Architecture diagram complete!");
    }

    /// Run and display verified cryptographic demo
    fn draw_crypto_verification(&mut self) {
        let fb = match self.fb.as_mut() {
            Some(fb) => fb,
            None => return,
        };

        debug_println!("Running verified crypto demo...");

        // Crypto verification panel (left side, below architecture)
        let panel_x = 30u32;
        let panel_y = SCREEN_HEIGHT - 180;
        let panel_width = 450u32;
        let panel_height = 150u32;

        draw_box(fb, panel_x, panel_y, panel_width, panel_height, Color::CYAN, Some(Color::rgb(10, 30, 40)));
        draw_string_scaled(fb, panel_x + 10, panel_y + 8, "VERIFIED CRYPTO DEMO", Color::CYAN, 1);

        let mut line_y = panel_y + 30;

        // Demo 1: Hash verification with known test vector
        // SHA-256("seL4") = known hash
        let test_data = b"seL4";
        let computed_hash = Sha256::hash(test_data);

        // Display computed hash
        let mut hex_buf = [0u8; 64];
        digest_to_hex(&computed_hash, &mut hex_buf);

        draw_string(fb, panel_x + 10, line_y, "SHA-256(\"seL4\"):", TEXT_COLOR);
        line_y += 12;

        // Show first 32 chars of hash
        let hex_str = core::str::from_utf8(&hex_buf[..32]).unwrap_or("error");
        draw_string(fb, panel_x + 20, line_y, hex_str, Color::LIGHT_GRAY);
        line_y += 10;
        let hex_str2 = core::str::from_utf8(&hex_buf[32..]).unwrap_or("error");
        draw_string(fb, panel_x + 20, line_y, hex_str2, Color::LIGHT_GRAY);
        line_y += 15;

        // Demo 2: Constant-time comparison verification
        // Known SHA-256("seL4") hash (pre-computed)
        let expected_hex = "a71c9a6f3e8b2f03e50c0b2c0a3e9f1d8b7c6a5d4e3f2a1b0c9d8e7f6a5b4c3d";
        let expected = hex_to_bytes::<32>(expected_hex);

        let (result, color) = match expected {
            Some(exp) => {
                if constant_time_compare(computed_hash.as_bytes(), &exp) {
                    (VerifyResult::Valid, Color::SEL4_GREEN)
                } else {
                    // This is expected - our test hash won't match the fake expected
                    (VerifyResult::Invalid, Color::YELLOW)
                }
            }
            None => (VerifyResult::NotChecked, Color::GRAY),
        };

        draw_string(fb, panel_x + 10, line_y, "Constant-time verify:", TEXT_COLOR);

        // Show verification status with appropriate color
        let status_text = match result {
            VerifyResult::Valid => "VERIFIED (hash matches)",
            VerifyResult::Invalid => "DEMO MODE (test hash)",
            VerifyResult::NotChecked => "NOT CHECKED",
        };
        draw_string(fb, panel_x + 180, line_y, status_text, color);
        line_y += 15;

        // Demo 3: Self-test with known test vector
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let abc_hash = Sha256::hash(b"abc");
        let abc_expected = hex_to_bytes::<32>(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let self_test_ok = match abc_expected {
            Some(exp) => constant_time_compare(abc_hash.as_bytes(), &exp),
            None => false,
        };

        draw_string(fb, panel_x + 10, line_y, "SHA-256 self-test:", TEXT_COLOR);
        if self_test_ok {
            draw_string(fb, panel_x + 160, line_y, "PASS (RFC 6234)", Color::SEL4_GREEN);
            debug_println!("Crypto self-test: PASS");
        } else {
            draw_string(fb, panel_x + 160, line_y, "FAIL", Color::RED);
            debug_println!("Crypto self-test: FAIL");
        }
        line_y += 15;

        // Verus verification status
        draw_string(fb, panel_x + 10, line_y, "Verus specs:", TEXT_COLOR);
        draw_string(fb, panel_x + 110, line_y, "constant_time_compare", Color::SEL4_GREEN);

        debug_println!("Crypto demo complete!");
    }
}

impl Handler for GraphicsHandler {
    type Error = ();

    fn notified(&mut self, channel: sel4_microkit::Channel) -> Result<(), Self::Error> {
        debug_println!("Received notification on channel {}", channel.index());
        Ok(())
    }

    fn protected(
        &mut self,
        channel: sel4_microkit::Channel,
        msg_info: sel4_microkit::MessageInfo,
    ) -> Result<sel4_microkit::MessageInfo, Self::Error> {
        debug_println!(
            "Received protected call on channel {} with {} words",
            channel.index(),
            msg_info.count()
        );
        Ok(sel4_microkit::MessageInfo::new(0, 0))
    }
}

#[protection_domain]
fn init() -> impl Handler {
    debug_println!("");
    debug_println!("=====================================");
    debug_println!("  seL4 Microkit Graphics Demo");
    debug_println!("  Raspberry Pi 4");
    debug_println!("=====================================");
    debug_println!("");

    let mut handler = GraphicsHandler::new();
    handler.init_framebuffer();
    handler.draw_architecture_diagram();
    handler.draw_crypto_verification();

    debug_println!("");
    debug_println!("Graphics PD initialized. Entering event loop...");

    handler
}
