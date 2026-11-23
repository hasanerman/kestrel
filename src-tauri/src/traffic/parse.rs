use std::net::Ipv4Addr;

pub const TCP: u8 = 6;
pub const UDP: u8 = 17;

#[derive(Debug, PartialEq)]
pub struct Packet<'a> {
    pub source: Ipv4Addr,
    pub destination: Ipv4Addr,
    pub protocol: u8,
    pub source_port: u16,
    pub destination_port: u16,
    pub length: usize,
    pub payload: &'a [u8],
}

pub fn ipv4(data: &[u8]) -> Option<Packet<'_>> {
    if data.len() < 20 || data[0] >> 4 != 4 {
        return None;
    }
    let header = usize::from(data[0] & 0x0f) * 4;
    let total = usize::from(u16::from_be_bytes([data[2], data[3]])).min(data.len());
    if header < 20 || total < header {
        return None;
    }
    let fragment_offset = u16::from_be_bytes([data[6], data[7]]) & 0x1fff;
    let protocol = data[9];
    let source = Ipv4Addr::new(data[12], data[13], data[14], data[15]);
    let destination = Ipv4Addr::new(data[16], data[17], data[18], data[19]);
    let body = &data[header..total];
    let (source_port, destination_port, payload) = match protocol {
        _ if fragment_offset != 0 => (0, 0, &body[..0]),
        TCP if body.len() >= 20 => {
            let offset = usize::from(body[12] >> 4) * 4;
            let payload = body.get(offset.max(20)..).unwrap_or(&[]);
            (u16::from_be_bytes([body[0], body[1]]), u16::from_be_bytes([body[2], body[3]]), payload)
        }
        UDP if body.len() >= 8 => (u16::from_be_bytes([body[0], body[1]]), u16::from_be_bytes([body[2], body[3]]), &body[8..]),
        _ => (0, 0, &body[..0]),
    };
    Some(Packet { source, destination, protocol, source_port, destination_port, length: total, payload })
}

#[derive(Debug, PartialEq)]
pub struct Dns {
    pub response: bool,
    pub name: String,
    pub kind: u16,
    pub answers: Vec<Ipv4Addr>,
}

fn dns_name(message: &[u8], mut position: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut end = None;
    for _ in 0..64 {
        let length = *message.get(position)?;
        if length == 0 {
            return Some((labels.join("."), end.unwrap_or(position + 1)));
        }
        if length & 0xc0 == 0xc0 {
            let pointer = usize::from(u16::from_be_bytes([length & 0x3f, *message.get(position + 1)?]));
            end.get_or_insert(position + 2);
            position = pointer;
            continue;
        }
        let start = position + 1;
        let label = message.get(start..start + usize::from(length))?;
        labels.push(String::from_utf8_lossy(label).into_owned());
        position = start + usize::from(length);
    }
    None
}

pub fn dns(message: &[u8]) -> Option<Dns> {
    if message.len() < 12 {
        return None;
    }
    let response = message[2] & 0x80 != 0;
    let questions = u16::from_be_bytes([message[4], message[5]]);
    let answers = u16::from_be_bytes([message[6], message[7]]);
    if questions != 1 {
        return None;
    }
    let (name, after) = dns_name(message, 12)?;
    let kind = u16::from_be_bytes([*message.get(after)?, *message.get(after + 1)?]);
    let mut position = after + 4;
    let mut addresses = Vec::new();
    for _ in 0..answers.min(32) {
        let (_, next) = dns_name(message, position)?;
        let record = message.get(next..next + 10)?;
        let record_kind = u16::from_be_bytes([record[0], record[1]]);
        let length = usize::from(u16::from_be_bytes([record[8], record[9]]));
        let data = message.get(next + 10..next + 10 + length)?;
        if record_kind == 1 && length == 4 {
            addresses.push(Ipv4Addr::new(data[0], data[1], data[2], data[3]));
        }
        position = next + 10 + length;
    }
    Some(Dns { response, name, kind, answers: addresses })
}

pub fn server_name(payload: &[u8]) -> Option<String> {
    if payload.len() < 43 || payload[0] != 0x16 || payload[5] != 0x01 {
        return None;
    }
    let mut position = 5 + 4 + 2 + 32;
    let session = usize::from(*payload.get(position)?);
    position += 1 + session;
    let suites = usize::from(u16::from_be_bytes([*payload.get(position)?, *payload.get(position + 1)?]));
    position += 2 + suites;
    let compression = usize::from(*payload.get(position)?);
    position += 1 + compression;
    let extensions_end = position + 2 + usize::from(u16::from_be_bytes([*payload.get(position)?, *payload.get(position + 1)?]));
    position += 2;
    while position + 4 <= extensions_end.min(payload.len()) {
        let kind = u16::from_be_bytes([payload[position], payload[position + 1]]);
        let length = usize::from(u16::from_be_bytes([payload[position + 2], payload[position + 3]]));
        let body = payload.get(position + 4..position + 4 + length)?;
        if kind == 0 && body.len() >= 5 && body[2] == 0 {
            let name_length = usize::from(u16::from_be_bytes([body[3], body[4]]));
            let name = body.get(5..5 + name_length)?;
            return std::str::from_utf8(name).ok().map(str::to_ascii_lowercase);
        }
        position += 4 + length;
    }
    None
}

pub fn tls_version(payload: &[u8]) -> Option<&'static str> {
    if payload.len() < 5 || !(0x14..=0x17).contains(&payload[0]) || payload[1] != 0x03 {
        return None;
    }
    Some(match payload[2] {
        0x01 => "TLS 1.0",
        0x02 => "TLS 1.1",
        _ => "TLS",
    })
}

pub fn http_request(payload: &[u8]) -> Option<(String, Option<String>)> {
    const METHODS: &[&str] = &["GET ", "POST ", "PUT ", "HEAD ", "DELETE ", "OPTIONS ", "PATCH ", "CONNECT "];
    let head = std::str::from_utf8(payload.get(..payload.len().min(1024))?).ok()?;
    if !METHODS.iter().any(|m| head.starts_with(m)) {
        return None;
    }
    let mut lines = head.lines();
    let request = lines.next()?.chars().take(120).collect();
    let host = lines
        .take_while(|line| !line.is_empty())
        .find_map(|line| line.strip_prefix("Host:").or_else(|| line.strip_prefix("host:")))
        .map(|h| h.trim().to_ascii_lowercase());
    Some((request, host))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dns_response() -> Vec<u8> {
        let mut m = vec![0xa1, 0x2f, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0];
        m.extend_from_slice(b"\x07discord\x03com\x00\x00\x01\x00\x01");
        m.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 162, 159, 137, 232]);
        m
    }

    #[test]
    fn parses_dns_answers() {
        let parsed = dns(&dns_response()).unwrap();
        assert!(parsed.response);
        assert_eq!(parsed.name, "discord.com");
        assert_eq!(parsed.kind, 1);
        assert_eq!(parsed.answers, vec![Ipv4Addr::new(162, 159, 137, 232)]);
    }

    #[test]
    fn parses_ipv4_udp() {
        let payload = dns_response();
        let mut packet = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, UDP, 0, 0, 192, 168, 8, 1, 192, 168, 8, 42];
        packet.extend_from_slice(&[0, 53, 0xc9, 0x58, 0, 0, 0, 0]);
        packet.extend_from_slice(&payload);
        let total = packet.len() as u16;
        packet[2..4].copy_from_slice(&total.to_be_bytes());
        let parsed = ipv4(&packet).unwrap();
        assert_eq!(parsed.protocol, UDP);
        assert_eq!(parsed.source_port, 53);
        assert_eq!(parsed.destination_port, 51544);
        assert_eq!(parsed.payload, payload.as_slice());
    }

    #[test]
    fn reads_server_name_from_client_hello() {
        let name = b"media.discordapp.net";
        let mut extension = vec![0, 0];
        let list_length = (name.len() + 3) as u16;
        let mut body = list_length.to_be_bytes().to_vec();
        body.push(0);
        body.extend_from_slice(&(name.len() as u16).to_be_bytes());
        body.extend_from_slice(name);
        extension.extend_from_slice(&(body.len() as u16).to_be_bytes());
        extension.extend_from_slice(&body);
        let mut hello = vec![0x03, 0x03];
        hello.extend_from_slice(&[0u8; 32]);
        hello.push(0);
        hello.extend_from_slice(&[0, 2, 0x13, 0x01]);
        hello.extend_from_slice(&[1, 0]);
        hello.extend_from_slice(&(extension.len() as u16).to_be_bytes());
        hello.extend_from_slice(&extension);
        let mut record = vec![0x16, 0x03, 0x01];
        record.extend_from_slice(&((hello.len() + 4) as u16).to_be_bytes());
        record.extend_from_slice(&[0x01, 0, 0, hello.len() as u8]);
        record.extend_from_slice(&hello);
        assert_eq!(server_name(&record).as_deref(), Some("media.discordapp.net"));
        assert_eq!(server_name(b"GET / HTTP/1.1\r\n"), None);
    }

    #[test]
    fn reads_http_request_line_and_host() {
        let (line, host) = http_request(b"GET /index.html HTTP/1.1\r\nHost: Example.com\r\n\r\n").unwrap();
        assert_eq!(line, "GET /index.html HTTP/1.1");
        assert_eq!(host.as_deref(), Some("example.com"));
        assert!(http_request(&[0x16, 0x03, 0x01]).is_none());
    }
}
