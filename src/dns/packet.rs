use hickory_proto::op::Message;

use crate::error::DnsError;

#[derive(Debug, Clone)]
pub struct DnsPacket {
    pub inner: Message,
}

impl DnsPacket {
    pub fn serialize(&self) -> Result<Vec<u8>, DnsError> {
        let bytes = self.inner.to_vec()?;
        Ok(bytes)
    }
    pub fn parse(bytes: &[u8]) -> Result<Self, DnsError> {
        let message = Message::from_vec(bytes)?;
        Ok(Self { inner: message })
    }
    pub fn make_nxdomain(&self) -> Self {
        let mut response = Message::new();

        response.set_id(self.inner.id());

        response.set_message_type(hickory_proto::op::MessageType::Response);

        response.set_response_code(hickory_proto::op::ResponseCode::NXDomain);

        if let Some(query) = self.inner.queries().first() {
            response.add_query(query.clone());
        }
        DnsPacket { inner: response }
    }
    pub fn get_domain(&self) -> Option<String> {
        self.inner
            .queries()
            .first()
            .map(|query| query.name().to_string())
    }
    pub fn get_ttl(&self) -> u32 {
        self.inner
            .answers()
            .iter()
            .map(|record| record.ttl())
            .min()
            .unwrap_or(300)
    }
}
