//! Serial output monitor for Raspberry Pi 4 boot debugging
//!
//! Provides real-time monitoring of serial output with:
//! - Timestamped logging
//! - Boot stage detection
//! - Error pattern highlighting
//! - Log file export

use crate::serial::{PortConfig, SerialConnection};
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use colored::Colorize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Boot stage detection patterns
const BOOT_STAGES: &[(&str, &str)] = &[
    ("GPU firmware", "Raspberry Pi"),
    ("GPU firmware", "bootcode.bin"),
    ("start.elf", "start"),
    ("U-Boot", "U-Boot"),
    ("Linux kernel", "Linux version"),
    ("Linux kernel", "Booting Linux"),
    ("Kernel init", "Run /init"),
    ("systemd", "systemd"),
    ("Login prompt", "login:"),
    ("seL4", "seL4"),
    ("seL4 Microkit", "microkit"),
    ("seL4 Microkit", "MON|"),
];

/// Error patterns to highlight
const ERROR_PATTERNS: &[&str] = &[
    "error",
    "Error",
    "ERROR",
    "fail",
    "Fail",
    "FAIL",
    "panic",
    "Panic",
    "PANIC",
    "kernel panic",
    "Oops",
    "BUG",
    "WARNING",
    "unable to",
    "cannot",
    "not found",
    "No such",
    "Permission denied",
    "timeout",
    "Timeout",
    "TIMEOUT",
    "fault",
    "Fault",
    "FAULT",
    "abort",
    "Abort",
    "ABORT",
];

/// Known Raspberry Pi boot error patterns with explanations
const RPI_BOOT_ERRORS: &[(&str, &str)] = &[
    (
        "mmc0: error",
        "SD card read error - check SD card connection or try a different card",
    ),
    (
        "kernel panic - not syncing",
        "Kernel panic - check kernel image and device tree",
    ),
    (
        "VFS: Cannot open root device",
        "Root filesystem not found - check boot config and partition",
    ),
    (
        "Kernel image is corrupt",
        "Kernel image corrupted - re-flash the SD card",
    ),
    (
        "start.elf not found",
        "Boot files missing - ensure proper boot partition setup",
    ),
    (
        "fixup.dat not found",
        "Boot files missing - copy firmware files to boot partition",
    ),
    (
        "device tree not found",
        "Device tree missing - check dtb files in boot partition",
    ),
    (
        "RAMDISK: incomplete write",
        "Initramfs load error - check initrd image or memory",
    ),
    (
        "Unable to handle kernel paging",
        "Memory fault - check kernel and device tree compatibility",
    ),
    (
        "seL4: cap fault",
        "seL4 capability fault - check system configuration",
    ),
    (
        "seL4: vaddr",
        "seL4 virtual address error - check memory mappings",
    ),
];

/// Configuration for serial monitoring
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// Port configuration
    pub port_config: PortConfig,
    /// Enable timestamp prefixes
    pub show_timestamps: bool,
    /// Enable boot stage detection
    pub detect_boot_stages: bool,
    /// Highlight errors
    pub highlight_errors: bool,
    /// Log file path (optional)
    pub log_file: Option<String>,
    /// Show hex dump for non-printable characters
    pub hex_dump: bool,
    /// Auto-detect baud rate on garbage output
    pub auto_baud: bool,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            port_config: PortConfig::default(),
            show_timestamps: true,
            detect_boot_stages: true,
            highlight_errors: true,
            log_file: None,
            hex_dump: false,
            auto_baud: false,
        }
    }
}

/// Serial output monitor with boot debugging features
pub struct SerialMonitor {
    config: MonitorConfig,
    connection: Option<SerialConnection>,
    log_writer: Option<BufWriter<File>>,
    current_stage: Option<String>,
    line_count: usize,
    error_count: usize,
    running: Arc<AtomicBool>,
}

impl SerialMonitor {
    /// Create a new serial monitor with the given configuration
    pub fn new(config: MonitorConfig) -> Self {
        Self {
            config,
            connection: None,
            log_writer: None,
            current_stage: None,
            line_count: 0,
            error_count: 0,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a clone of the running flag for signal handling
    pub fn get_running_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.running)
    }

    /// Connect to the serial port
    pub fn connect(&mut self) -> Result<()> {
        let connection = SerialConnection::open(self.config.port_config.clone())?;

        println!(
            "{} Connected to {} at {} baud",
            "[OK]".green().bold(),
            self.config.port_config.port_path.white().bold(),
            self.config.port_config.baud_rate
        );

        self.connection = Some(connection);

        // Setup log file if configured
        if let Some(ref log_path) = self.config.log_file {
            let file = File::create(log_path)
                .with_context(|| format!("Failed to create log file: {}", log_path))?;
            self.log_writer = Some(BufWriter::new(file));
            println!(
                "{} Logging to: {}",
                "[LOG]".cyan().bold(),
                log_path.white()
            );
        }

        Ok(())
    }

    /// Start monitoring serial output
    pub fn start(&mut self) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);

        println!("{}", "\n--- Serial Monitor Started ---".cyan().bold());
        println!("{}", "Press Ctrl+C to stop\n".yellow());

        // Print header
        self.print_header();

        // Main monitoring loop
        while self.running.load(Ordering::SeqCst) {
            if let Some(ref mut conn) = self.connection {
                match conn.read_line() {
                    Ok(Some(line)) => {
                        self.process_line(&line)?;
                    }
                    Ok(None) => {
                        // No data, brief sleep
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => {
                        eprintln!("{} Read error: {}", "[ERROR]".red().bold(), e);
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            } else {
                break;
            }
        }

        self.print_summary();
        Ok(())
    }

    /// Stop monitoring
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Process a single line of output
    fn process_line(&mut self, line: &str) -> Result<()> {
        self.line_count += 1;

        // Detect boot stage
        if self.config.detect_boot_stages {
            self.detect_boot_stage(line);
        }

        // Check for errors
        let is_error = self.config.highlight_errors && self.is_error_line(line);
        if is_error {
            self.error_count += 1;
        }

        // Format and print the line
        let formatted = self.format_line(line, is_error);
        println!("{}", formatted);

        // Check for known boot errors
        if is_error {
            self.check_known_errors(line);
        }

        // Write to log file
        if let Some(ref mut writer) = self.log_writer {
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            writeln!(writer, "[{}] {}", timestamp, line)?;
            writer.flush()?;
        }

        Ok(())
    }

    /// Detect boot stage from output
    fn detect_boot_stage(&mut self, line: &str) {
        for (stage, pattern) in BOOT_STAGES {
            if line.contains(pattern) {
                if self.current_stage.as_deref() != Some(*stage) {
                    self.current_stage = Some(stage.to_string());
                    println!(
                        "\n{} {} {}\n",
                        ">>>".blue().bold(),
                        "Boot Stage:".cyan(),
                        stage.white().bold()
                    );
                }
                break;
            }
        }
    }

    /// Check if a line contains error patterns
    fn is_error_line(&self, line: &str) -> bool {
        ERROR_PATTERNS.iter().any(|pattern| line.contains(pattern))
    }

    /// Check for known RPi boot errors and provide hints
    fn check_known_errors(&self, line: &str) {
        for (pattern, hint) in RPI_BOOT_ERRORS {
            if line.to_lowercase().contains(&pattern.to_lowercase()) {
                println!(
                    "  {} {}",
                    "HINT:".yellow().bold(),
                    hint.white()
                );
                break;
            }
        }
    }

    /// Format a line for display
    fn format_line(&self, line: &str, is_error: bool) -> String {
        let mut output = String::new();

        // Add timestamp if enabled
        if self.config.show_timestamps {
            let timestamp = Local::now().format("%H:%M:%S%.3f");
            output.push_str(&format!("{} ", timestamp.to_string().dimmed()));
        }

        // Color the line based on content
        if is_error {
            output.push_str(&line.red().to_string());
        } else if line.contains("OK") || line.contains("success") || line.contains("done") {
            output.push_str(&line.green().to_string());
        } else if line.contains("Warning") || line.contains("WARNING") {
            output.push_str(&line.yellow().to_string());
        } else {
            output.push_str(line);
        }

        output
    }

    /// Print monitor header
    fn print_header(&self) {
        println!("{}", "=".repeat(70).dimmed());
        println!(
            "{}: {}",
            "Port".cyan(),
            self.config.port_config.port_path.white()
        );
        println!(
            "{}: {}",
            "Baud".cyan(),
            self.config.port_config.baud_rate.to_string().white()
        );
        if let Some(ref log) = self.config.log_file {
            println!("{}: {}", "Log".cyan(), log.white());
        }
        println!("{}", "=".repeat(70).dimmed());
        println!();
    }

    /// Print summary statistics
    fn print_summary(&self) {
        println!("\n{}", "=".repeat(70).dimmed());
        println!("{}", "--- Monitor Summary ---".cyan().bold());
        println!("Total lines: {}", self.line_count);
        println!(
            "Errors detected: {}",
            if self.error_count > 0 {
                self.error_count.to_string().red().bold().to_string()
            } else {
                self.error_count.to_string().green().to_string()
            }
        );
        if let Some(ref stage) = self.current_stage {
            println!("Last boot stage: {}", stage.white().bold());
        }
        if let Some(ref log) = self.config.log_file {
            println!("Log saved to: {}", log.white());
        }
        println!("{}", "=".repeat(70).dimmed());
    }
}

/// Run the serial monitor with signal handling
pub fn run_monitor(config: MonitorConfig) -> Result<()> {
    let mut monitor = SerialMonitor::new(config);

    // Setup Ctrl+C handler
    let running = monitor.get_running_flag();
    ctrlc_handler(running)?;

    // Connect and start monitoring
    monitor.connect()?;
    monitor.start()?;

    Ok(())
}

/// Setup Ctrl+C signal handler
fn ctrlc_handler(running: Arc<AtomicBool>) -> Result<()> {
    ctrlc::set_handler(move || {
        println!("\n{}", "Stopping monitor...".yellow());
        running.store(false, Ordering::SeqCst);
    })
    .with_context(|| "Failed to set Ctrl+C handler")
}

// Add ctrlc dependency handling
mod ctrlc {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Simple Ctrl+C handler using signal hooks
    pub fn set_handler<F>(handler: F) -> Result<(), std::io::Error>
    where
        F: FnMut() + Send + 'static,
    {
        // Use nix signal handling on Unix
        #[cfg(unix)]
        {
            use std::thread;

            let handler = std::sync::Mutex::new(handler);

            // Spawn a thread to handle signals
            thread::spawn(move || {
                // Set up signal handling
                unsafe {
                    libc::signal(libc::SIGINT, handle_sigint as libc::sighandler_t);
                    // Store handler reference
                    HANDLER.store(Box::into_raw(Box::new(handler)) as usize, Ordering::SeqCst);
                }
            });

            Ok(())
        }

        #[cfg(not(unix))]
        {
            // Fallback for non-Unix systems
            let _ = handler;
            Ok(())
        }
    }

    #[cfg(unix)]
    static HANDLER: AtomicUsize = AtomicUsize::new(0);

    #[cfg(unix)]
    use std::sync::atomic::AtomicUsize;

    #[cfg(unix)]
    extern "C" fn handle_sigint(_: libc::c_int) {
        let ptr = HANDLER.load(Ordering::SeqCst);
        if ptr != 0 {
            unsafe {
                let handler: &std::sync::Mutex<Box<dyn FnMut() + Send>> =
                    &*(ptr as *const std::sync::Mutex<Box<dyn FnMut() + Send>>);
                if let Ok(mut h) = handler.lock() {
                    (*h)();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_stage_patterns() {
        // Verify all patterns are valid
        for (stage, pattern) in BOOT_STAGES {
            assert!(!stage.is_empty());
            assert!(!pattern.is_empty());
        }
    }

    #[test]
    fn test_error_detection() {
        let config = MonitorConfig::default();
        let monitor = SerialMonitor::new(config);

        assert!(monitor.is_error_line("kernel panic - not syncing"));
        assert!(monitor.is_error_line("ERROR: boot failed"));
        assert!(!monitor.is_error_line("Boot successful"));
    }
}
