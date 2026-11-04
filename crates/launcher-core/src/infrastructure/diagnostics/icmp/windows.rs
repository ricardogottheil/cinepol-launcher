//! `IcmpSendEcho`, which needs no administrator rights (it is what `tracert`
//! and `pathping` use).

use std::ffi::c_void;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    ICMP_ECHO_REPLY, IP_DEST_HOST_UNREACHABLE, IP_DEST_NET_UNREACHABLE, IP_DEST_PORT_UNREACHABLE,
    IP_DEST_PROT_UNREACHABLE, IP_OPTION_INFORMATION, IP_REQ_TIMED_OUT, IP_SUCCESS,
    IP_TTL_EXPIRED_TRANSIT, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
};

use super::payload;
use crate::domain::diagnostics::{HopProbeError, ProbeOutcome};

/// Closes the ICMP handle on every path.
struct IcmpHandle(*mut c_void);

impl Drop for IcmpHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from IcmpCreateFile and is closed once.
        unsafe {
            IcmpCloseHandle(self.0);
        }
    }
}

pub(super) fn probe(
    target: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
) -> Result<ProbeOutcome, HopProbeError> {
    // SAFETY: no arguments; the result is checked below.
    let raw = unsafe { IcmpCreateFile() };
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        // SAFETY: reads the calling thread's last error.
        let code = unsafe { GetLastError() };
        return Err(HopProbeError::Unavailable(format!(
            "IcmpCreateFile failed with error {code}"
        )));
    }
    let handle = IcmpHandle(raw);

    let request = payload(0);
    let options = IP_OPTION_INFORMATION {
        Ttl: ttl,
        Tos: 0,
        Flags: 0,
        OptionsSize: 0,
        OptionsData: std::ptr::null_mut(),
    };
    // Room for one reply, its echoed data and an ICMP error's 8 extra bytes.
    let mut reply = vec![0u8; size_of::<ICMP_ECHO_REPLY>() + request.len() + 8 + 64];
    let millis = u32::try_from(timeout.as_millis())
        .unwrap_or(u32::MAX)
        .max(1);
    let started = Instant::now();
    // SAFETY: every pointer refers to a live buffer of the given size.
    let count = unsafe {
        IcmpSendEcho(
            handle.0,
            u32::from_ne_bytes(target.octets()),
            request.as_ptr().cast(),
            u16::try_from(request.len()).unwrap_or(u16::MAX),
            &raw const options,
            reply.as_mut_ptr().cast(),
            u32::try_from(reply.len()).unwrap_or(u32::MAX),
            millis,
        )
    };
    let rtt = started.elapsed();
    // SAFETY: the buffer is larger than one ICMP_ECHO_REPLY; read it
    // unaligned because a Vec<u8> carries no alignment.
    let echo = unsafe { reply.as_ptr().cast::<ICMP_ECHO_REPLY>().read_unaligned() };
    if count == 0 {
        // SAFETY: reads the calling thread's last error.
        let code = unsafe { GetLastError() };
        // A zero count can still carry a router's answer (a hop's "TTL
        // expired", an unreachable): use the reply when it names that router,
        // as .NET's Ping does, and count the probe as lost otherwise.
        return if code == IP_REQ_TIMED_OUT {
            Ok(ProbeOutcome::TimedOut)
        } else if ROUTER_STATUSES.contains(&code) {
            Ok(if echo.Address != 0 && echo.Status == code {
                outcome(&echo, target, rtt)
            } else {
                ProbeOutcome::TimedOut
            })
        } else {
            Err(HopProbeError::Failed(format!(
                "IcmpSendEcho failed with error {code}"
            )))
        };
    }
    Ok(outcome(&echo, target, rtt))
}

/// The statuses a router on the path answers with. Every other status on a
/// zero count (buffer, resources, hardware) is a local failure and stays an
/// error, so it is never mistaken for loss on every hop.
const ROUTER_STATUSES: [u32; 5] = [
    IP_TTL_EXPIRED_TRANSIT,
    IP_DEST_HOST_UNREACHABLE,
    IP_DEST_NET_UNREACHABLE,
    IP_DEST_PORT_UNREACHABLE,
    IP_DEST_PROT_UNREACHABLE,
];

fn outcome(echo: &ICMP_ECHO_REPLY, target: Ipv4Addr, rtt: Duration) -> ProbeOutcome {
    let from = IpAddr::V4(Ipv4Addr::from(echo.Address.to_ne_bytes()));
    match echo.Status {
        IP_SUCCESS => ProbeOutcome::Reply {
            from,
            rtt,
            reached: true,
        },
        IP_TTL_EXPIRED_TRANSIT => ProbeOutcome::Reply {
            from,
            rtt,
            reached: false,
        },
        IP_DEST_HOST_UNREACHABLE
        | IP_DEST_NET_UNREACHABLE
        | IP_DEST_PORT_UNREACHABLE
        | IP_DEST_PROT_UNREACHABLE => ProbeOutcome::Reply {
            from,
            rtt,
            reached: from == IpAddr::V4(target),
        },
        _ => ProbeOutcome::TimedOut,
    }
}
