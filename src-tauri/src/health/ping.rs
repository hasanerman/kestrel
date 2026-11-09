use std::io::ErrorKind;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::net::TcpStream;

const TIMEOUT: Duration = Duration::from_millis(1500);
const UNKNOWN: u8 = 0;
const WORKS: u8 = 1;
const BLOCKED: u8 = 2;

static ICMP: AtomicU8 = AtomicU8::new(UNKNOWN);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Icmp,
    Tcp,
}

pub fn method() -> Method {
    if ICMP.load(Ordering::Relaxed) == BLOCKED {
        Method::Tcp
    } else {
        Method::Icmp
    }
}

pub async fn measure(ip: Ipv4Addr, tcp_port: u16) -> Option<u32> {
    if ICMP.load(Ordering::Relaxed) != BLOCKED {
        let answer = tokio::task::spawn_blocking(move || icmp::echo(ip, TIMEOUT)).await;
        match answer {
            Ok(Ok(rtt)) => {
                if ICMP.swap(WORKS, Ordering::Relaxed) == UNKNOWN {
                    crate::log::write("measuring with ICMP");
                }
                return rtt;
            }
            _ => {
                if ICMP.swap(BLOCKED, Ordering::Relaxed) != BLOCKED {
                    crate::log::warn("ICMP is not available, measuring with TCP connect time");
                }
            }
        }
    }
    tcp(ip, tcp_port).await
}

async fn tcp(ip: Ipv4Addr, port: u16) -> Option<u32> {
    let start = Instant::now();
    let elapsed = || (start.elapsed().as_millis() as u32).max(1);
    match tokio::time::timeout(TIMEOUT, TcpStream::connect((ip, port))).await {
        Ok(Ok(_)) => Some(elapsed()),
        Ok(Err(err)) if err.kind() == ErrorKind::ConnectionRefused => Some(elapsed()),
        _ => None,
    }
}

#[cfg(windows)]
mod icmp {
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::NetworkManagement::IpHelper::{IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY};

    const PAYLOAD: &[u8; 32] = b"kestrel-connection-health-probe!";
    const SUCCESS: u32 = 0;

    pub struct Unavailable;

    pub fn echo(ip: Ipv4Addr, timeout: Duration) -> Result<Option<u32>, Unavailable> {
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + PAYLOAD.len() + 64];
        unsafe {
            let handle = IcmpCreateFile();
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return Err(Unavailable);
            }
            let count = IcmpSendEcho(
                handle,
                u32::from_ne_bytes(ip.octets()),
                PAYLOAD.as_ptr().cast(),
                PAYLOAD.len() as u16,
                std::ptr::null(),
                reply.as_mut_ptr().cast(),
                reply.len() as u32,
                timeout.as_millis() as u32,
            );
            IcmpCloseHandle(handle);
            if count == 0 {
                return Ok(None);
            }
            let answer = std::ptr::read_unaligned(reply.as_ptr().cast::<ICMP_ECHO_REPLY>());
            Ok((answer.Status == SUCCESS).then(|| answer.RoundTripTime.max(1)))
        }
    }
}

#[cfg(unix)]
mod icmp {
    use std::mem::MaybeUninit;
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::time::{Duration, Instant};

    use socket2::{Domain, Protocol, SockAddr, Socket, Type};

    const ECHO_REQUEST: u8 = 8;
    const ECHO_REPLY: u8 = 0;

    static SEQUENCE: AtomicU16 = AtomicU16::new(1);

    pub struct Unavailable;

    pub fn echo(ip: Ipv4Addr, timeout: Duration) -> Result<Option<u32>, Unavailable> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).map_err(|_| Unavailable)?;
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed).to_be_bytes();
        let mut packet = [0u8; 16];
        packet[0] = ECHO_REQUEST;
        packet[6..8].copy_from_slice(&sequence);
        packet[8..].copy_from_slice(b"kestrel!");
        let sum = super::checksum(&packet);
        packet[2..4].copy_from_slice(&sum.to_be_bytes());

        let start = Instant::now();
        socket
            .send_to(&packet, &SockAddr::from(SocketAddrV4::new(ip, 0)))
            .map_err(|_| Unavailable)?;
        let mut buffer = [MaybeUninit::<u8>::uninit(); 256];
        loop {
            let Some(left) = timeout.checked_sub(start.elapsed()).filter(|d| !d.is_zero()) else {
                return Ok(None);
            };
            if socket.set_read_timeout(Some(left)).is_err() {
                return Ok(None);
            }
            let Ok(size) = socket.recv(&mut buffer) else {
                return Ok(None);
            };
            let data: Vec<u8> = buffer[..size].iter().map(|b| unsafe { b.assume_init() }).collect();
            let message = super::strip_ip_header(&data);
            if message.len() >= 8 && message[0] == ECHO_REPLY && message[6..8] == sequence {
                return Ok(Some((start.elapsed().as_millis() as u32).max(1)));
            }
        }
    }
}

#[cfg(any(unix, test))]
fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = data
        .chunks(2)
        .map(|pair| u32::from(u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)])))
        .sum();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(any(unix, test))]
fn strip_ip_header(data: &[u8]) -> &[u8] {
    if data.len() >= 20 && data[0] >> 4 == 4 {
        let header = usize::from(data[0] & 0x0f) * 4;
        return data.get(header..).unwrap_or(&[]);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_of_echo_request() {
        let packet = [8, 0, 0, 0, 0, 1, 0, 1];
        assert_eq!(checksum(&packet), 0xf7fd);
    }

    #[test]
    fn strips_ipv4_header_only_when_present() {
        let mut with_header = vec![0x45; 20];
        with_header.extend_from_slice(&[0, 0, 1, 2]);
        assert_eq!(strip_ip_header(&with_header), &[0, 0, 1, 2]);
        assert_eq!(strip_ip_header(&[0, 0, 1, 2]), &[0, 0, 1, 2]);
    }
}
