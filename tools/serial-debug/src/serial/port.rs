//! Serial port configuration and connection management
//!
//! Handles USB serial port discovery and connection for Raspberry Pi debugging.

use anyhow::{Context, Result};
use colored::Colorize;
use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::io::{Read, Write};
use std::time::Duration;

/// Common baud rates for Raspberry Pi serial console
pub const COMMON_BAUD_RATES: &[u32] = &[
    9600,    // Legacy
    19200,   // Legacy
    38400,   // Legacy
    57600,   // Common for older devices
    115200,  // Default for Raspberry Pi
    230400,  // High speed
    460800,  // High speed
    921600,  // Very high speed
    1000000, // 1 Mbps
];

/// Default Raspberry Pi 4 serial configuration
pub const RPI4_DEFAULT_BAUD: u32 = 115200;

/// Configuration for serial port connection
#[derive(Debug, Clone)]
pub struct PortConfig {
    /// Serial port path (e.g., /dev/ttyUSB0, /dev/ttyACM0)
    pub port_path: String,
    /// Baud rate (default: 115200 for RPi4)
    pub baud_rate: u32,
    /// Data bits (default: 8)
    pub data_bits: DataBits,
    /// Parity (default: None)
    pub parity: Parity,
    /// Stop bits (default: 1)
    pub stop_bits: StopBits,
    /// Flow control (default: None)
    pub flow_control: FlowControl,
    /// Read timeout
    pub timeout: Duration,
}

impl Default for PortConfig {
    fn default() -> Self {
        Self {
            port_path: String::from("/dev/ttyUSB0"),
            baud_rate: RPI4_DEFAULT_BAUD,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
            timeout: Duration::from_millis(100),
        }
    }
}

impl PortConfig {
    /// Create a new configuration with default RPi4 settings
    pub fn new(port_path: &str) -> Self {
        Self {
            port_path: port_path.to_string(),
            ..Default::default()
        }
    }

    /// Set the baud rate
    pub fn with_baud_rate(mut self, baud_rate: u32) -> Self {
        self.baud_rate = baud_rate;
        self
    }

    /// Set the read timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Wrapper around a serial port connection with RPi4-specific functionality
pub struct SerialConnection {
    port: Box<dyn SerialPort>,
    config: PortConfig,
}

impl SerialConnection {
    /// Open a serial connection with the given configuration
    pub fn open(config: PortConfig) -> Result<Self> {
        let port = serialport::new(&config.port_path, config.baud_rate)
            .data_bits(config.data_bits)
            .parity(config.parity)
            .stop_bits(config.stop_bits)
            .flow_control(config.flow_control)
            .timeout(config.timeout)
            .open()
            .with_context(|| format!("Failed to open serial port: {}", config.port_path))?;

        Ok(Self { port, config })
    }

    /// Get the port configuration
    pub fn config(&self) -> &PortConfig {
        &self.config
    }

    /// Read bytes from the serial port
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        self.port
            .read(buffer)
            .with_context(|| "Failed to read from serial port")
    }

    /// Read a line from the serial port (until newline)
    pub fn read_line(&mut self) -> Result<Option<String>> {
        let mut buffer = Vec::new();
        let mut byte = [0u8; 1];

        loop {
            match self.port.read(&mut byte) {
                Ok(1) => {
                    if byte[0] == b'\n' {
                        break;
                    }
                    buffer.push(byte[0]);
                }
                Ok(0) => {
                    if buffer.is_empty() {
                        return Ok(None);
                    }
                    break;
                }
                Ok(_) => unreachable!(),
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    if buffer.is_empty() {
                        return Ok(None);
                    }
                    break;
                }
                Err(e) => return Err(e).with_context(|| "Failed to read from serial port"),
            }
        }

        // Handle carriage returns
        if buffer.last() == Some(&b'\r') {
            buffer.pop();
        }

        Ok(Some(String::from_utf8_lossy(&buffer).to_string()))
    }

    /// Write bytes to the serial port
    pub fn write(&mut self, data: &[u8]) -> Result<usize> {
        self.port
            .write(data)
            .with_context(|| "Failed to write to serial port")
    }

    /// Write a string to the serial port
    pub fn write_str(&mut self, s: &str) -> Result<()> {
        self.write(s.as_bytes())?;
        Ok(())
    }

    /// Send a break signal (useful for entering debug mode on some bootloaders)
    pub fn send_break(&mut self) -> Result<()> {
        self.port
            .set_break()
            .with_context(|| "Failed to set break")?;
        std::thread::sleep(Duration::from_millis(100));
        self.port
            .clear_break()
            .with_context(|| "Failed to clear break")?;
        Ok(())
    }

    /// Flush output buffer
    pub fn flush(&mut self) -> Result<()> {
        self.port
            .flush()
            .with_context(|| "Failed to flush serial port")
    }

    /// Clear input and output buffers
    pub fn clear_buffers(&mut self) -> Result<()> {
        self.port
            .clear(serialport::ClearBuffer::All)
            .with_context(|| "Failed to clear serial buffers")
    }

    /// Set DTR (Data Terminal Ready) signal
    pub fn set_dtr(&mut self, level: bool) -> Result<()> {
        self.port
            .write_data_terminal_ready(level)
            .with_context(|| "Failed to set DTR")
    }

    /// Set RTS (Request To Send) signal
    pub fn set_rts(&mut self, level: bool) -> Result<()> {
        self.port
            .write_request_to_send(level)
            .with_context(|| "Failed to set RTS")
    }
}

/// Information about a detected serial port
#[derive(Debug, Clone)]
pub struct PortInfo {
    pub path: String,
    pub port_type: PortType,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial_number: Option<String>,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PortType {
    UsbSerial,
    PciSerial,
    Bluetooth,
    Unknown,
}

impl std::fmt::Display for PortType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortType::UsbSerial => write!(f, "USB Serial"),
            PortType::PciSerial => write!(f, "PCI Serial"),
            PortType::Bluetooth => write!(f, "Bluetooth"),
            PortType::Unknown => write!(f, "Unknown"),
        }
    }
}

/// List all available serial ports
pub fn list_ports() -> Result<Vec<PortInfo>> {
    let ports = serialport::available_ports().with_context(|| "Failed to enumerate serial ports")?;

    let port_infos: Vec<PortInfo> = ports
        .into_iter()
        .map(|p| {
            let (port_type, manufacturer, product, serial_number, vid, pid) = match p.port_type {
                serialport::SerialPortType::UsbPort(info) => (
                    PortType::UsbSerial,
                    info.manufacturer,
                    info.product,
                    info.serial_number,
                    Some(info.vid),
                    Some(info.pid),
                ),
                serialport::SerialPortType::PciPort => {
                    (PortType::PciSerial, None, None, None, None, None)
                }
                serialport::SerialPortType::BluetoothPort => {
                    (PortType::Bluetooth, None, None, None, None, None)
                }
                serialport::SerialPortType::Unknown => {
                    (PortType::Unknown, None, None, None, None, None)
                }
            };

            PortInfo {
                path: p.port_name,
                port_type,
                manufacturer,
                product,
                serial_number,
                vid,
                pid,
            }
        })
        .collect();

    Ok(port_infos)
}

/// Print formatted list of available serial ports
pub fn print_ports() -> Result<()> {
    let ports = list_ports()?;

    if ports.is_empty() {
        println!("{}", "No serial ports found".yellow());
        println!("\n{}", "Troubleshooting tips:".cyan().bold());
        println!("  1. Connect a USB-to-serial adapter");
        println!("  2. Check if the device is recognized: ls -la /dev/ttyUSB* /dev/ttyACM*");
        println!("  3. Add your user to the 'dialout' group: sudo usermod -aG dialout $USER");
        println!("  4. Check dmesg for connection events: dmesg | tail -20");
        return Ok(());
    }

    println!("{}", "Available Serial Ports:".green().bold());
    println!("{}", "=".repeat(60));

    for port in ports {
        println!("\n{}: {}", "Port".cyan(), port.path.white().bold());
        println!("  Type: {}", port.port_type);

        if let Some(ref mfg) = port.manufacturer {
            println!("  Manufacturer: {}", mfg);
        }
        if let Some(ref prod) = port.product {
            println!("  Product: {}", prod);
        }
        if let Some(ref sn) = port.serial_number {
            println!("  Serial: {}", sn);
        }
        if let (Some(vid), Some(pid)) = (port.vid, port.pid) {
            println!("  VID:PID: {:04x}:{:04x}", vid, pid);
        }
    }

    println!("\n{}", "=".repeat(60));
    println!(
        "{}",
        format!(
            "Use: rpi4-debug serial monitor -p <PORT> to start monitoring",
        )
        .yellow()
    );

    Ok(())
}

/// Auto-detect likely Raspberry Pi serial ports
pub fn detect_rpi_ports() -> Result<Vec<PortInfo>> {
    let ports = list_ports()?;

    // Filter for USB serial ports that are likely USB-to-serial adapters
    // Common chips: FTDI, CP210x, CH340, PL2303
    let rpi_ports: Vec<PortInfo> = ports
        .into_iter()
        .filter(|p| {
            // Must be USB serial
            if p.port_type != PortType::UsbSerial {
                return false;
            }

            // Check for common USB-to-serial adapter VID/PIDs
            if let (Some(vid), Some(pid)) = (p.vid, p.pid) {
                // FTDI
                if vid == 0x0403 {
                    return true;
                }
                // Silicon Labs CP210x
                if vid == 0x10c4 && (pid == 0xea60 || pid == 0xea70) {
                    return true;
                }
                // WCH CH340/CH341
                if vid == 0x1a86 && (pid == 0x7523 || pid == 0x5523) {
                    return true;
                }
                // Prolific PL2303
                if vid == 0x067b && pid == 0x2303 {
                    return true;
                }
            }

            // Fallback: check product name for common keywords
            if let Some(ref prod) = p.product {
                let prod_lower = prod.to_lowercase();
                return prod_lower.contains("serial")
                    || prod_lower.contains("uart")
                    || prod_lower.contains("usb")
                    || prod_lower.contains("ftdi")
                    || prod_lower.contains("ch340");
            }

            false
        })
        .collect();

    Ok(rpi_ports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PortConfig::default();
        assert_eq!(config.baud_rate, 115200);
        assert_eq!(config.port_path, "/dev/ttyUSB0");
    }

    #[test]
    fn test_config_builder() {
        let config = PortConfig::new("/dev/ttyACM0")
            .with_baud_rate(9600)
            .with_timeout(Duration::from_secs(1));

        assert_eq!(config.port_path, "/dev/ttyACM0");
        assert_eq!(config.baud_rate, 9600);
        assert_eq!(config.timeout, Duration::from_secs(1));
    }
}
