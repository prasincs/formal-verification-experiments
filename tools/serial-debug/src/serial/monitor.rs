//! Serial output monitor for embedded device boot debugging
//!
//! Provides real-time monitoring of serial output with:
//! - Timestamped logging
//! - Device-profile-driven boot stage detection
//! - Error pattern highlighting with suggestions
//! - Log file export

use crate::devices::DeviceProfile;
use crate::serial::{PortConfig, SerialConnection};
use anyhow::{Context, Result};
use chrono::Local;
use colored::Colorize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Set to true by the SIGINT handler. A static is required because a C signal
/// handler cannot capture state; an atomic store is async-signal-safe.
static SIGINT_RECEIVED: AtomicBool = AtomicBool::new(false);

/// Configuration for serial monitoring
#[derive(Clone)]
pub struct MonitorConfig {
    /// Port configuration
    pub port_config: PortConfig,
    /// Device profile providing boot stages and error patterns
    pub profile: &'static DeviceProfile,
    /// Enable timestamp prefixes
    pub show_timestamps: bool,
    /// Enable boot stage detection
    pub detect_boot_stages: bool,
    /// Highlight errors
    pub highlight_errors: bool,
    /// Log file path (optional)
    pub log_file: Option<String>,
}

/// Serial output monitor with boot debugging features
pub struct SerialMonitor {
    config: MonitorConfig,
    connection: Option<SerialConnection>,
    log_writer: Option<BufWriter<File>>,
    current_stage: Option<String>,
    line_count: usize,
    error_count: usize,
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
        }
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

    /// Start monitoring serial output. Runs until SIGINT (Ctrl+C).
    pub fn start(&mut self) -> Result<()> {
        println!("{}", "\n--- Serial Monitor Started ---".cyan().bold());
        println!("{}", "Press Ctrl+C to stop\n".yellow());

        self.print_header();

        while !SIGINT_RECEIVED.load(Ordering::SeqCst) {
            let Some(conn) = self.connection.as_mut() else {
                break;
            };
            match conn.read_line() {
                Ok(Some(line)) => {
                    self.process_line(&line)?;
                }
                Ok(None) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    eprintln!("{} Read error: {}", "[ERROR]".red().bold(), e);
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }

        self.print_summary();
        Ok(())
    }

    /// Process a single line of output
    fn process_line(&mut self, line: &str) -> Result<()> {
        self.line_count += 1;

        if self.config.detect_boot_stages {
            self.detect_boot_stage(line);
        }

        let error = if self.config.highlight_errors {
            self.config.profile.match_error(line)
        } else {
            None
        };
        let is_error = error.map(|e| e.severity == "error").unwrap_or(false);
        let is_warning = error.map(|e| e.severity == "warning").unwrap_or(false);
        if is_error {
            self.error_count += 1;
        }

        let formatted = self.format_line(line, is_error, is_warning);
        println!("{}", formatted);

        // Print suggestion hint for recognized errors
        if let Some(err) = error {
            if let Some(ref suggestion) = err.suggestion {
                println!("  {} {}", "HINT:".yellow().bold(), suggestion.white());
            }
        }

        if let Some(ref mut writer) = self.log_writer {
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            writeln!(writer, "[{}] {}", timestamp, line)?;
            writer.flush()?;
        }

        Ok(())
    }

    /// Detect boot stage transitions using the device profile
    fn detect_boot_stage(&mut self, line: &str) {
        if let Some(stage) = self.config.profile.match_boot_stage(line) {
            if self.current_stage.as_deref() != Some(stage.name.as_str()) {
                self.current_stage = Some(stage.name.clone());
                println!(
                    "\n{} {} {}\n",
                    ">>>".blue().bold(),
                    "Boot Stage:".cyan(),
                    stage.name.white().bold()
                );
            }
        }
    }

    /// Format a line for display
    fn format_line(&self, line: &str, is_error: bool, is_warning: bool) -> String {
        let mut output = String::new();

        if self.config.show_timestamps {
            let timestamp = Local::now().format("%H:%M:%S%.3f");
            output.push_str(&format!("{} ", timestamp.to_string().dimmed()));
        }

        if is_error {
            output.push_str(&line.red().to_string());
        } else if is_warning {
            output.push_str(&line.yellow().to_string());
        } else if self.config.profile.is_success(line) {
            output.push_str(&line.green().to_string());
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
            "Device".cyan(),
            self.config.profile.name.white()
        );
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

/// Run the serial monitor until Ctrl+C
pub fn run_monitor(config: MonitorConfig) -> Result<()> {
    install_sigint_handler();

    let mut monitor = SerialMonitor::new(config);
    monitor.connect()?;
    monitor.start()?;

    Ok(())
}

/// Install a SIGINT handler that only sets an atomic flag (async-signal-safe).
fn install_sigint_handler() {
    #[cfg(unix)]
    unsafe {
        let handler = handle_sigint as extern "C" fn(libc::c_int);
        libc::signal(libc::SIGINT, handler as libc::sighandler_t);
    }
}

#[cfg(unix)]
extern "C" fn handle_sigint(_: libc::c_int) {
    SIGINT_RECEIVED.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use crate::devices::RPI4_PROFILE;

    #[test]
    fn test_profile_error_detection() {
        let profile = &*RPI4_PROFILE;

        let err = profile.match_error("Kernel panic - not syncing: attempted to kill init");
        assert!(err.is_some());
        assert_eq!(err.unwrap().severity, "error");

        assert!(profile.match_error("normal boot message").is_none());
    }

    #[test]
    fn test_profile_boot_stage_detection() {
        let profile = &*RPI4_PROFILE;

        let stage = profile.match_boot_stage("Linux version 6.6.0-v8+");
        assert!(stage.is_some());
        assert_eq!(stage.unwrap().name, "Linux Kernel");
    }
}
