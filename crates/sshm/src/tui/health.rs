//! Host reachability probing.
//!
//! Two layers:
//! - [`probe_host`] does a single TCP connect, optionally tries to read the
//!   SSH protocol banner, and returns a populated [`HostStatus`].
//! - The TUI/background worker keep their own cache of last-known statuses
//!   keyed by host name and the timestamp at which each entry was refreshed.

use std::collections::HashSet;
use std::io::Read;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{mpsc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::tui::app::HostStatus;

/// Connect to `host:port` and probe SSH banner.
///
/// `connect_timeout` bounds the TCP handshake. After connect we set a short
/// read timeout (300 ms or `connect_timeout/3`, whichever is smaller, capped
/// at 750ms) and try to read the `SSH-…\r\n` greeting line. If anything goes
/// wrong with banner reading, we still report the host as `Reachable` —
/// just without an `ssh_banner`.
///
/// `connect_timeout` is the whole budget for name resolution plus connecting;
/// every resolved address is tried in turn (an IPv6-first `localhost` must not
/// read as down because the server only listens on IPv4).
pub fn probe_host(host: &str, port: u16, connect_timeout: Duration) -> HostStatus {
    let connect_timeout = connect_timeout.min(Duration::from_secs(60));
    let start = Instant::now();
    let deadline = start + connect_timeout;

    for sock in resolve(host, port, connect_timeout) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let Ok(mut stream) = TcpStream::connect_timeout(&sock, left) else {
            continue;
        };
        let latency_ms = start.elapsed().as_millis().min(u32::MAX as u128) as u32;
        let read_to = std::cmp::min(
            Duration::from_millis(750),
            connect_timeout
                .checked_div(3)
                .unwrap_or(Duration::from_millis(300)),
        );
        let _ = stream.set_read_timeout(Some(read_to));

        // OpenSSH banner: ASCII line ending with \r\n, at most 255 chars per RFC 4253.
        // Read up to 256 bytes — that covers any sane banner without blocking forever.
        let mut buf = [0u8; 256];
        let banner = match stream.read(&mut buf) {
            Ok(n) if n > 0 => parse_ssh_banner(&buf[..n]),
            _ => None,
        };
        return HostStatus::Reachable {
            latency_ms,
            ssh_banner: banner,
        };
    }
    HostStatus::Unreachable
}

/// Hostnames with a `getaddrinfo` still running on an abandoned helper thread.
static DNS_IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// Resolve `host:port`, giving up after `timeout`. `getaddrinfo` has no
/// timeout of its own, so it runs on a helper thread that is abandoned (and
/// exits whenever the resolver finally returns) when the budget runs out.
///
/// At most one lookup per hostname is in flight: while a previous one is
/// still hanging, this resolves to nothing (the host reads as unreachable for
/// this pass) instead of piling up another stuck thread every probe.
fn resolve(host: &str, port: u16, timeout: Duration) -> Vec<SocketAddr> {
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        return vec![SocketAddr::new(ip, port)];
    }
    let host = host.to_string();
    if let Ok(mut set) = DNS_IN_FLIGHT.lock() {
        if !set.insert(host.clone()) {
            return Vec::new();
        }
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let addrs = (host.as_str(), port)
            .to_socket_addrs()
            .map(|a| a.collect::<Vec<_>>());
        if let Ok(mut set) = DNS_IN_FLIGHT.lock() {
            set.remove(&host);
        }
        let _ = tx.send(addrs);
    });
    rx.recv_timeout(timeout)
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default()
}

/// Extract the version token from a raw SSH banner.
///
/// Banner format (RFC 4253): `SSH-protoversion-softwareversion[ comments]\r\n`,
/// e.g. `SSH-2.0-OpenSSH_9.6\r\n`. We return the third hyphen-separated piece
/// (the software version). Returns `None` if the prefix doesn't match.
pub fn parse_ssh_banner(raw: &[u8]) -> Option<String> {
    // Take the first line only (banner is one line).
    let line_end = raw.iter().position(|&b| b == b'\n').unwrap_or(raw.len());
    let line = &raw[..line_end];
    let line = std::str::from_utf8(line)
        .ok()?
        .trim_end_matches('\r')
        .trim();
    if !line.starts_with("SSH-") {
        return None;
    }
    // SSH-2.0-OpenSSH_9.6 [optional comments separated by space]
    // Strip the "SSH-<proto>-" prefix to get "<software> [comments]".
    let after_proto = line.splitn(3, '-').nth(2)?;
    // Software version is up to the first space (comments follow after a space).
    let software = after_proto.split_whitespace().next()?;
    if software.is_empty() {
        None
    } else {
        Some(software.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_ssh_banner;

    #[test]
    fn probes_every_resolved_address() {
        // `localhost` often resolves to ::1 first; an IPv4-only listener must
        // still be found.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let status = super::probe_host("localhost", port, std::time::Duration::from_secs(2));
        assert!(matches!(status, super::HostStatus::Reachable { .. }));
    }

    #[test]
    fn a_hung_lookup_is_not_duplicated() {
        super::DNS_IN_FLIGHT
            .lock()
            .unwrap()
            .insert("stuck.invalid".into());
        let t = std::time::Duration::from_millis(50);
        assert!(super::resolve("stuck.invalid", 22, t).is_empty());
        super::DNS_IN_FLIGHT.lock().unwrap().remove("stuck.invalid");
    }

    #[test]
    fn parses_openssh() {
        assert_eq!(
            parse_ssh_banner(b"SSH-2.0-OpenSSH_9.6\r\n"),
            Some("OpenSSH_9.6".to_string())
        );
    }

    #[test]
    fn parses_with_comments() {
        assert_eq!(
            parse_ssh_banner(b"SSH-2.0-OpenSSH_8.9p1 Ubuntu-3ubuntu0.6\r\n"),
            Some("OpenSSH_8.9p1".to_string())
        );
    }

    #[test]
    fn parses_dropbear() {
        assert_eq!(
            parse_ssh_banner(b"SSH-2.0-dropbear_2022.83\n"),
            Some("dropbear_2022.83".to_string())
        );
    }

    #[test]
    fn rejects_non_ssh() {
        assert_eq!(parse_ssh_banner(b"HTTP/1.1 200 OK\r\n"), None);
        assert_eq!(parse_ssh_banner(b""), None);
        assert_eq!(parse_ssh_banner(b"random garbage"), None);
    }

    #[test]
    fn ignores_trailing_lines() {
        assert_eq!(
            parse_ssh_banner(b"SSH-2.0-OpenSSH_9.6\r\nfollowing data..."),
            Some("OpenSSH_9.6".to_string())
        );
    }
}
