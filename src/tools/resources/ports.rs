//! Which process owns each network port (TCP and UDP, IPv4 and IPv6).

use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

#[derive(Debug, Clone, PartialEq)]
pub struct Port {
    pub proto: &'static str,
    pub local: IpAddr,
    pub port: u16,
    /// The other end, for a TCP connection.
    pub remote: Option<(IpAddr, u16)>,
    pub state: &'static str,
    pub pid: u32,
}

/// Every TCP socket and UDP port on the PC.
pub fn list() -> Vec<Port> {
    let mut out = Vec::new();
    for af in [AF_INET, AF_INET6] {
        let tcp = table(|buf, size| unsafe {
            GetExtendedTcpTable(buf, size, 0, af, TCP_TABLE_OWNER_PID_ALL, 0)
        });
        let udp = table(|buf, size| unsafe {
            GetExtendedUdpTable(buf, size, 0, af, UDP_TABLE_OWNER_PID, 0)
        });
        if af == AF_INET {
            for r in rows::<MIB_TCPROW_OWNER_PID>(&tcp) {
                out.push(tcp_port(
                    v4(r.dwLocalAddr),
                    r.dwLocalPort,
                    v4(r.dwRemoteAddr),
                    r.dwRemotePort,
                    r.dwState,
                    r.dwOwningPid,
                ));
            }
            for r in rows::<MIB_UDPROW_OWNER_PID>(&udp) {
                out.push(udp_port(v4(r.dwLocalAddr), r.dwLocalPort, r.dwOwningPid));
            }
        } else {
            for r in rows::<MIB_TCP6ROW_OWNER_PID>(&tcp) {
                out.push(tcp_port(
                    v6(r.ucLocalAddr),
                    r.dwLocalPort,
                    v6(r.ucRemoteAddr),
                    r.dwRemotePort,
                    r.dwState,
                    r.dwOwningPid,
                ));
            }
            for r in rows::<MIB_UDP6ROW_OWNER_PID>(&udp) {
                out.push(udp_port(v6(r.ucLocalAddr), r.dwLocalPort, r.dwOwningPid));
            }
        }
    }
    out
}

fn tcp_port(local: IpAddr, lport: u32, remote: IpAddr, rport: u32, state: u32, pid: u32) -> Port {
    let state = tcp_state(state);
    Port {
        proto: "TCP",
        local,
        port: port(lport),
        remote: (state != "listening").then_some((remote, port(rport))),
        state,
        pid,
    }
}

fn udp_port(local: IpAddr, lport: u32, pid: u32) -> Port {
    Port {
        proto: "UDP",
        local,
        port: port(lport),
        remote: None,
        state: "open",
        pid,
    }
}

/// Ports are stored in network byte order in the low 16 bits.
fn port(raw: u32) -> u16 {
    u16::from_be(raw as u16)
}

/// Addresses are stored in network byte order, so their bytes are in order in memory.
fn v4(raw: u32) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from(raw.to_ne_bytes()))
}

fn v6(raw: [u8; 16]) -> IpAddr {
    IpAddr::V6(Ipv6Addr::from(raw))
}

fn tcp_state(s: u32) -> &'static str {
    match s {
        2 => "listening",
        3 | 4 => "connecting",
        5 => "connected",
        1 | 12 => "closed",
        _ => "closing",
    }
}

/// Calls an IP Helper "get table" function until the buffer is big enough.
/// The buffer is u32s so the rows are aligned.
fn table(get: impl Fn(*mut c_void, *mut u32) -> u32) -> Vec<u32> {
    let mut size = 0u32;
    let mut buf: Vec<u32> = Vec::new();
    for _ in 0..5 {
        buf.resize((size as usize).div_ceil(4).max(1), 0);
        size = (buf.len() * 4) as u32;
        match get(buf.as_mut_ptr().cast(), &mut size) {
            0 => return buf,
            ERROR_INSUFFICIENT_BUFFER => size += 4096, // it can grow between calls
            _ => break,
        }
    }
    Vec::new()
}

/// The rows after the table's leading entry count.
fn rows<T: Copy>(buf: &[u32]) -> Vec<T> {
    let Some(&n) = buf.first() else {
        return Vec::new();
    };
    let start = buf[1..].as_ptr() as *const u8;
    let fits = (buf.len() - 1) * 4 / size_of::<T>();
    (0..(n as usize).min(fits))
        .map(|i| unsafe { std::ptr::read_unaligned((start as *const T).add(i)) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_network_byte_order() {
        // Port 8000 is 0x1F40, stored as the bytes 1F 40.
        assert_eq!(port(u32::from_ne_bytes([0x1f, 0x40, 0, 0])), 8000);
        assert_eq!(
            v4(u32::from_ne_bytes([127, 0, 0, 1])).to_string(),
            "127.0.0.1"
        );
    }

    #[test]
    fn finds_a_port_we_listen_on() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let p = l.local_addr().unwrap().port();
        let found = list()
            .into_iter()
            .find(|x| x.proto == "TCP" && x.port == p && x.state == "listening")
            .expect("our listening port");
        assert_eq!(found.pid, std::process::id());
        assert_eq!(found.remote, None);
    }
}
