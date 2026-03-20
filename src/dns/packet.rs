use hickory_proto::op::Message;

use crate::error::DnsError;

#[derive(Debug, Clone)]
pub struct DnsPacket {
    pub inner: Message,
}

pub fn parse(bytes: &[u8]) -> Result<DnsPacket, DnsError> {
    let message = Message::from_vec(bytes)?;
    Ok(DnsPacket { inner: message })
}

impl DnsPacket {
    pub fn serialize(&self) -> Result<Vec<u8>, DnsError> {
        let bytes = self.inner.to_vec()?;
        Ok(bytes)
    }
}
