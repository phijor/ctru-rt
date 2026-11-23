// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::net::SocketAddr;

use super::{AddressFamily, Protocol, Result, Soc, Socket, SocketError, Type};

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

    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        let (bytes_read, Some(peer_addr)) = self.socket.recvfrom(0, buf)? else {
            return Err(SocketError::MissingAddress);
        };

        Ok((bytes_read, peer_addr))
    }

    pub fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        self.socket.recv(0, buf)
    }

    pub fn send_to(&self, buf: &[u8], dest: &SocketAddr) -> Result<usize> {
        self.socket.sendto(buf, 0, Some(dest))
    }

    pub fn send(&self, buf: &[u8]) -> Result<usize> {
        self.socket.sendto(buf, 0, None)
    }
}
