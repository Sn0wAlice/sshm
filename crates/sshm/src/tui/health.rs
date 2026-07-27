//! Host reachability probing.
//!
//! Two layers:
//! - [`probe_host`] does a single TCP connect, optionally tries to read the
//!   SSH protocol banner, and returns a populated [`HostStatus`].
//! - The TUI/background worker keep their own cache of last-known statuses
//!   keyed by host name and the timestamp at which each entry was refreshed.

use std::io::Read;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::tui::app::HostStatus;

/// Connect to `host:port` and probe SSH banner.
///
/// `connect_timeout` bounds the TCP handshake. After connect we set a short
/// read timeout (300 ms or `connect_timeout/3`, whichever is smaller, capped
/// at 750ms) and try to read the `SSH-…\r\n` greeting line. If anything goes
/// wrong with banner reading, we still report the host as `Reachable` —
/// just without an `ssh_banner`.
pub fn probe_host(host: &str, port: u16, connect_timeout: Duration) -> HostStatus {
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

    match TcpStream::connect_timeout(&sock, connect_timeout) {
        Ok(mut stream) => {
            let latency_ms = start.elapsed().as_millis().min(u32::MAX as u128) as u32;
            let read_to = std::cmp::min(
                Duration::from_millis(750),
                connect_timeout.checked_div(3).unwrap_or(Duration::from_millis(300)),
            );
            let _ = stream.set_read_timeout(Some(read_to));

            // OpenSSH banner: ASCII line ending with \r\n, at most 255 chars per RFC 4253.
            // Read up to 256 bytes — that covers any sane banner without blocking forever.
            let mut buf = [0u8; 256];
            let banner = match stream.read(&mut buf) {
                Ok(n) if n > 0 => parse_ssh_banner(&buf[..n]),
                _ => None,
            };
            HostStatus::Reachable { latency_ms, ssh_banner: banner }
        }
        Err(_) => HostStatus::Unreachable,
    }
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
    let line = std::str::from_utf8(line).ok()?.trim_end_matches('\r').trim();
    if !line.starts_with("SSH-") { return None; }
    // SSH-2.0-OpenSSH_9.6 [optional comments separated by space]
    // Strip the "SSH-<proto>-" prefix to get "<software> [comments]".
    let after_proto = line.splitn(3, '-').nth(2)?;
    // Software version is up to the first space (comments follow after a space).
    let software = after_proto.split_whitespace().next()?;
    if software.is_empty() { None } else { Some(software.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::parse_ssh_banner;

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
