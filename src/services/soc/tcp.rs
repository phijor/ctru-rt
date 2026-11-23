// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::net::SocketAddr;

use super::{AddressFamily, Flags, Protocol, Result, Soc, Socket, SocketError, Type};

#[derive(Debug)]
pub struct TcpListener<'soc> {
    listener: Socket<'soc>,
}

impl<'soc> TcpListener<'soc> {
    pub fn bind(soc: &'soc Soc, addr: &SocketAddr, backlog: u32) -> Result<Self> {
        let family = AddressFamily::from_addr(addr);

        let listener = soc.socket(family, Type::Stream, Protocol::Default)?;
        listener.bind(addr)?;
        listener.listen(backlog)?;

        Ok(Self { listener })
    }

    /// Accept a connection from a peer.
    ///
    /// On success, returns a [`TcpStream`] for the peer together with its address.
    /// Returns [`SocketError::MissingAddress`] if the system did not report a peer address.
    pub fn accept(&self) -> Result<(TcpStream<'soc>, SocketAddr)> {
        let (peer, Some(peer_addr)) = self.listener.accept()? else {
            return Err(SocketError::MissingAddress);
        };

        Ok((TcpStream { peer }, peer_addr))
    }

    pub fn close(self) -> Result<()> {
        self.listener.close()
    }
}

#[derive(Debug)]
pub struct TcpStream<'soc> {
    peer: Socket<'soc>,
}

impl<'soc> TcpStream<'soc> {
    pub fn connect(soc: &'soc Soc, addr: &SocketAddr) -> Result<Self> {
        let family = AddressFamily::from_addr(addr);
        let peer = soc.socket(family, Type::Stream, Protocol::Default)?;

        peer.connect(addr)?;

        Ok(Self { peer })
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.peer.recv(Flags::NONE, buf)
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        self.peer.sendto(buf, Flags::NONE, None)
    }

    /// Write all of `buf`, calling [`write`](Self::write) until it has been sent.
    ///
    /// Fails with [`SocketError::WriteZero`] if a write sends no bytes.
    pub fn write_all(&self, buf: &[u8]) -> Result<()> {
        let mut remaining = buf;
        while !remaining.is_empty() {
            let bytes_written = self.write(remaining)?;
            if bytes_written == 0 {
                return Err(SocketError::WriteZero);
            }

            remaining = &remaining[bytes_written..];
        }

        Ok(())
    }

    pub fn close(self) -> Result<()> {
        self.peer.close()
    }

    // later: peer_addr(), shutdown(Shutdown)
}
