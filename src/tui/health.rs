//! Host reachability probing.
//!
//! Shared between the `c` hotkey (manual check) and the background worker
//! thread that auto-refreshes reachability/latency for the host list.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::tui::app::HostStatus;

/// TCP-connect to `host:port` with the given timeout, measure the
/// round-trip, and return a populated [`HostStatus`].
pub fn probe_host(host: &str, port: u16, timeout: Duration) -> HostStatus {
    let addr = format!("{}:{}", host, port);
    let start = Instant::now();

    let sock: SocketAddr = match addr.parse::<SocketAddr>() {
        Ok(s) => s,
        Err(_) => match addr.to_socket_addrs() {
            Ok(mut addrs) => match addrs.next() {
                Some(s) => s,
                None => return HostStatus::Unreachable,
            },
            Err(_) => return HostStatus::Unreachable,
        },
    };

    match TcpStream::connect_timeout(&sock, timeout) {
        Ok(_) => {
            let latency_ms = start.elapsed().as_millis().min(u32::MAX as u128) as u32;
            HostStatus::Reachable { latency_ms }
        }
        Err(_) => HostStatus::Unreachable,
    }
}
