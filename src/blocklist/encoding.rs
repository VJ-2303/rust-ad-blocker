pub(crate) fn encode_domain(domain: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    for label in domain.split('.') {
        let len = label.len();
        if len > 0 {
            bytes.push(len as u8);
            bytes.extend_from_slice(label.as_bytes());
        }
    }
    bytes.push(0);
    bytes
}

pub(crate) fn decode_domain(bytes: &[u8]) -> String {
    let mut domain = String::new();

    let mut i = 0;

    while i < bytes.len() {
        let len = bytes[i] as usize;
        if len == 0 {
            break;
        }
        i += 1;
        if i + len > bytes.len() {
            break;
        }
        if !domain.is_empty() {
            domain.push('.');
        }
        if let Ok(label) = std::str::from_utf8(&bytes[i..i + len]) {
            domain.push_str(label);
        }
        i += len
    }
    domain
}
