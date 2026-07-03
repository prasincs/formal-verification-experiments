use crate::{Slb9670Tpm, TpmRc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    InvalidCommand,
    ResponseTooLarge { required: usize, available: usize },
    MalformedResponse,
    Timeout,
    Device(TpmRc),
}

/// Transport boundary for complete TPM 2.0 command/response frames.
pub trait TpmTransport {
    fn exchange(&mut self, cmd: &[u8], resp: &mut [u8]) -> Result<usize, TransportError>;
}

impl TpmTransport for Slb9670Tpm {
    fn exchange(&mut self, cmd: &[u8], resp: &mut [u8]) -> Result<usize, TransportError> {
        let response_len = self.execute_command(cmd).map_err(TransportError::Device)?;
        if response_len > resp.len() {
            return Err(TransportError::ResponseTooLarge {
                required: response_len,
                available: resp.len(),
            });
        }
        resp[..response_len].copy_from_slice(self.response());
        Ok(response_len)
    }
}

/// Host-test backend that records the command and returns a configured frame.
#[cfg(any(test, feature = "std"))]
pub struct FakeTransport {
    pub last_command: std::vec::Vec<u8>,
    pub response: std::vec::Vec<u8>,
    pub exchanges: usize,
}

#[cfg(any(test, feature = "std"))]
impl FakeTransport {
    pub fn success() -> Self {
        Self {
            last_command: std::vec::Vec::new(),
            response: std::vec![0x80, 0x01, 0, 0, 0, 10, 0, 0, 0, 0],
            exchanges: 0,
        }
    }
}

#[cfg(any(test, feature = "std"))]
impl TpmTransport for FakeTransport {
    fn exchange(&mut self, cmd: &[u8], resp: &mut [u8]) -> Result<usize, TransportError> {
        self.last_command.clear();
        self.last_command.extend_from_slice(cmd);
        self.exchanges += 1;
        if self.response.len() > resp.len() {
            return Err(TransportError::ResponseTooLarge {
                required: self.response.len(),
                available: resp.len(),
            });
        }
        resp[..self.response.len()].copy_from_slice(&self.response);
        Ok(self.response.len())
    }
}
