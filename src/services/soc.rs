use crate::ports::srv::Srv;
use crate::{
    heap::PageAlignedBuffer,
    ipc::{IpcParameter, IpcRequest, IpcResult, ThisProcessId},
    os::{mem::MemoryPermission, BorrowHandle, Handle},
    result::{ErrorCode as SystemErrorCode, Result as SystemResult},
    svc, tls,
};

use core::{marker::PhantomData, num::NonZeroU32};

use ctru_rt_macros::EnumCast;
use log::debug;

#[derive(Debug)]
pub struct Soc {
    handle: Handle,
    buffer: PageAlignedBuffer,
    buffer_handle: Handle,
}

impl Soc {
    pub fn init(srv: &Srv, buffer: PageAlignedBuffer) -> SystemResult<Self> {
        debug!("Creating memory block for buffer {:?}", buffer);
        let buffer_handle = unsafe {
            svc::create_memory_block(
                buffer.as_ptr().unwrap().as_ptr() as usize,
                buffer.size(),
                MemoryPermission::None,
                MemoryPermission::Rw,
            )?
        };
        debug!("Got buffer handle: {:?}", buffer_handle);
        let handle = srv.get_service_handle("soc:U")?;

        debug!("Got service handle: {:?}", handle);
        let _reply = IpcRequest::command(0x1)
            .parameter(buffer.size())
            .translate_parameter(ThisProcessId)
            .translate_parameter(buffer_handle.borrow_handle())
            .dispatch(handle.borrow_handle())?;

        debug!("Initialized!");
        Ok(Self {
            handle,
            buffer,
            buffer_handle,
        })
    }

    pub fn socket(
        &self,
        domain: Domain,
        socket_type: Type,
        protocol: Protocol,
    ) -> SystemResult<SocketFd<'_>> {
        let mut reply = IpcRequest::command(0x2)
            .parameters(&[
                domain.to_value(),
                socket_type.to_value(),
                protocol.to_value(),
            ])
            .translate_parameter(ThisProcessId)
            .dispatch(self.handle.handle())?;

        Ok(reply.read_result())
    }

    pub fn listen(&self, fd: &SocketFd<'_>, backlog: isize) -> Result<()> {
        let mut reply = IpcRequest::command(0x3)
            .parameter(fd)
            .parameter(backlog as u32)
            .translate_parameter(ThisProcessId)
            .dispatch(self.handle.handle())
            .map_err(SocketError::SystemErr)?;

        SocketError::into_result(reply.read_result())
    }

    pub fn accept(&self, fd: &SocketFd<'_>) -> SystemResult<SocketAddress> {
        let mut address_data = [0; 0x1c];

        let tls = tls::get_thread_local_storage();
        let mut buffer_descriptors = tls.static_buffer_descriptors();

        buffer_descriptors.set(0, &mut address_data);

        let _reply = IpcRequest::command(0x4)
            .parameter(fd)
            .parameter(address_data.len())
            .translate_parameter(ThisProcessId)
            .dispatch(self.handle.handle())?;

        unimplemented!()
    }

    pub fn bind(&self, socket: &SocketFd<'_>, addrlen: usize) -> Result<()> {
        todo!()
    }

    pub fn gethostid(&self) -> Result<[u8; 4]> {
        let mut reply = IpcRequest::command(0x16).dispatch(self.handle.borrow_handle())?;

        Ok(reply.read_word().to_ne_bytes())
    }

    fn shutdown(&self) -> SystemResult<()> {
        IpcRequest::command(0x19)
            .dispatch(self.handle.handle())
            .map(drop)
    }

    pub fn reclaim(mut self) -> SystemResult<PageAlignedBuffer> {
        self.shutdown()?;
        let buffer = core::mem::take(&mut self.buffer);

        drop(self);

        Ok(buffer)
    }
}

impl Drop for Soc {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Debug, EnumCast)]
#[non_exhaustive]
#[enum_cast(value_type = "u32")]
pub enum Domain {
    AfInet = 2,
}

impl Default for Domain {
    fn default() -> Self {
        Self::AfInet
    }
}

#[derive(Debug, EnumCast)]
#[non_exhaustive]
#[enum_cast(value_type = "u32")]
pub enum Type {
    Stream = 1,
    Datagram = 2,
}

#[derive(Debug, EnumCast)]
#[non_exhaustive]
#[enum_cast(value_type = "u32")]
pub enum Protocol {
    Default = 0,
}

impl Default for Protocol {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug)]
pub struct PosixReturnValue(u32);

impl IpcResult for PosixReturnValue {
    fn decode(result: u32) -> Self {
        Self(result)
    }
}

impl PosixReturnValue {
    pub fn check(ret: u32) -> Result<()> {
        if ret == 0 {
            Ok(())
        } else {
            Err(SocketError::SocketErr(PosixReturnValue(ret)))
        }
    }
}

#[derive(Debug)]
pub struct PosixErrorCode(NonZeroU32);

#[derive(Debug)]
pub struct SocketFd<'s>(u32, PhantomData<&'s u32>);

impl<'s> IpcParameter for &SocketFd<'s> {
    fn encode(&self) -> u32 {
        self.0
    }
}

impl<'s> IpcResult for SocketFd<'s> {
    fn decode(result: u32) -> Self {
        SocketFd(result, PhantomData)
    }
}

#[derive(Debug)]
pub struct SocketAddress {
    family: u32,
    data: [u8; 0x1a],
}

#[derive(Debug)]
pub enum SocketError {
    SystemErr(SystemErrorCode),
    SocketErr(PosixReturnValue),
}

impl From<SystemErrorCode> for SocketError {
    fn from(e: SystemErrorCode) -> Self {
        SocketError::SystemErr(e)
    }
}

impl From<PosixReturnValue> for SocketError {
    fn from(e: PosixReturnValue) -> Self {
        SocketError::SocketErr(e)
    }
}

type Result<T> = ::core::result::Result<T, SocketError>;

impl SocketError {
    fn into_result(rv: PosixReturnValue) -> Result<()> {
        match rv.0 {
            0 => Ok(()),
            _ => Err(SocketError::SocketErr(rv)),
        }
    }
}
