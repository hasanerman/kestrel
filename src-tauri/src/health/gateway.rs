use std::net::Ipv4Addr;

pub fn find() -> Option<Ipv4Addr> {
    platform::find().filter(|ip| !ip.is_unspecified() && !ip.is_broadcast())
}

#[cfg(windows)]
mod platform {
    use std::net::Ipv4Addr;

    use windows_sys::Win32::NetworkManagement::IpHelper::{GetBestRoute, MIB_IPFORWARDROW};

    pub fn find() -> Option<Ipv4Addr> {
        let mut row: MIB_IPFORWARDROW = unsafe { std::mem::zeroed() };
        let target = u32::from_ne_bytes([1, 1, 1, 1]);
        if unsafe { GetBestRoute(target, 0, &mut row) } != 0 {
            return None;
        }
        Some(Ipv4Addr::from(row.dwForwardNextHop.to_ne_bytes()))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::net::Ipv4Addr;

    pub fn find() -> Option<Ipv4Addr> {
        super::parse_proc_route(&std::fs::read_to_string("/proc/net/route").ok()?)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::net::Ipv4Addr;

    pub fn find() -> Option<Ipv4Addr> {
        let output = crate::system::run("route", &["-n", "get", "default"]).ok()?;
        super::parse_route_get(&String::from_utf8_lossy(&output.stdout))
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use std::net::Ipv4Addr;

    pub fn find() -> Option<Ipv4Addr> {
        None
    }
}

#[cfg(any(target_os = "linux", test))]
fn parse_proc_route(table: &str) -> Option<Ipv4Addr> {
    table.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.get(1) != Some(&"00000000") {
            return None;
        }
        let raw = u32::from_str_radix(fields.get(2)?, 16).ok()?;
        Some(Ipv4Addr::from(raw.to_le_bytes()))
    })
}

#[cfg(any(target_os = "macos", test))]
fn parse_route_get(output: &str) -> Option<Ipv4Addr> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("gateway:"))
        .and_then(|value| value.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_linux_default_route() {
        let table = "Iface\tDestination\tGateway\tFlags\n\
                     wlan0\t0000A8C0\t00000000\t0001\n\
                     wlan0\t00000000\t0101A8C0\t0003\n";
        assert_eq!(parse_proc_route(table), Some(Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn reads_macos_default_route() {
        let output = "   route to: default\ndestination: default\n    gateway: 10.0.0.1\n  interface: en0\n";
        assert_eq!(parse_route_get(output), Some(Ipv4Addr::new(10, 0, 0, 1)));
    }
}
