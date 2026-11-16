// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::os::{AsHandle, BorrowedHandle};
use crate::result::{Result, ResultCode};
use crate::svc;
use crate::tls::get_thread_local_storage;

use super::reply::IpcReply;
use super::{CommandBuffer, IpcHeader, Parameter, TranslateParameter, state};

use core::marker::PhantomData;

use log::{error, trace};

pub(crate) struct CommandBufferWriter {
    buf: CommandBuffer,
    end_ptr: *mut u32,
}

impl CommandBufferWriter {
    #[inline(always)]
    pub(crate) fn write(&mut self, arg: u32) {
        if self.buf.range().contains(&(self.end_ptr as *const u32)) {
            unsafe { self.end_ptr.write(arg) };
            unsafe { self.advance() };
        } else {
            panic!(
                "Detected attempt to access command buffer out of bounds: {:?} is outside of {:?}",
                self.end_ptr,
                self.buf.range()
            )
        }
    }

    pub(crate) unsafe fn advance(&mut self) {
        unsafe {
            self.end_ptr = self.end_ptr.add(1);
        }
    }

    pub(crate) fn pos(&self) -> usize {
        unsafe { self.end_ptr.offset_from(self.buf.start()) as usize }
    }

    pub(super) const fn new(buf: CommandBuffer) -> Self {
        let end_ptr = buf.start();
        Self { buf, end_ptr }
    }

    pub(super) const fn finish(self) -> CommandBuffer {
        self.buf
    }
}

pub(crate) struct IpcRequest<'p, S: state::State = state::Normal> {
    /// A writer for encoding the request into the command buffer in TLS.
    cmdbuf: CommandBufferWriter,
    /// The number of words written for normal parameters.
    param_words: u32,
    /// The number of words written for translate parameters.
    translate_param_words: u32,
    /// The command ID.
    id: u16,
    /// The typestate: are we encoding normal or translate parameters?
    _state: PhantomData<S>,
    /// The borrows held by translate parameters (e.g. memory backing mapped buffers).
    /// Translate parameters must stay live until the request is dispatched.
    _parameters: PhantomData<&'p ()>,
}

impl<'p> IpcRequest<'p, state::Normal> {
    #[inline]
    pub fn command(id: u16) -> Self {
        let mut cmdbuf = CommandBufferWriter::new(CommandBuffer::get());
        // # Safety
        // `end_ptr` points inside of command buffer
        unsafe { cmdbuf.advance() }; // write the header last
        Self {
            cmdbuf,
            param_words: 0,
            translate_param_words: 0,
            id,
            _state: PhantomData,
            _parameters: PhantomData,
        }
    }

    #[inline]
    pub fn parameter<P>(mut self, parameter: P) -> Self
    where
        P: Parameter,
    {
        let before = self.cmdbuf.pos();
        parameter.encode(&mut self.cmdbuf);
        self.param_words += (self.cmdbuf.pos() - before) as u32;
        self
    }

    #[inline]
    pub fn parameters<P, const N: usize>(mut self, parameters: &[P; N]) -> Self
    where
        P: Parameter,
    {
        let before = self.cmdbuf.pos();
        for parameter in parameters {
            parameter.encode(&mut self.cmdbuf);
        }
        self.param_words += (self.cmdbuf.pos() - before) as u32;
        self
    }
}

impl<'p, S: state::State> IpcRequest<'p, S> {
    #[inline]
    pub fn translate_parameter<P>(mut self, parameter: P) -> IpcRequest<'p, state::Translate>
    where
        P: TranslateParameter + 'p,
    {
        let pos = self.cmdbuf.pos();
        let before = self.cmdbuf.end_ptr;
        parameter.encode(&mut self.cmdbuf);

        let size = unsafe { self.cmdbuf.end_ptr.offset_from(before) as u32 };

        trace!("request[{}] = <size: {}>", pos, size);

        IpcRequest {
            cmdbuf: self.cmdbuf,
            param_words: self.param_words,
            translate_param_words: self.translate_param_words + size,
            id: self.id,
            _state: PhantomData,
            _parameters: PhantomData,
        }
    }

    #[inline]
    pub fn dispatch_impl<const N: usize>(
        self,
        receiver: BorrowedHandle,
        receive_buffers: &mut [&mut [u8]; N],
    ) -> Result<(ResultCode, IpcReply)> {
        let cmdbuf = self.cmdbuf.finish();
        let header = IpcHeader::new(
            self.id,
            self.param_words as usize,
            self.translate_param_words as usize,
        );

        trace!("Dispatching IPC command: header = {:#x?}", header);

        // Write IPC header
        unsafe { cmdbuf.start().write(header.into()) }

        let request = || unsafe {
            svc::send_sync_request(receiver, cmdbuf.into_inner())
                .map(|reply_buffer| IpcReply::new(reply_buffer))
                .inspect_err(|err| {
                    error!(
                        "`svc::send_sync_request` failed: receiver = {receiver:?}, err = {err:?}"
                    );
                })
        };
        let mut reply: IpcReply = if N == 0 {
            request()?
        } else {
            // Set the receive buffer descriptors to point to `receive_buffers`
            // before sending the request, then clear them after.
            get_thread_local_storage().with_receive_buffers(receive_buffers, request)?
        };

        let result = reply.read_result::<ResultCode>();
        Ok((result, reply))
    }

    #[inline]
    pub fn dispatch_no_fail<Handle: AsHandle>(
        self,
        receiver: Handle,
    ) -> Result<(ResultCode, IpcReply)> {
        self.dispatch_impl(receiver.as_handle(), &mut [])
    }

    #[inline]
    pub fn dispatch<Handle: AsHandle>(self, receiver: Handle) -> Result<IpcReply> {
        let (result, reply) = self.dispatch_no_fail(receiver)?;
        result.into_result()?;

        Ok(reply)
    }

    #[inline]
    pub fn dispatch_with_receive_buffers<const N: usize, Handle: AsHandle>(
        self,
        receiver: Handle,
        receive_buffers: &mut [&mut [u8]; N],
    ) -> Result<IpcReply> {
        let (result, reply) = self.dispatch_impl(receiver.as_handle(), receive_buffers)?;
        result.into_result()?;

        Ok(reply)
    }
}
