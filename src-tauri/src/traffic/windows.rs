use std::collections::HashMap;
use std::ffi::c_void;
use std::io::ErrorKind;
use std::mem::MaybeUninit;
use std::net::SocketAddrV4;
use std::os::windows::io::AsRawSocket;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCPROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    UDP_TABLE_OWNER_PID,
};
use windows_sys::Win32::Networking::WinSock::{WSAIoctl, RCVALL_ON, SIO_RCVALL};
use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};

use super::parse::{self, TCP, UDP};
use super::{lock, local_address, Monitor, Status};
use crate::health::now_ms;

const ACCESS_DENIED: i32 = 10013;
const AF_INET: u32 = 2;
const NETWORK_CHECK: Duration = Duration::from_secs(10);

fn refused(error: &std::io::Error) -> Status {
    if error.raw_os_error() == Some(ACCESS_DENIED) {
        Status::NeedsAdmin
    } else {
        crate::log::warn(format!("traffic capture: {error}"));
        Status::Failed
    }
}

pub fn capture(shared: &Arc<Mutex<Monitor>>) -> Status {
    let Some(local) = local_address() else {
        return Status::Failed;
    };
    let socket = match Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::from(0))) {
        Ok(socket) => socket,
        Err(error) => return refused(&error),
    };
    if let Err(error) = socket.bind(&SocketAddrV4::new(local, 0).into()) {
        return refused(&error);
    }
    let on: i32 = RCVALL_ON;
    let mut returned = 0u32;
    let result = unsafe {
        WSAIoctl(
            socket.as_raw_socket() as usize,
            SIO_RCVALL,
            (&on as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
            None,
        )
    };
    if result != 0 {
        return refused(&std::io::Error::last_os_error());
    }
    let _ = socket.set_read_timeout(Some(Duration::from_millis(500)));
    lock(shared).running(local);
    crate::log::write("traffic capture running");

    let mut buffer = vec![MaybeUninit::<u8>::uninit(); 65_536];
    let mut checked = Instant::now();
    loop {
        match socket.recv(&mut buffer) {
            Ok(size) => {
                let data = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), size) };
                if let Some(packet) = parse::ipv4(data) {
                    lock(shared).observe(&packet, now_ms());
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => return refused(&error),
        }
        if checked.elapsed() >= NETWORK_CHECK {
            checked = Instant::now();
            if local_address() != Some(local) {
                crate::log::write("network changed, restarting traffic capture");
                return Status::Starting;
            }
        }
    }
}

fn read_table<R: Copy>(fetch: impl Fn(*mut c_void, *mut u32) -> u32, row: impl Fn(&R) -> (u32, u32)) -> Vec<(u16, u32)> {
    let mut size = 0u32;
    fetch(std::ptr::null_mut(), &mut size);
    for _ in 0..3 {
        let mut buffer = vec![0u32; size as usize / 4 + 64];
        let mut length = (buffer.len() * 4) as u32;
        if fetch(buffer.as_mut_ptr().cast(), &mut length) == 0 {
            let capacity = (buffer.len() * 4 - 4) / std::mem::size_of::<R>();
            let count = (buffer[0] as usize).min(capacity);
            let rows = unsafe { std::slice::from_raw_parts(buffer.as_ptr().add(1).cast::<R>(), count) };
            return rows
                .iter()
                .map(|r| {
                    let (port, pid) = row(r);
                    (u16::from_be(port as u16), pid)
                })
                .collect();
        }
        size = length;
    }
    Vec::new()
}

pub fn owners() -> HashMap<(u8, u16), u32> {
    let tcp = read_table::<MIB_TCPROW_OWNER_PID>(
        |buffer, size| unsafe { GetExtendedTcpTable(buffer, size, 0, AF_INET, TCP_TABLE_OWNER_PID_ALL, 0) },
        |r| (r.dwLocalPort, r.dwOwningPid),
    );
    let udp = read_table::<MIB_UDPROW_OWNER_PID>(
        |buffer, size| unsafe { GetExtendedUdpTable(buffer, size, 0, AF_INET, UDP_TABLE_OWNER_PID, 0) },
        |r| (r.dwLocalPort, r.dwOwningPid),
    );
    let mut map = HashMap::with_capacity(tcp.len() + udp.len());
    for (port, pid) in tcp {
        if pid != 0 {
            map.insert((TCP, port), pid);
        }
    }
    for (port, pid) in udp {
        if pid != 0 {
            map.insert((UDP, port), pid);
        }
    }
    map
}

pub fn process(pid: u32) -> Option<(String, String)> {
    if pid == 4 {
        return Some(("System".into(), String::new()));
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        let name = Path::new(&path).file_stem()?.to_string_lossy().into_owned();
        Some((name, path))
    }
}
