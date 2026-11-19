// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::{Result, Soc};
use crate::ipc::{CommandBufferWriter, Parameter};

use core::mem::ManuallyDrop;
use core::net::SocketAddr;

use log::{debug, warn};

#[derive(Debug)]
pub struct Socket<'soc> {
    fd: u32,
    soc: &'soc Soc,
}

impl<'soc> Socket<'soc> {
    pub(super) const fn new(fd: u32, soc: &'soc Soc) -> Self {
        Self { fd, soc }
    }
}

impl Drop for Socket<'_> {
    fn drop(&mut self) {
        let Self { fd, soc } = self;

        debug!("Closing socket: fd={fd}");
        if let Err(err) = soc.close_raw(*fd) {
            warn!("Failed to close socket (fd={fd}): {err:0x?}")
        }
    }
}

impl Parameter for &Socket<'_> {
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(self.fd)
    }
}

impl Socket<'_> {
    pub fn listen(&self, backlog: u32) -> Result<()> {
        self.soc.listen(self, backlog)
    }

    pub fn accept(&self) -> Result<(Socket<'_>, Option<SocketAddr>)> {
        self.soc.accept(self)
    }

    pub fn bind(&self, addr: &SocketAddr) -> Result<()> {
        self.soc.bind(self, addr)
    }

    pub fn connect(&self, dest: &SocketAddr) -> Result<()> {
        self.soc.connect(self, dest)
    }

    pub fn recv(&self, flags: u8, buf: &mut [u8]) -> Result<usize> {
        self.soc.recv(self, flags, buf)
    }

    pub fn recvfrom(&self, flags: u8, buf: &mut [u8]) -> Result<(usize, Option<SocketAddr>)> {
        self.soc.recvfrom(self, flags, buf)
    }

    pub fn sendto(&self, buf: &[u8], flags: u8, dest: Option<&SocketAddr>) -> Result<usize> {
        self.soc.sendto(self, buf, flags, dest)
    }

    /// Explicitly close the socket, reporting any errors on close.
    ///
    /// Dropping a socket also closes it, but cannot return errors.
    /// Use this method if you need the error yourself.
    pub fn close(self) -> Result<()> {
        let socket = ManuallyDrop::new(self);
        socket.soc.close_raw(socket.fd)
    }
}
