use crate::{Sha256Digest, TpmRc, TpmTransport, TransportError};

pub const TPM2_ST_NO_SESSIONS: u16 = 0x8001;
pub const TPM2_ST_SESSIONS: u16 = 0x8002;

pub const TPM2_CC_STARTUP: u32 = 0x0000_0144;
pub const TPM2_CC_PCR_EXTEND: u32 = 0x0000_0182;
pub const TPM2_CC_PCR_READ: u32 = 0x0000_017e;
pub const TPM2_CC_QUOTE: u32 = 0x0000_0158;
pub const TPM2_CC_NV_READ: u32 = 0x0000_014e;
pub const TPM2_CC_NV_INCREMENT: u32 = 0x0000_0134;
pub const TPM2_CC_CREATE_PRIMARY: u32 = 0x0000_0131;
pub const TPM2_CC_CREATE: u32 = 0x0000_0153;
pub const TPM2_CC_LOAD: u32 = 0x0000_0157;
pub const TPM2_CC_SIGN: u32 = 0x0000_015d;
pub const TPM2_CC_VERIFY_SIGNATURE: u32 = 0x0000_0177;

const MAX_COMMAND: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    Transport(TransportError),
    BodyTooLarge,
    ResponseTooShort,
    ResponseLength { declared: usize, actual: usize },
    Tpm(TpmRc),
    InvalidPcr,
}

impl From<TransportError> for CommandError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

pub struct TpmClient<T> {
    transport: T,
}

impl<T> TpmClient<T> {
    pub const fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn into_transport(self) -> T {
        self.transport
    }
}

impl<T: TpmTransport> TpmClient<T> {
    /// Execute a complete command body through the transport-neutral boundary.
    pub fn execute<'a>(
        &mut self,
        tag: u16,
        command_code: u32,
        body: &[u8],
        response: &'a mut [u8],
    ) -> Result<&'a [u8], CommandError> {
        let total = 10usize
            .checked_add(body.len())
            .ok_or(CommandError::BodyTooLarge)?;
        if total > MAX_COMMAND {
            return Err(CommandError::BodyTooLarge);
        }

        let mut command = [0u8; MAX_COMMAND];
        command[0..2].copy_from_slice(&tag.to_be_bytes());
        command[2..6].copy_from_slice(&(total as u32).to_be_bytes());
        command[6..10].copy_from_slice(&command_code.to_be_bytes());
        command[10..total].copy_from_slice(body);

        let actual = self.transport.exchange(&command[..total], response)?;
        validate_response(&response[..actual])
    }

    pub fn startup<'a>(
        &mut self,
        startup_type: u16,
        response: &'a mut [u8],
    ) -> Result<&'a [u8], CommandError> {
        self.execute(
            TPM2_ST_NO_SESSIONS,
            TPM2_CC_STARTUP,
            &startup_type.to_be_bytes(),
            response,
        )
    }

    pub fn pcr_extend<'a>(
        &mut self,
        pcr_index: u8,
        digest: &Sha256Digest,
        response: &'a mut [u8],
    ) -> Result<&'a [u8], CommandError> {
        if pcr_index > 23 {
            return Err(CommandError::InvalidPcr);
        }
        let mut body = [0u8; 55];
        body[0..4].copy_from_slice(&(pcr_index as u32).to_be_bytes());
        body[4..8].copy_from_slice(&9u32.to_be_bytes());
        body[8..12].copy_from_slice(&0x4000_0009u32.to_be_bytes());
        body[12..14].copy_from_slice(&0u16.to_be_bytes());
        body[14] = 0;
        body[15..17].copy_from_slice(&0u16.to_be_bytes());
        body[17..21].copy_from_slice(&1u32.to_be_bytes());
        body[21..23].copy_from_slice(&0x000bu16.to_be_bytes());
        body[23..55].copy_from_slice(&digest.bytes);
        self.execute(TPM2_ST_SESSIONS, TPM2_CC_PCR_EXTEND, &body, response)
    }

    pub fn pcr_read<'a>(
        &mut self,
        selection: [u8; 3],
        response: &'a mut [u8],
    ) -> Result<&'a [u8], CommandError> {
        let mut body = [0u8; 10];
        body[0..4].copy_from_slice(&1u32.to_be_bytes());
        body[4..6].copy_from_slice(&0x000bu16.to_be_bytes());
        body[6] = 3;
        body[7..10].copy_from_slice(&selection);
        self.execute(TPM2_ST_NO_SESSIONS, TPM2_CC_PCR_READ, &body, response)
    }

    pub fn quote<'a>(&mut self, body: &[u8], response: &'a mut [u8]) -> Result<&'a [u8], CommandError> {
        self.execute(TPM2_ST_SESSIONS, TPM2_CC_QUOTE, body, response)
    }

    pub fn nv_read<'a>(&mut self, body: &[u8], response: &'a mut [u8]) -> Result<&'a [u8], CommandError> {
        self.execute(TPM2_ST_SESSIONS, TPM2_CC_NV_READ, body, response)
    }

    pub fn nv_increment<'a>(&mut self, body: &[u8], response: &'a mut [u8]) -> Result<&'a [u8], CommandError> {
        self.execute(TPM2_ST_SESSIONS, TPM2_CC_NV_INCREMENT, body, response)
    }

    pub fn create_primary<'a>(&mut self, body: &[u8], response: &'a mut [u8]) -> Result<&'a [u8], CommandError> {
        self.execute(TPM2_ST_SESSIONS, TPM2_CC_CREATE_PRIMARY, body, response)
    }

    pub fn create<'a>(&mut self, body: &[u8], response: &'a mut [u8]) -> Result<&'a [u8], CommandError> {
        self.execute(TPM2_ST_SESSIONS, TPM2_CC_CREATE, body, response)
    }

    pub fn load<'a>(&mut self, body: &[u8], response: &'a mut [u8]) -> Result<&'a [u8], CommandError> {
        self.execute(TPM2_ST_SESSIONS, TPM2_CC_LOAD, body, response)
    }

    pub fn sign<'a>(&mut self, body: &[u8], response: &'a mut [u8]) -> Result<&'a [u8], CommandError> {
        self.execute(TPM2_ST_SESSIONS, TPM2_CC_SIGN, body, response)
    }

    pub fn verify_signature<'a>(
        &mut self,
        body: &[u8],
        response: &'a mut [u8],
    ) -> Result<&'a [u8], CommandError> {
        self.execute(
            TPM2_ST_NO_SESSIONS,
            TPM2_CC_VERIFY_SIGNATURE,
            body,
            response,
        )
    }
}

pub fn validate_response(response: &[u8]) -> Result<&[u8], CommandError> {
    if response.len() < 10 {
        return Err(CommandError::ResponseTooShort);
    }
    let declared = u32::from_be_bytes(response[2..6].try_into().expect("fixed range")) as usize;
    if declared < 10 || declared > response.len() {
        return Err(CommandError::ResponseLength {
            declared,
            actual: response.len(),
        });
    }
    let rc = u32::from_be_bytes(response[6..10].try_into().expect("fixed range"));
    if rc != 0 {
        return Err(CommandError::Tpm(TpmRc::from(rc)));
    }
    Ok(&response[..declared])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FakeTransport;

    fn command_code(command: &[u8]) -> u32 {
        u32::from_be_bytes(command[6..10].try_into().unwrap())
    }

    #[test]
    fn startup_uses_transport_boundary() {
        let mut client = TpmClient::new(FakeTransport::success());
        let mut response = [0u8; 32];
        client.startup(0, &mut response).unwrap();
        assert_eq!(command_code(&client.transport().last_command), TPM2_CC_STARTUP);
        assert_eq!(client.transport().exchanges, 1);
    }

    #[test]
    fn command_families_have_stable_codes() {
        let mut client = TpmClient::new(FakeTransport::success());
        let mut response = [0u8; 32];
        let cases: &[(u32, fn(&mut TpmClient<FakeTransport>, &mut [u8]) -> Result<(), CommandError>)] = &[
            (TPM2_CC_QUOTE, |c, r| c.quote(&[], r).map(|_| ())),
            (TPM2_CC_NV_READ, |c, r| c.nv_read(&[], r).map(|_| ())),
            (TPM2_CC_NV_INCREMENT, |c, r| c.nv_increment(&[], r).map(|_| ())),
            (TPM2_CC_CREATE_PRIMARY, |c, r| c.create_primary(&[], r).map(|_| ())),
            (TPM2_CC_CREATE, |c, r| c.create(&[], r).map(|_| ())),
            (TPM2_CC_LOAD, |c, r| c.load(&[], r).map(|_| ())),
            (TPM2_CC_SIGN, |c, r| c.sign(&[], r).map(|_| ())),
            (TPM2_CC_VERIFY_SIGNATURE, |c, r| c.verify_signature(&[], r).map(|_| ())),
        ];
        for (expected, call) in cases {
            call(&mut client, &mut response).unwrap();
            assert_eq!(command_code(&client.transport().last_command), *expected);
        }
    }

    #[test]
    fn malformed_response_is_rejected() {
        let mut fake = FakeTransport::success();
        fake.response = std::vec![0; 9];
        let mut client = TpmClient::new(fake);
        let mut response = [0u8; 32];
        assert_eq!(client.startup(0, &mut response), Err(CommandError::ResponseTooShort));
    }
}
