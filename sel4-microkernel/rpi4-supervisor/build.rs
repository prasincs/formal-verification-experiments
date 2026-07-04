use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=WORKER_RESTART_ENTRY");
    let raw = env::var("WORKER_RESTART_ENTRY").unwrap_or_else(|_| "0".to_owned());
    let value = if let Some(hex) = raw.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).expect("WORKER_RESTART_ENTRY must be hexadecimal")
    } else {
        raw.parse::<u64>()
            .expect("WORKER_RESTART_ENTRY must be an integer")
    };
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"))
        .join("worker_restart_entry.rs");
    fs::write(
        output,
        format!("pub const WORKER_RESTART_ENTRY: u64 = {value};\n"),
    )
    .expect("write generated restart entry");
}
