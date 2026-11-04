//! Unprivileged ICMP sockets (`SOCK_DGRAM` + `IPPROTO_ICMP`): always allowed
//! on macOS; on Linux allowed for the groups in `net.ipv4.ping_group_range`
//! (every user on systemd systems). Linux delivers "time exceeded" through
//! the socket's error queue instead of a normal read.

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};

use super::{Reply, echo_request, parse_reply};
use crate::domain::diagnostics::{HopProbeError, ProbeOutcome};

pub(super) fn probe(
    target: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
    ident: u16,
    sequence: u16,
) -> Result<ProbeOutcome, HopProbeError> {
    let socket =
        Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).map_err(|error| {
            HopProbeError::Unavailable(format!("the system refused an ICMP socket: {error}"))
        })?;
    socket
        .set_ttl_v4(u32::from(ttl))
        .map_err(|error| HopProbeError::Failed(format!("setting the TTL: {error}")))?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    linux::receive_errors(&socket)?;
    let socket: UdpSocket = socket.into();

    let started = Instant::now();
    let destination = SocketAddr::V4(SocketAddrV4::new(target, 0));
    socket
        .send_to(&echo_request(ident, sequence), destination)
        .map_err(|error| HopProbeError::Failed(format!("sending the probe: {error}")))?;

    let deadline = started + timeout;
    let mut buffer = [0u8; 1500];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(ProbeOutcome::TimedOut);
        }
        socket
            .set_read_timeout(Some(remaining))
            .map_err(|error| HopProbeError::Failed(error.to_string()))?;
        match socket.recv_from(&mut buffer) {
            Ok((length, from)) => {
                if let Some(reply) = parse_reply(&buffer[..length], sequence) {
                    return Ok(outcome(from.ip(), started.elapsed(), reply, target));
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Ok(ProbeOutcome::TimedOut);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => {
                #[cfg(any(target_os = "linux", target_os = "android"))]
                if let Some(router) = linux::read_error(&socket, sequence) {
                    return Ok(outcome(router, started.elapsed(), Reply::Router, target));
                }
                return Err(HopProbeError::Failed(format!("reading the reply: {error}")));
            }
        }
    }
}

fn outcome(from: IpAddr, rtt: Duration, reply: Reply, target: Ipv4Addr) -> ProbeOutcome {
    ProbeOutcome::Reply {
        from,
        rtt,
        reached: reply == Reply::Echo || from == IpAddr::V4(target),
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod linux {
    use std::mem::{size_of, zeroed};
    use std::net::{IpAddr, Ipv4Addr, UdpSocket};
    use std::os::fd::AsRawFd;

    use socket2::Socket;

    use crate::domain::diagnostics::HopProbeError;

    /// Asks the kernel to queue ICMP errors (time exceeded) for this socket.
    pub(super) fn receive_errors(socket: &Socket) -> Result<(), HopProbeError> {
        let enable: libc::c_int = 1;
        // SAFETY: a valid socket descriptor and a c_int option value that
        // lives for the duration of the call.
        let result = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::IPPROTO_IP,
                libc::IP_RECVERR,
                (&raw const enable).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(HopProbeError::Failed(format!(
                "enabling IP_RECVERR: {}",
                std::io::Error::last_os_error()
            )))
        }
    }

    /// Reads one queued ICMP error and returns the router that sent it, when
    /// it is about probe `sequence` (the queue holds the probe it bounced).
    pub(super) fn read_error(socket: &UdpSocket, sequence: u16) -> Option<IpAddr> {
        /// Control messages hold `cmsghdr`s, which need pointer alignment.
        #[repr(C, align(8))]
        struct Control([u8; 512]);

        let mut data = [0u8; 576];
        let mut control = Control([0u8; 512]);
        let mut iov = libc::iovec {
            iov_base: data.as_mut_ptr().cast(),
            iov_len: data.len(),
        };
        // SAFETY: an all-zero msghdr is a valid empty header.
        let mut message: libc::msghdr = unsafe { zeroed() };
        message.msg_iov = &raw mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.0.as_mut_ptr().cast();
        message.msg_controllen = control.0.len() as _;
        // SAFETY: the header points to buffers that outlive the call.
        let length = unsafe {
            libc::recvmsg(
                socket.as_raw_fd(),
                &raw mut message,
                libc::MSG_ERRQUEUE | libc::MSG_DONTWAIT,
            )
        };
        let length = usize::try_from(length).ok()?;
        let bounced = data.get(..length)?;
        let bounced_sequence = u16::from_be_bytes([*bounced.get(6)?, *bounced.get(7)?]);
        if bounced_sequence != sequence {
            return None;
        }
        // The error and, right after it, the offender's address (SO_EE_OFFENDER).
        let needed = size_of::<libc::sock_extended_err>() + size_of::<libc::sockaddr_in>();
        // SAFETY: CMSG_* walk the aligned control buffer the kernel just
        // filled, bounded by msg_controllen. The payload is read only when
        // cmsg_len covers it, and with read_unaligned, since CMSG_DATA only
        // promises the alignment of cmsghdr.
        unsafe {
            let mut header = libc::CMSG_FIRSTHDR(&raw const message);
            while !header.is_null() {
                if (*header).cmsg_level == libc::IPPROTO_IP
                    && (*header).cmsg_type == libc::IP_RECVERR
                {
                    if ((*header).cmsg_len as usize) < libc::CMSG_LEN(needed as u32) as usize {
                        return None;
                    }
                    let data = libc::CMSG_DATA(header);
                    let error = data.cast::<libc::sock_extended_err>().read_unaligned();
                    if error.ee_origin != libc::SO_EE_ORIGIN_ICMP {
                        return None;
                    }
                    let offender = data
                        .add(size_of::<libc::sock_extended_err>())
                        .cast::<libc::sockaddr_in>()
                        .read_unaligned();
                    if libc::c_int::from(offender.sin_family) != libc::AF_INET {
                        return None;
                    }
                    let address = u32::from_be(offender.sin_addr.s_addr);
                    return Some(IpAddr::V4(Ipv4Addr::from(address)));
                }
                header = libc::CMSG_NXTHDR(&raw const message, header);
            }
        }
        None
    }
}
