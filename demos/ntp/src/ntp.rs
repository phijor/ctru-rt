// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use alloc::vec::Vec;
use core::fmt;
use core::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use core::time::Duration;

use ctru_rt::{
    debug::log::{error, info, warn},
    os::SystemTick,
    services::soc::{self, Soc, SocketError},
    svc::Timeout,
};

pub use jiff::Timestamp;

struct UdpSocket<'soc>(soc::UdpSocket<'soc>);

impl UdpSocket<'_> {
    /// Discard queued datagrams.
    fn drain(&self) {
        // A buffer large enough to hold a single NTP packet;
        // if anything bigger arrives, it gets truncated (which is fine).
        let mut buf = [0u8; 48];

        // Attempt to drain a small number of datagrams with zero timeout.
        for _ in 0..16 {
            // This only succeeds if anything is queued; in that case try
            // again to see if more datagrams are in the queue.
            match self.0.recv_from_timeout(&mut buf, Timeout::none()) {
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }
}

impl sntpc::NtpUdpSocket for UdpSocket<'_> {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> sntpc::Result<usize> {
        self.0.send_to(buf, &addr).map_err(|err| {
            error!("send_to failed: {err:?}");
            sntpc::Error::Network
        })
    }

    async fn recv_from(&self, buf: &mut [u8]) -> sntpc::Result<(usize, SocketAddr)> {
        const TIMEOUT: Timeout = Timeout::from_seconds(5);
        self.0.recv_from_timeout(buf, TIMEOUT).map_err(|err| {
            error!("recv_from failed: {err:?}");
            sntpc::Error::Network
        })
    }
}

#[derive(Default, Clone, Copy)]
struct TimestampGenerator {
    time_since_boot: Duration,
}

impl sntpc::NtpTimestampGenerator for TimestampGenerator {
    fn init(&mut self) {
        self.time_since_boot = SystemTick::now().to_duration();
    }

    fn timestamp_sec(&self) -> u64 {
        self.time_since_boot.as_secs()
    }

    fn timestamp_subsec_micros(&self) -> u32 {
        self.time_since_boot.subsec_micros()
    }
}

pub struct Ntp<'soc> {
    socket: UdpSocket<'soc>,
    server_addrs: Vec<SocketAddr>,
    context: sntpc::NtpContext<TimestampGenerator>,
}

#[derive(Debug)]
pub enum Error {
    SocketError(SocketError),
    NameLookupFailed,
    NoSuccess,
    InvalidTimestamp(jiff::Error),
}

impl From<SocketError> for Error {
    fn from(err: SocketError) -> Self {
        Error::SocketError(err)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::SocketError(socket_error) => {
                write!(f, "Socket operation failed: {socket_error:?}")
            }
            Error::NameLookupFailed => write!(f, "Failed to lookup name of time server"),
            Error::NoSuccess => write!(f, "No requests to time servers succeeded"),
            Error::InvalidTimestamp(err) => write!(f, "Server returned invalid timestamp: {err}"),
        }
    }
}

impl<'soc> Ntp<'soc> {
    pub fn init(soc: &'soc Soc) -> Result<Self, Error> {
        const LOCAL: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0));
        let socket = UdpSocket(soc::UdpSocket::bind(soc, &LOCAL)?);
        let context = sntpc::NtpContext::new(TimestampGenerator::default());

        let server_addrs = soc.lookup(c"pool.ntp.org", 123)?;
        if server_addrs.is_empty() {
            error!("Failed to look up time server address");
            return Err(Error::NameLookupFailed);
        }

        info!("Found time servers:");
        for addr in &server_addrs {
            info!("{addr}");
        }

        Ok(Self {
            socket,
            server_addrs,
            context,
        })
    }

    pub fn get_time(&self) -> Result<Timestamp, Error> {
        for addr in &self.server_addrs {
            info!("Requesting time from {addr}...");
            // Ensure that there are no packets queued from an earlier (failed) call.
            self.socket.drain();
            match sntpc::sync::get_time(*addr, &self.socket, self.context) {
                Ok(result) => {
                    return Timestamp::from_second(result.seconds as i64)
                        .map_err(Error::InvalidTimestamp);
                }
                Err(err) => warn!("Request to {addr} failed: {err:?}"),
            }
        }

        Err(Error::NoSuccess)
    }
}
