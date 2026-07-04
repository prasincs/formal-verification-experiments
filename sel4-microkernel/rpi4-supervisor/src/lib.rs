#![no_std]

pub mod installer;
pub mod lifecycle;
pub mod protocol;
pub mod verifier;

pub mod build_constants {
    include!(concat!(env!("OUT_DIR"), "/worker_restart_entry.rs"));
}
