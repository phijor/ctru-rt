// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::net::SocketAddr;

use crate::svc::Timeout;

use super::{
    AddressFamily, Flags, PollEvents, Protocol, Result, Soc, Socket, SocketError, SocketErrorCode,
    Type,
};

pub struct UdpSocket<'soc> {
    socket: Socket<'soc>,
}

impl<'soc> UdpSocket<'soc> {
    pub fn bind(soc: &'soc Soc, addr: &SocketAddr) -> Result<Self> {
        let family = AddressFamily::from_addr(addr);

        let socket = soc.socket(family, Type::Datagram, Protocol::Default)?;
        socket.bind(addr)?;

        Ok(Self { socket })
    }
}

impl UdpSocket<'_> {
    pub fn connect(&self, dest: &SocketAddr) -> Result<()> {
        self.socket.connect(dest)
    }

    fn recv_from_with_flags(&self, flags: Flags, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        let (bytes_read, Some(peer_addr)) = self.socket.recvfrom(flags, buf)? else {
            return Err(SocketError::MissingAddress);
        };

        Ok((bytes_read, peer_addr))
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        self.recv_from_with_flags(Flags::NONE, buf)
    }

    pub fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        self.socket.recv(Flags::NONE, buf)
    }

    /// Try to receive data within the given timeout.
    ///
    /// The timeout is rounded up to whole milliseconds.
    /// [`Timeout::none`] attempts to read immediately,
    /// [`Timeout::infinite`] waits indefinitely.
    ///
    /// Returns [`SocketErr(ETIMEDOUT)`][SocketErrorCode::ETIMEDOUT] if no data arrived in time.
    /// Returns [`SocketErr(EAGAIN)`][SocketErrorCode::EAGAIN] if the socket was signaled ready but no data could be read.
    pub fn recv_from_timeout(
        &self,
        buf: &mut [u8],
        timeout: Timeout,
    ) -> Result<(usize, SocketAddr)> {
        let events = self.socket.poll(PollEvents::IN, timeout)?;

        // The call to poll timed out.
        if events.is_empty() {
            return Err(SocketError::SocketErr(SocketErrorCode::ETIMEDOUT));
        }

        // The socket handle is invalid (should not happen).
        if events.contains(PollEvents::NVAL) {
            return Err(SocketError::SocketErr(SocketErrorCode::EBADF));
        }

        // A packet should be ready. Try to receive data without blocking.
        self.recv_from_with_flags(Flags::DONTWAIT, buf)
    }

    pub fn send_to(&self, buf: &[u8], dest: &SocketAddr) -> Result<usize> {
        self.socket.sendto(buf, Flags::NONE, Some(dest))
    }

    pub fn send(&self, buf: &[u8]) -> Result<usize> {
        self.socket.sendto(buf, Flags::NONE, None)
    }
}
