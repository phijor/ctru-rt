// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::{ffi::CStr, mem::ManuallyDrop, num::NonZeroUsize, ops::BitOr};

use alloc::{vec, vec::Vec};

use crate::{
    ipc::{
        CommandBufferWriter, IpcRequest, MappedReadableBuffer, MappedWritableBuffer, Parameter,
        StaticBuffer, ThisProcessId,
    },
    os::{AsHandle, OwnedHandle, mem::MemoryBlock},
    ports::srv::Srv,
    result::Result,
};

#[derive(Debug)]
pub struct Http {
    handle: OwnedHandle,
    context: MemoryBlock,
    srv: Srv,
}

impl Http {
    pub fn init(srv: Srv, post_buffer_pages: NonZeroUsize) -> Result<Self> {
        let context = MemoryBlock::new(post_buffer_pages)?;
        let handle = srv.get_service_handle("http:C")?;

        let _ = IpcRequest::command(0x1)
            .parameter(context.size())
            .translate_parameter(ThisProcessId)
            .translate_parameter(context.as_handle())
            .dispatch(&handle)?;

        Ok(Self {
            handle,
            context,
            srv,
        })
    }

    pub fn context(&self, method: Method, url: &CStr) -> Result<Context<'_>> {
        let url = url.to_bytes_with_nul();

        let session = self.srv.get_service_handle("http:C")?;

        let mut reply = IpcRequest::command(0x2)
            .parameter(url.len())
            .parameter(method)
            .translate_parameter(MappedReadableBuffer::new(url))
            .dispatch(&self.handle)?;

        let handle = ContextHandle {
            id: ContextId(reply.read_word()),
            http: self,
        };

        Context::init(session, handle)
    }

    fn close_context(&self, context: ContextId) -> Result<()> {
        let _ = IpcRequest::command(0x3)
            .parameter(context)
            .dispatch(&self.handle)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
#[non_exhaustive]
pub enum Method {
    GET = 1,
    POST,
    HEAD,
    PUT,
    DELETE,
}

impl Parameter for Method {
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(*self as u32);
    }
}

#[derive(Debug, Clone, Copy)]
struct ContextId(u32);

impl Parameter for ContextId {
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(self.0)
    }
}

#[derive(Debug)]
struct ContextHandle<'http> {
    id: ContextId,
    http: &'http Http,
}

impl Drop for ContextHandle<'_> {
    fn drop(&mut self) {
        let _ = self.http.close_context(self.id);
    }
}

impl ContextHandle<'_> {
    fn close(self) -> Result<()> {
        let this = ManuallyDrop::new(self);
        this.http.close_context(this.id)
    }
}

#[derive(Debug)]
pub struct Context<'http> {
    // Fields in drop order: Close the session with `http:C` first,
    // before calling [`Http::close_context`] on this context.
    session: OwnedHandle,
    handle: ContextHandle<'http>,
}

impl<'http> Context<'http> {
    fn init(session: OwnedHandle, handle: ContextHandle<'http>) -> Result<Self> {
        let _ = IpcRequest::command(0x8)
            .parameter(handle.id)
            .translate_parameter(ThisProcessId)
            .dispatch(&session)?;

        Ok(Self { session, handle })
    }
}

impl Context<'_> {
    pub fn close(self) -> Result<()> {
        let Context { session, handle } = self;
        drop(session);
        handle.close()
    }

    pub fn get_download_progress(&self) -> Result<(usize, Option<NonZeroUsize>)> {
        let mut reply = IpcRequest::command(0x6)
            .parameter(self.handle.id)
            .dispatch(self.session.as_handle())?;

        let bytes_downloaded = reply.read_word() as usize;
        let bytes_total = reply.read_word() as usize;

        Ok((bytes_downloaded, NonZeroUsize::new(bytes_total)))
    }

    pub fn download(&self, buf: &mut [u8]) -> Result<bool> {
        use crate::result::{Level, Module, ResultCode, Summary};

        /// A download is still pending (`0xD840A02B`).
        pub const DOWNLOAD_PENDING: ResultCode =
            ResultCode::new(Level::Permanent, Summary::WouldBlock, Module::Http, 43);

        let (result, _) = IpcRequest::command(0xb)
            .parameter(self.handle.id)
            .parameter(buf.len())
            .translate_parameter(MappedWritableBuffer::new(buf))
            .dispatch_no_fail(self.session.as_handle())?;

        match result {
            DOWNLOAD_PENDING => Ok(true),
            result => result.and(false),
        }
    }

    pub fn download_all(&self) -> Result<Vec<u8>> {
        let (start_pos, total) = self.get_download_progress()?;

        let total = match total {
            Some(total) => total.get(),
            None => 0x1000,
        };

        let mut buf: Vec<u8> = vec![0u8; total];

        let mut end: usize = 0;
        loop {
            if end >= buf.len() {
                buf.resize(buf.len() * 2, 0);
            }

            let pending = self.download(&mut buf[end..])?;
            let (pos, _) = self.get_download_progress()?;
            end = pos
                .checked_sub(start_pos)
                .expect("Download should make positive progress");

            if !pending {
                break;
            }
        }

        buf.truncate(end);
        Ok(buf)
    }

    pub fn begin_request(&self) -> Result<()> {
        let _ = IpcRequest::command(0x9)
            .parameter(self.handle.id)
            .dispatch(self.session.as_handle())?;
        Ok(())
    }

    pub fn add_request_header_field(&self, field: &CStr, value: &CStr) -> Result<()> {
        let field = field.to_bytes_with_nul();
        let value = value.to_bytes_with_nul();

        let _ = IpcRequest::command(0x11)
            .parameter(self.handle.id)
            .parameter(field.len())
            .parameter(value.len())
            .translate_parameter(StaticBuffer::new(field, 3))
            .translate_parameter(MappedReadableBuffer::new(value))
            .dispatch(self.session.as_handle())?;
        Ok(())
    }

    pub fn get_status_code(&self) -> Result<u32> {
        let mut reply = IpcRequest::command(0x22)
            .parameter(self.handle.id)
            .dispatch(self.session.as_handle())?;

        let status_code = reply.read_word();
        Ok(status_code)
    }

    pub fn set_ssl_option(&self, opt: SSLOption) -> Result<()> {
        let _ = IpcRequest::command(0x2b)
            .parameter(self.handle.id)
            .parameter(opt)
            .dispatch(self.session.as_handle())?;
        Ok(())
    }

    pub fn set_keep_alive(&self, keep_alive: bool) -> Result<()> {
        let _ = IpcRequest::command(0x37)
            .parameter(self.handle.id)
            .parameter(u32::from(keep_alive))
            .dispatch(self.session.as_handle())?;
        Ok(())
    }
}

pub struct SSLOption(u32);

impl SSLOption {
    const DISABLE_VERIFY: Self = Self(1 << 9);
    const USE_TLS_1_0: Self = Self(1 << 11);
}

impl Parameter for SSLOption {
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(self.0);
    }
}

impl BitOr for SSLOption {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}
