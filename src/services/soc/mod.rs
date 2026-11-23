// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::ipc::{MappedReadableBuffer, MappedWritableBuffer, StaticBuffer};
use crate::ports::srv::Srv;
use crate::{
    heap::PageBuffer,
    ipc::{IpcRequest, IpcResult, Parameter, ThisProcessId},
    os::{AsHandle, BorrowedHandle, OwnedHandle, mem::MemoryPermission},
    result::Result as SystemResult,
    svc,
};

use core::net::SocketAddr;
use core::num::{NonZeroU32, NonZeroUsize};
use core::ops::BitOr;

use log::debug;
use num_enum::{FromPrimitive, IntoPrimitive};

mod address;
mod error;
mod socket;
mod tcp;
mod udp;

use address::RawAddr;
pub use error::{Result, SocketError, SocketErrorCode};
use socket::Socket;
pub use tcp::{TcpListener, TcpStream};
pub use udp::UdpSocket;

#[derive(Debug)]
pub struct Soc {
    handle: OwnedHandle,
    context: Context,
}

impl Soc {
    pub fn init(srv: &Srv, context_num_pages: NonZeroUsize) -> SystemResult<Self> {
        let context = Context::new(context_num_pages)?;
        let handle = srv.get_service_handle("soc:U")?;

        debug!("Got service handle: {:08x?}", handle);
        let _reply = IpcRequest::command(0x1)
            .parameter(context.buffer.size())
            .translate_parameter(ThisProcessId)
            .translate_parameter(context.as_handle())
            .dispatch(&handle)?;

        debug!("Initialized!");
        Ok(Self { handle, context })
    }

    fn conjure_fd(&self, rv: ReturnValue) -> Result<Socket<'_>> {
        let fd = rv.check()?;
        Ok(Socket::new(fd, self))
    }

    fn socket(
        &self,
        domain: AddressFamily,
        socket_type: Type,
        protocol: Protocol,
    ) -> Result<Socket<'_>> {
        let mut reply = IpcRequest::command(0x2)
            .parameters(&[
                u32::from(u8::from(domain)),
                u32::from(socket_type),
                u32::from(protocol),
            ])
            .translate_parameter(ThisProcessId)
            .dispatch(&self.handle)?;

        self.conjure_fd(reply.read_result())
    }

    fn listen(&self, fd: &Socket<'_>, backlog: u32) -> Result<()> {
        let mut reply = IpcRequest::command(0x3)
            .parameter(fd)
            .parameter(backlog)
            .translate_parameter(ThisProcessId)
            .dispatch(&self.handle)?;

        ReturnValue::check(reply.read_result()).map(drop)
    }

    fn accept(&self, fd: &Socket<'_>) -> Result<(Socket<'_>, Option<SocketAddr>)> {
        let mut addr = RawAddr::unspec();
        let addr_bytes = addr.as_out_bytes();

        let mut reply = IpcRequest::command(0x4)
            .parameter(fd)
            .parameter(addr_bytes.len())
            .translate_parameter(ThisProcessId)
            .dispatch_with_receive_buffers(&self.handle, &mut [addr_bytes])?;

        let fd = self.conjure_fd(reply.read_result())?;
        let addr = addr.decode();

        Ok((fd, addr))
    }

    fn bind(&self, socket: &Socket<'_>, addr: &SocketAddr) -> Result<()> {
        let raw = RawAddr::encode(addr);
        let addr_bytes = raw.as_in_bytes();

        let mut reply = IpcRequest::command(0x5)
            .parameter(socket)
            .parameter(addr_bytes.len())
            .translate_parameter(ThisProcessId)
            .translate_parameter(StaticBuffer::new(addr_bytes, 0))
            .dispatch(&self.handle)?;

        ReturnValue::check(reply.read_result()).map(drop)
    }

    fn connect(&self, socket: &Socket<'_>, dest: &SocketAddr) -> Result<()> {
        let addr = RawAddr::encode(dest);
        let dest_bytes = addr.as_in_bytes();

        let mut reply = IpcRequest::command(0x6)
            .parameter(socket)
            .parameter(dest_bytes.len())
            .translate_parameter(ThisProcessId)
            .translate_parameter(StaticBuffer::new(dest_bytes, 0))
            .dispatch(&self.handle)?;

        ReturnValue::check(reply.read_result()).map(drop)
    }

    fn recvfrom_src_addr(
        &self,
        socket: &Socket<'_>,
        flags: Flags,
        buf: &mut [u8],
        src_addr: &mut [u8],
    ) -> Result<usize> {
        let bytes_received = if buf.len() > 0x2000 {
            let mut reply = IpcRequest::command(0x7)
                .parameter(socket)
                .parameter(buf.len())
                .parameter(flags)
                .parameter(src_addr.len())
                .translate_parameter(ThisProcessId)
                .translate_parameter(MappedWritableBuffer::new(buf))
                .dispatch_with_receive_buffers(&self.handle, &mut [src_addr])?;

            ReturnValue::check(reply.read_result())? as usize
        } else {
            let mut reply = IpcRequest::command(0x8)
                .parameter(socket)
                .parameter(buf.len())
                .parameter(flags)
                .parameter(src_addr.len())
                .translate_parameter(ThisProcessId)
                .dispatch_with_receive_buffers(&self.handle, &mut [buf, src_addr])?;

            let _ = ReturnValue::check(reply.read_result())?;
            reply.read_word() as usize
        };

        Ok(bytes_received)
    }

    fn recvfrom(
        &self,
        socket: &Socket<'_>,
        flags: Flags,
        buf: &mut [u8],
    ) -> Result<(usize, Option<SocketAddr>)> {
        let mut src_addr = RawAddr::unspec();
        let bytes_received = self.recvfrom_src_addr(socket, flags, buf, src_addr.as_out_bytes())?;
        Ok((bytes_received, src_addr.decode()))
    }

    fn recv(&self, socket: &Socket<'_>, flags: Flags, buf: &mut [u8]) -> Result<usize> {
        // `recv` doesn't need to decode a source address, so we omit it by passing an empty slice.
        // We cannot simply pass `&mut []`, however. The pointer of this slice is dangling; it
        // points to 0x1. Since the service always expects a valid address (even if the address
        // length is 0), pass an empty subslice of stack variable.
        let mut tmp = [0u8; 1];
        let ignore_src_addr = &mut tmp[..0];
        let bytes_received = self.recvfrom_src_addr(socket, flags, buf, ignore_src_addr)?;
        Ok(bytes_received)
    }

    fn sendto(
        &self,
        socket: &Socket<'_>,
        buf: &[u8],
        flags: Flags,
        dest: Option<&SocketAddr>,
    ) -> Result<usize> {
        let addr = match dest {
            Some(dest) => RawAddr::encode(dest),
            None => RawAddr::unspec(),
        };
        let dest_bytes = addr.as_in_bytes();

        let mut reply = if buf.len() > 0x2000 {
            IpcRequest::command(0x9)
                .parameter(socket)
                .parameter(buf.len())
                .parameter(flags)
                .parameter(dest_bytes.len())
                .translate_parameter(ThisProcessId)
                .translate_parameter(StaticBuffer::new(dest_bytes, 1))
                .translate_parameter(MappedReadableBuffer::new(buf))
                .dispatch(&self.handle)?
        } else {
            IpcRequest::command(0xA)
                .parameter(socket)
                .parameter(buf.len())
                .parameter(flags)
                .parameter(dest_bytes.len())
                .translate_parameter(ThisProcessId)
                .translate_parameter(StaticBuffer::new(buf, 2))
                .translate_parameter(StaticBuffer::new(dest_bytes, 1))
                .dispatch(&self.handle)?
        };

        let bytes_sent: u32 = ReturnValue::check(reply.read_result())?;

        Ok(bytes_sent as usize)
    }

    fn close_raw(&self, raw_socket: u32) -> Result<()> {
        let mut reply = IpcRequest::command(0xB)
            .parameter(raw_socket)
            .translate_parameter(ThisProcessId)
            .dispatch(&self.handle)?;

        let _ = ReturnValue::check(reply.read_result())?;
        Ok(())
    }

    pub fn gethostid(&self) -> Result<u32> {
        let mut reply = IpcRequest::command(0x16).dispatch(&self.handle)?;

        Ok(reply.read_word())
    }

    fn shutdown_sockets(&self) -> SystemResult<()> {
        IpcRequest::command(0x19).dispatch(&self.handle).map(drop)
    }
}

impl Drop for Soc {
    fn drop(&mut self) {
        let _ = self.shutdown_sockets();
    }
}

#[derive(Debug)]
struct Context {
    handle: OwnedHandle,
    buffer: PageBuffer,
}

impl Context {
    fn new(num_pages: NonZeroUsize) -> SystemResult<Self> {
        debug!("Allocating SOC context of {num_pages:#0x} page(s)");
        let mut buffer = PageBuffer::allocate_pages(num_pages).expect("Context allocation failed");

        debug!("Creating memory block for buffer {:?}", buffer);
        let handle = unsafe {
            svc::create_memory_block(
                buffer.as_mut_ptr(),
                MemoryPermission::None,
                MemoryPermission::Rw,
            )?
        };
        debug!("Got buffer handle: {:08x?}", handle);
        Ok(Context { handle, buffer })
    }
}

impl AsHandle for Context {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

#[derive(Debug, Default, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
enum AddressFamily {
    #[default]
    Unspecified = 0,
    INetV4 = 2,
    INetV6 = 23,
}

impl AddressFamily {
    #[inline]
    fn from_addr(addr: &SocketAddr) -> Self {
        match addr {
            SocketAddr::V4(_) => Self::INetV4,
            SocketAddr::V6(_) => Self::INetV6,
        }
    }
}

#[derive(Debug, IntoPrimitive)]
#[repr(u32)]
enum Type {
    Stream = 1,
    Datagram = 2,
}

#[derive(Debug, Default, IntoPrimitive)]
#[repr(u32)]
enum Protocol {
    #[default]
    Default = 0,
}

#[derive(Debug, Clone, Copy)]
struct Flags(u8);

impl Flags {
    const NONE: Self = Self(0);
    const PEEK: Self = Self(1 << 1);
    const DONTWAIT: Self = Self(1 << 2);
}

impl BitOr for Flags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl Parameter for Flags {
    fn encode(&self, cmdbuf: &mut crate::ipc::CommandBufferWriter) {
        cmdbuf.write(u32::from(self.0));
    }
}

#[derive(Debug, Clone, Copy)]
struct ReturnValue(i32);

impl ReturnValue {
    fn check(self) -> Result<u32> {
        let value = self.0;
        if let Some(errno) = NonZeroU32::new(value.unsigned_abs())
            && value < 0
        {
            Err(SocketError::SocketErr(SocketErrorCode::new(errno)))
        } else {
            Ok(value as u32)
        }
    }
}

impl IpcResult for ReturnValue {
    fn decode(result: u32) -> Self {
        Self(result as i32)
    }
}
