// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::os::WeakHandle;
use crate::result::{Result, ResultCode};
use crate::svc;

use super::reply::IpcReply;
use super::{state, CommandBuffer, IpcHeader, IpcParameter, TranslateParameter};

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
        self.end_ptr = self.end_ptr.add(1);
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

pub(crate) struct IpcRequest<S: state::State = state::Normal> {
    cmdbuf: CommandBufferWriter,
    param_words: u32,
    translate_param_words: u32,
    id: u16,
    _state: PhantomData<S>,
}

impl IpcRequest<state::Normal> {
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
        }
    }

    #[inline]
    pub fn parameter<P>(mut self, parameter: P) -> Self
    where
        P: IpcParameter,
    {
        self.cmdbuf.write(parameter.encode());
        self.param_words += 1;
        self
    }

    #[inline]
    pub fn parameters<P, const N: usize>(mut self, parameters: &[P; N]) -> Self
    where
        P: IpcParameter,
    {
        for parameter in parameters {
            self.cmdbuf.write(parameter.encode());
        }
        self.param_words += parameters.len() as u32;
        self
    }
}

impl<S: state::State> IpcRequest<S> {
    #[inline]
    pub fn translate_parameter<P>(mut self, parameter: P) -> IpcRequest<state::Translate>
    where
        P: TranslateParameter,
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
        }
    }

    #[inline]
    pub fn dispatch_no_fail(self, receiver: WeakHandle) -> Result<(ResultCode, IpcReply)> {
        let cmdbuf = self.cmdbuf.finish();
        let header = IpcHeader::new(
            self.id,
            self.param_words as usize,
            self.translate_param_words as usize,
        );

        trace!("Dispatching IPC command: header = {:#x?}", header);

        // Write IPC header
        unsafe { cmdbuf.start().write(header.into()) }

        let mut reply = match unsafe { svc::send_sync_request(receiver, cmdbuf.into_inner()) } {
            Ok(reply_buffer) => unsafe { IpcReply::new(reply_buffer) },
            Err(e) => {
                error!(
                    "`svc::send_sync_request` failed: receiver = {:?}, err = {:?}",
                    receiver, e
                );
                return Err(e);
            }
        };

        let result = reply.read_result::<ResultCode>();
        Ok((result, reply))
    }

    #[inline]
    pub fn dispatch(self, receiver: WeakHandle) -> Result<IpcReply> {
        let (result, reply) = self.dispatch_no_fail(receiver)?;
        result.into_result()?;

        Ok(reply)
    }
}
