//! Wave 2 update-verifier boundary.
//!
//! WP-3 intentionally performs no signature or capsule verification. Keeping
//! this module distinct prevents lifecycle code from accumulating verifier
//! authority before the one-shot authorization protocol is implemented.

#[derive(Clone, Copy, Debug, Default)]
pub struct VerifierStub;

impl VerifierStub {
    pub const fn new() -> Self {
        Self
    }
}
