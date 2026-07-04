//! Wave 2 installer boundary.
//!
//! This stub deliberately has no executable-region mapping or write API. The
//! future installer will consume authenticated one-shot authorizations from a
//! separate verifier PD and re-hash a private staging copy before installation.

#[derive(Clone, Copy, Debug, Default)]
pub struct InstallerStub;

impl InstallerStub {
    pub const fn new() -> Self {
        Self
    }
}
