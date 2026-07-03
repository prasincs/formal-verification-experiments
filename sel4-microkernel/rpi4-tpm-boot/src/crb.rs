use crate::{TpmTransport, TransportError};

pub const CRB_CTRL_REQ: usize = 0x40;
pub const CRB_CTRL_STS: usize = 0x44;
pub const CRB_CTRL_CANCEL: usize = 0x48;
pub const CRB_CTRL_START: usize = 0x4c;
pub const CRB_CTRL_CMD_SIZE: usize = 0x58;
pub const CRB_CTRL_CMD_ADDR: usize = 0x5c;
pub const CRB_CTRL_RSP_SIZE: usize = 0x64;
pub const CRB_CTRL_RSP_ADDR: usize = 0x68;

/// Minimal CRB register/buffer operations. A QEMU Microkit PD can implement
/// this over its mapped TPM CRB MMIO window without exposing MMIO to the
/// command layer.
pub trait CrbIo {
    fn read32(&self, offset: usize) -> u32;
    fn write32(&mut self, offset: usize, value: u32);
    fn command_capacity(&self) -> usize;
    fn response_capacity(&self) -> usize;
    fn write_command(&mut self, bytes: &[u8]);
    fn read_response(&self, out: &mut [u8]);
}

pub struct CrbTransport<I> {
    io: I,
    poll_limit: u32,
}

impl<I> CrbTransport<I> {
    pub const fn new(io: I, poll_limit: u32) -> Self {
        Self { io, poll_limit }
    }

    pub fn io(&self) -> &I {
        &self.io
    }

    pub fn io_mut(&mut self) -> &mut I {
        &mut self.io
    }

    pub fn into_io(self) -> I {
        self.io
    }
}

impl<I: CrbIo> TpmTransport for CrbTransport<I> {
    fn exchange(&mut self, cmd: &[u8], resp: &mut [u8]) -> Result<usize, TransportError> {
        if cmd.len() < 10 || cmd.len() > self.io.command_capacity() {
            return Err(TransportError::InvalidCommand);
        }
        self.io.write_command(cmd);
        self.io.write32(CRB_CTRL_START, 1);

        let mut complete = false;
        for _ in 0..self.poll_limit {
            if self.io.read32(CRB_CTRL_START) & 1 == 0 {
                complete = true;
                break;
            }
        }
        if !complete {
            return Err(TransportError::Timeout);
        }

        let mut header = [0u8; 10];
        self.io.read_response(&mut header);
        let response_len = u32::from_be_bytes(
            header[2..6]
                .try_into()
                .map_err(|_| TransportError::MalformedResponse)?,
        ) as usize;
        if response_len < 10 || response_len > self.io.response_capacity() {
            return Err(TransportError::MalformedResponse);
        }
        if response_len > resp.len() {
            return Err(TransportError::ResponseTooLarge {
                required: response_len,
                available: resp.len(),
            });
        }
        self.io.read_response(&mut resp[..response_len]);
        Ok(response_len)
    }
}

/// Direct MMIO CRB backend for the QEMU `tpm-crb-device` window. The command
/// and response buffers must be separately mapped into the TPM PD.
pub struct MmioCrb {
    registers: *mut u8,
    command_buffer: *mut u8,
    command_len: usize,
    response_buffer: *const u8,
    response_len: usize,
}

impl MmioCrb {
    /// # Safety
    /// All pointers must denote mapped, uncached CRB regions exclusively owned
    /// by the TPM PD for the lifetime of this value.
    pub const unsafe fn new(
        registers: *mut u8,
        command_buffer: *mut u8,
        command_len: usize,
        response_buffer: *const u8,
        response_len: usize,
    ) -> Self {
        Self {
            registers,
            command_buffer,
            command_len,
            response_buffer,
            response_len,
        }
    }
}

unsafe impl Send for MmioCrb {}

impl CrbIo for MmioCrb {
    fn read32(&self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile(self.registers.add(offset).cast::<u32>()) }
    }

    fn write32(&mut self, offset: usize, value: u32) {
        unsafe { core::ptr::write_volatile(self.registers.add(offset).cast::<u32>(), value) }
    }

    fn command_capacity(&self) -> usize {
        self.command_len
    }

    fn response_capacity(&self) -> usize {
        self.response_len
    }

    fn write_command(&mut self, bytes: &[u8]) {
        for (index, value) in bytes.iter().copied().enumerate() {
            unsafe { core::ptr::write_volatile(self.command_buffer.add(index), value) }
        }
    }

    fn read_response(&self, out: &mut [u8]) {
        for (index, value) in out.iter_mut().enumerate() {
            *value = unsafe { core::ptr::read_volatile(self.response_buffer.add(index)) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeCrb {
        command: [u8; 64],
        response: [u8; 64],
        starts: u32,
    }

    impl FakeCrb {
        fn new() -> Self {
            let mut response = [0u8; 64];
            response[..10].copy_from_slice(&[0x80, 0x01, 0, 0, 0, 10, 0, 0, 0, 0]);
            Self {
                command: [0; 64],
                response,
                starts: 0,
            }
        }
    }

    impl CrbIo for FakeCrb {
        fn read32(&self, offset: usize) -> u32 {
            if offset == CRB_CTRL_START { 0 } else { 0 }
        }

        fn write32(&mut self, offset: usize, value: u32) {
            if offset == CRB_CTRL_START && value == 1 {
                self.starts += 1;
            }
        }

        fn command_capacity(&self) -> usize { self.command.len() }
        fn response_capacity(&self) -> usize { self.response.len() }

        fn write_command(&mut self, bytes: &[u8]) {
            self.command[..bytes.len()].copy_from_slice(bytes);
        }

        fn read_response(&self, out: &mut [u8]) {
            out.copy_from_slice(&self.response[..out.len()]);
        }
    }

    #[test]
    fn qemu_crb_round_trip() {
        let io = FakeCrb::new();
        let mut transport = CrbTransport::new(io, 10);
        let command = [0x80, 0x01, 0, 0, 0, 10, 0, 0, 1, 0x44];
        let mut response = [0u8; 32];
        assert_eq!(transport.exchange(&command, &mut response).unwrap(), 10);
        assert_eq!(transport.io().starts, 1);
        assert_eq!(&transport.io().command[..10], &command);
    }
}
