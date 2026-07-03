#![no_std]
#![allow(unused)]
#![allow(clippy::assign_op_pattern)]
#![allow(clippy::new_without_default)]

#[cfg(any(test, feature = "std"))]
extern crate std;

#[path = "lib.rs"]
mod legacy;

pub use legacy::*;

pub mod command;
pub mod crb;
pub mod transport;

pub use command::{CommandError, TpmClient};
pub use crb::{CrbIo, CrbTransport, MmioCrb};
pub use transport::{TpmTransport, TransportError};
#[cfg(any(test, feature = "std"))]
pub use transport::FakeTransport;
