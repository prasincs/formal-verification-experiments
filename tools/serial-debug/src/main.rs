//! Serial Debug Tools
//!
//! A comprehensive debugging toolkit for embedded systems with device profiles
//! for Raspberry Pi 4, STM32, ESP32, and other devices.
//!
//! # Features
//!
//! - **Serial Monitor**: Read and analyze serial output from USB-to-serial adapters
//!   (requires `serial` feature and libudev on Linux)
//! - **Device Profiles**: Built-in profiles for RPi4, STM32, ESP32 with boot stages and error patterns
//! - **Boot Partition Analysis**: Validate boot files and configuration (for devices with boot partitions)
//! - **Kernel Image Analysis**: Analyze kernel images for compatibility
//!
//! # Usage
//!
//! ```bash
//! # List supported device profiles
//! serial-debug devices list
//!
//! # Show device profile details
//! serial-debug devices show rpi4
//!
//! # List available serial ports (requires serial feature)
//! serial-debug serial list
//!
//! # Monitor serial output with device profile
//! serial-debug serial monitor -p /dev/ttyUSB0 --device rpi4
//!
//! # Analyze boot partition (for RPi4)
//! serial-debug boot analyze /media/boot --device rpi4
//!
//! # Generate debug config for device
//! serial-debug generate config --device rpi4
//! ```

mod boot;
mod devices;
mod image;
#[cfg(feature = "serial")]
mod serial;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

use boot::{BootConfig, BootPartition, BootValidator};
use devices::{get_profile, profile_names, DeviceProfile};
use image::KernelImage;

#[cfg(feature = "serial")]
use std::time::Duration;
#[cfg(feature = "serial")]
use serial::{MonitorConfig, PortConfig};

/// Serial Debug Tools
///
/// Comprehensive debugging toolkit for embedded systems
#[derive(Parser)]
#[command(name = "serial-debug")]
#[command(author = "Prasanna Gautam")]
#[command(version = "0.1.0")]
#[command(about = "Serial debugging toolkit with device profiles for embedded systems")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Device profile operations
    #[command(subcommand)]
    Devices(DeviceCommands),

    /// Serial port operations (requires --features serial)
    #[cfg(feature = "serial")]
    #[command(subcommand)]
    Serial(SerialCommands),

    /// Boot partition operations
    #[command(subcommand)]
    Boot(BootCommands),

    /// Image analysis operations
    #[command(subcommand)]
    Image(ImageCommands),

    /// Generate debug configuration files
    #[command(subcommand)]
    Generate(GenerateCommands),
}

#[derive(Subcommand)]
enum DeviceCommands {
    /// List all supported device profiles
    List,

    /// Show detailed information about a device profile
    Show {
        /// Device profile name (e.g., rpi4, stm32, esp32)
        device: String,
    },
}

#[cfg(feature = "serial")]
#[derive(Subcommand)]
enum SerialCommands {
    /// List available serial ports
    List,

    /// Monitor serial output
    Monitor {
        /// Serial port path (e.g., /dev/ttyUSB0)
        #[arg(short, long)]
        port: Option<String>,

        /// Device profile for boot stage detection and error patterns
        #[arg(short, long, default_value = "generic")]
        device: String,

        /// Baud rate (overrides device profile default)
        #[arg(short, long)]
        baud: Option<u32>,

        /// Log output to file
        #[arg(short, long)]
        log: Option<String>,

        /// Disable timestamps
        #[arg(long)]
        no_timestamps: bool,

        /// Disable boot stage detection
        #[arg(long)]
        no_stages: bool,

        /// Disable error highlighting
        #[arg(long)]
        no_highlight: bool,
    },

    /// Auto-detect serial connection
    Detect {
        /// Device profile for USB VID/PID matching
        #[arg(short, long)]
        device: Option<String>,
    },

    /// Send a command to the serial port
    Send {
        /// Serial port path
        #[arg(short, long)]
        port: String,

        /// Command to send
        command: String,

        /// Device profile (for baud rate)
        #[arg(short, long, default_value = "generic")]
        device: String,

        /// Baud rate (overrides device profile)
        #[arg(short, long)]
        baud: Option<u32>,
    },
}

#[derive(Subcommand)]
enum BootCommands {
    /// Analyze boot partition structure
    Analyze {
        /// Path to boot partition (e.g., /media/boot, /boot)
        path: PathBuf,

        /// Device profile
        #[arg(short, long, default_value = "rpi4")]
        device: String,
    },

    /// Validate boot configuration
    Validate {
        /// Path to boot partition
        path: PathBuf,

        /// Device profile
        #[arg(short, long, default_value = "rpi4")]
        device: String,
    },

    /// Parse and analyze config.txt (RPi specific)
    Config {
        /// Path to config.txt or boot partition containing it
        path: PathBuf,
    },

    /// Check boot files for common issues
    Check {
        /// Path to boot partition
        path: PathBuf,

        /// Device profile
        #[arg(short, long, default_value = "rpi4")]
        device: String,
    },
}

#[derive(Subcommand)]
enum ImageCommands {
    /// Analyze kernel image
    Analyze {
        /// Path to kernel image
        path: PathBuf,
    },

    /// Compare multiple kernel images
    Compare {
        /// Paths to kernel images
        paths: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
enum GenerateCommands {
    /// Generate debug-friendly config.txt
    Config {
        /// Device profile
        #[arg(short, long, default_value = "rpi4")]
        device: String,

        /// Output path (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Generate cmdline.txt for serial debugging
    Cmdline {
        /// Device profile
        #[arg(short, long, default_value = "rpi4")]
        device: String,

        /// Output path (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Devices(cmd) => handle_devices(cmd),
        #[cfg(feature = "serial")]
        Commands::Serial(cmd) => handle_serial(cmd),
        Commands::Boot(cmd) => handle_boot(cmd),
        Commands::Image(cmd) => handle_image(cmd),
        Commands::Generate(cmd) => handle_generate(cmd),
    }
}

fn handle_devices(cmd: DeviceCommands) -> Result<()> {
    match cmd {
        DeviceCommands::List => {
            println!("{}", "=".repeat(60));
            println!("{}", "Supported Device Profiles".cyan().bold());
            println!("{}", "=".repeat(60));

            for name in profile_names() {
                if let Some(profile) = get_profile(name) {
                    println!(
                        "\n  {}: {}",
                        name.white().bold(),
                        profile.description
                    );
                    println!("    Manufacturer: {}", profile.manufacturer);
                    println!("    Architecture: {}", profile.architecture);
                    println!("    Default baud: {}", profile.serial.baud_rate);
                }
            }

            println!("\n{}", "=".repeat(60));
            println!(
                "Use {} to see detailed profile information",
                "serial-debug devices show <device>".cyan()
            );
        }

        DeviceCommands::Show { device } => {
            let profile = get_profile(&device).ok_or_else(|| {
                anyhow::anyhow!(
                    "Unknown device profile: {}. Use 'serial-debug devices list' to see available profiles.",
                    device
                )
            })?;

            print_device_profile(profile);
        }
    }

    Ok(())
}

fn print_device_profile(profile: &DeviceProfile) {
    println!("{}", "=".repeat(70));
    println!("{}", format!("Device Profile: {}", profile.name).cyan().bold());
    println!("{}", "=".repeat(70));

    println!("\n{}", "Basic Information:".white().bold());
    println!("  ID: {}", profile.id);
    println!("  Description: {}", profile.description);
    println!("  Manufacturer: {}", profile.manufacturer);
    println!("  Architecture: {}", profile.architecture);

    println!("\n{}", "Serial Settings:".white().bold());
    println!("  Default baud rate: {}", profile.serial.baud_rate);
    println!("  Data bits: {}", profile.serial.data_bits);
    println!("  Stop bits: {}", profile.serial.stop_bits);
    println!("  Parity: {}", profile.serial.parity);
    println!("  Flow control: {}", profile.serial.flow_control);
    println!(
        "  Alternative baud rates: {:?}",
        profile.serial.alt_baud_rates
    );

    println!("\n{}", "Boot Stages:".white().bold());
    for stage in &profile.boot_stages {
        println!("  {} - {}", stage.name.cyan(), stage.description);
        println!(
            "    Patterns: {}",
            stage.patterns.join(", ").dimmed()
        );
    }

    println!("\n{}", "Error Patterns:".white().bold());
    let error_count = profile.error_patterns.iter()
        .filter(|e| e.severity == "error")
        .count();
    let warning_count = profile.error_patterns.iter()
        .filter(|e| e.severity == "warning")
        .count();
    println!(
        "  {} error patterns, {} warning patterns",
        error_count.to_string().red(),
        warning_count.to_string().yellow()
    );

    if !profile.boot_files.is_empty() {
        println!("\n{}", "Boot Files:".white().bold());
        for file in &profile.boot_files {
            let required = if file.required {
                "[required]".red()
            } else {
                "[optional]".dimmed()
            };
            println!("  {} {} - {}", required, file.name, file.description);
        }
    }

    println!("\n{}", "=".repeat(70));
}

#[cfg(feature = "serial")]
fn handle_serial(cmd: SerialCommands) -> Result<()> {
    match cmd {
        SerialCommands::List => {
            serial::port::print_ports()?;
        }

        SerialCommands::Monitor {
            port,
            device,
            baud,
            log,
            no_timestamps,
            no_stages,
            no_highlight,
        } => {
            let profile = get_profile(&device).ok_or_else(|| {
                anyhow::anyhow!("Unknown device profile: {}", device)
            })?;

            // Use baud from profile if not specified
            let baud_rate = baud.unwrap_or(profile.serial.baud_rate);

            // Try to auto-detect port if not specified
            let port_path = if let Some(p) = port {
                p
            } else {
                let detected = serial::port::detect_rpi_ports()?;
                if detected.is_empty() {
                    eprintln!("{} No USB serial ports detected", "[ERROR]".red().bold());
                    eprintln!("Use -p to specify port manually");
                    std::process::exit(1);
                }
                println!(
                    "{} Auto-detected: {}",
                    "[OK]".green().bold(),
                    detected[0].path.white()
                );
                detected[0].path.clone()
            };

            println!(
                "{} Using device profile: {} (baud: {})",
                "[*]".cyan().bold(),
                profile.name.white(),
                baud_rate
            );

            let port_config = PortConfig::new(&port_path)
                .with_baud_rate(baud_rate)
                .with_timeout(Duration::from_millis(100));

            let config = MonitorConfig {
                port_config,
                show_timestamps: !no_timestamps,
                detect_boot_stages: !no_stages,
                highlight_errors: !no_highlight,
                log_file: log,
                hex_dump: false,
                auto_baud: false,
            };

            serial::monitor::run_monitor(config)?;
        }

        SerialCommands::Detect { device } => {
            let profile_name = device.as_deref().unwrap_or("generic");
            let profile = get_profile(profile_name);

            println!(
                "{} Detecting serial connections{}...",
                "[*]".cyan().bold(),
                profile.map(|p| format!(" for {}", p.name)).unwrap_or_default()
            );

            let ports = serial::port::detect_rpi_ports()?;

            if ports.is_empty() {
                println!("{}", "No USB-to-serial adapters detected".yellow());
                println!("\n{}", "Troubleshooting:".white().bold());
                println!("  1. Connect USB-to-serial adapter");
                println!("  2. Check permissions: sudo usermod -aG dialout $USER");
                println!("  3. Check dmesg for connection events");
            } else {
                println!("\n{}", "Detected serial ports:".green().bold());
                for port in &ports {
                    println!("\n  {}", port.path.white().bold());
                    if let Some(ref prod) = port.product {
                        println!("    Product: {}", prod);
                    }
                    if let (Some(vid), Some(pid)) = (port.vid, port.pid) {
                        println!("    VID:PID: {:04x}:{:04x}", vid, pid);
                    }
                }
                println!("\n{}", "To monitor:".cyan());
                println!(
                    "  serial-debug serial monitor -p {} --device {}",
                    ports[0].path.white(),
                    profile_name
                );
            }
        }

        SerialCommands::Send { port, command, device, baud } => {
            let profile = get_profile(&device).ok_or_else(|| {
                anyhow::anyhow!("Unknown device profile: {}", device)
            })?;

            let baud_rate = baud.unwrap_or(profile.serial.baud_rate);
            let config = PortConfig::new(&port).with_baud_rate(baud_rate);
            let mut conn = serial::SerialConnection::open(config)?;

            println!(
                "{} Sending to {} at {} baud: {}",
                "[TX]".cyan().bold(),
                port,
                baud_rate,
                command
            );
            conn.write_str(&command)?;
            conn.write_str("\r\n")?;
            conn.flush()?;

            println!("{}", "[OK] Command sent".green());
        }
    }

    Ok(())
}

fn handle_boot(cmd: BootCommands) -> Result<()> {
    match cmd {
        BootCommands::Analyze { path, device } => {
            let profile = get_profile(&device);

            println!(
                "{} Analyzing boot partition: {}\n",
                "[*]".cyan().bold(),
                path.display()
            );

            if let Some(p) = profile {
                println!("Using device profile: {}\n", p.name.cyan());
            }

            let partition = BootPartition::analyze(&path)?;
            partition.print_report();

            if partition.is_bootable() {
                println!(
                    "\n{} Boot partition appears valid",
                    "[OK]".green().bold()
                );
            } else {
                println!(
                    "\n{} Boot partition has issues that may prevent boot",
                    "[WARNING]".yellow().bold()
                );
            }
        }

        BootCommands::Validate { path, device } => {
            let profile = get_profile(&device);

            println!(
                "{} Validating boot configuration: {}\n",
                "[*]".cyan().bold(),
                path.display()
            );

            if let Some(p) = profile {
                println!("Using device profile: {}\n", p.name.cyan());
            }

            let mut validator = BootValidator::new(path.to_str().unwrap_or("."));
            let result = validator.validate()?;
            BootValidator::print_report(&result);

            std::process::exit(if result.passed { 0 } else { 1 });
        }

        BootCommands::Config { path } => {
            let config_path = if path.is_file() {
                path.clone()
            } else {
                path.join("config.txt")
            };

            if !config_path.exists() {
                eprintln!(
                    "{} config.txt not found at {}",
                    "[ERROR]".red().bold(),
                    config_path.display()
                );
                std::process::exit(1);
            }

            let config = BootConfig::parse(&config_path)?;
            config.print_report();
        }

        BootCommands::Check { path, device } => {
            let profile = get_profile(&device);

            println!(
                "{} Quick boot check: {}\n",
                "[*]".cyan().bold(),
                path.display()
            );

            if let Some(p) = profile {
                println!("Using device profile: {}\n", p.name.cyan());

                // Check files from profile
                let mut all_ok = true;
                for file in &p.boot_files {
                    let exists = path.join(&file.name).exists();
                    let status = if exists {
                        "[OK]".green()
                    } else if file.required {
                        all_ok = false;
                        "[MISSING]".red()
                    } else {
                        "[optional]".dimmed()
                    };
                    println!("  {} {} - {}", status, file.name, file.description);
                }

                if all_ok {
                    println!("\n{}", "All required boot files present".green().bold());
                } else {
                    println!(
                        "\n{}",
                        "Some required boot files are missing".yellow().bold()
                    );
                }
            } else {
                // Fallback to default RPi4 checks
                let checks = vec![
                    ("config.txt", path.join("config.txt").exists()),
                    ("cmdline.txt", path.join("cmdline.txt").exists()),
                    ("start4.elf", path.join("start4.elf").exists()),
                    ("fixup4.dat", path.join("fixup4.dat").exists()),
                    ("kernel8.img", path.join("kernel8.img").exists()),
                ];

                let mut all_ok = true;
                for (name, exists) in checks {
                    let status = if exists {
                        "[OK]".green()
                    } else {
                        all_ok = false;
                        "[MISSING]".red()
                    };
                    println!("  {} {}", status, name);
                }

                if all_ok {
                    println!("\n{}", "All essential boot files present".green().bold());
                } else {
                    println!(
                        "\n{}",
                        "Some boot files are missing - boot may fail".yellow().bold()
                    );
                }
            }
        }
    }

    Ok(())
}

fn handle_image(cmd: ImageCommands) -> Result<()> {
    match cmd {
        ImageCommands::Analyze { path } => {
            if !path.exists() {
                eprintln!(
                    "{} File not found: {}",
                    "[ERROR]".red().bold(),
                    path.display()
                );
                std::process::exit(1);
            }

            let image = KernelImage::analyze(&path)?;
            image.print_report();

            if image.is_bootable_pi4() {
                println!(
                    "\n{} Image appears bootable on Pi 4",
                    "[OK]".green().bold()
                );
            } else {
                println!(
                    "\n{} Image may not boot directly on Pi 4",
                    "[WARNING]".yellow().bold()
                );
            }
        }

        ImageCommands::Compare { paths } => {
            if paths.is_empty() {
                eprintln!("{} No images specified", "[ERROR]".red().bold());
                std::process::exit(1);
            }

            let mut images = Vec::new();
            for path in &paths {
                match KernelImage::analyze(path) {
                    Ok(img) => images.push(img),
                    Err(e) => {
                        eprintln!(
                            "{} Failed to analyze {}: {}",
                            "[ERROR]".red().bold(),
                            path.display(),
                            e
                        );
                    }
                }
            }

            println!("{}", "=".repeat(70));
            println!("{}", "Kernel Image Comparison".cyan().bold());
            println!("{}", "=".repeat(70));

            for img in &images {
                println!(
                    "\n{}: {}",
                    "Image".white().bold(),
                    img.path
                );
                println!("  Format: {}", img.format);
                println!("  Architecture: {}", img.architecture);
                println!("  Size: {} bytes", img.size);
                println!(
                    "  Bootable on Pi 4: {}",
                    if img.is_bootable_pi4() {
                        "Yes".green()
                    } else {
                        "No".red()
                    }
                );
            }

            if let Some(best) = image::kernel::find_best_kernel(&images) {
                println!(
                    "\n{} Recommended for Pi 4: {}",
                    "[*]".cyan().bold(),
                    best.path.white()
                );
            }

            println!("{}", "=".repeat(70));
        }
    }

    Ok(())
}

fn handle_generate(cmd: GenerateCommands) -> Result<()> {
    match cmd {
        GenerateCommands::Config { device, output } => {
            let config = match device.as_str() {
                "rpi4" | "raspberry-pi-4" => devices::rpi4::generate_debug_config(),
                _ => {
                    eprintln!(
                        "{} Config generation only supported for rpi4 currently",
                        "[WARNING]".yellow()
                    );
                    devices::rpi4::generate_debug_config()
                }
            };

            if let Some(path) = output {
                std::fs::write(&path, &config)?;
                println!(
                    "{} Debug config.txt written to {}",
                    "[OK]".green().bold(),
                    path.display()
                );
            } else {
                println!("{}", config);
            }
        }

        GenerateCommands::Cmdline { device, output } => {
            let cmdline = match device.as_str() {
                "rpi4" | "raspberry-pi-4" => devices::rpi4::generate_debug_cmdline(),
                _ => "console=ttyS0,115200 loglevel=7".to_string(),
            };

            if let Some(path) = output {
                std::fs::write(&path, &cmdline)?;
                println!(
                    "{} Debug cmdline.txt written to {}",
                    "[OK]".green().bold(),
                    path.display()
                );
            } else {
                println!("{}", cmdline);
            }
        }
    }

    Ok(())
}
