// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::{state, CommandBuffer, IpcResult, TranslateResult};
use crate::ipc::IpcHeader;
use crate::os::OwnedHandle;

use core::marker::PhantomData;

use log::trace;

pub(crate) struct CommandBufferReader {
    cmdbuf: CommandBuffer,
    read_ptr: *const u32,
}

impl CommandBufferReader {
    pub(crate) const unsafe fn new(buf: *const u32) -> Self {
        Self {
            cmdbuf: CommandBuffer(buf as *mut u32),
            read_ptr: buf,
        }
    }

    pub(crate) const fn start(&self) -> *const u32 {
        self.cmdbuf.start()
    }

    pub(crate) fn pos(&self) -> usize {
        unsafe { self.read_ptr.offset_from(self.start()) as usize }
    }

    #[inline]
    pub(crate) fn read(&mut self) -> u32 {
        let range = self.cmdbuf.range();
        if range.contains(&self.read_ptr) {
            unsafe {
                let value = self.read_ptr.read();
                trace!("cmdbuf[{}] = 0x{:08x}", self.pos(), value);
                self.read_ptr = self.read_ptr.add(1);
                value
            }
        } else {
            panic!(
                "Detected attempt to read past the end of command buffer: {:?} is past the end of {:?}",
                self.read_ptr, range,
            )
        }
    }
}

pub(crate) struct IpcReply<S: state::State = state::Normal> {
    cmdbuf: CommandBufferReader,
    _state: PhantomData<S>,
}

impl IpcReply<state::Normal> {
    pub(crate) unsafe fn new(buf: *const u32) -> Self {
        let mut cmdbuf = CommandBufferReader::new(buf);
        let header = cmdbuf.read(); // Skip the header. Replies are not yet validated.

        trace!("Received IPC reply: header = {:#x?}", IpcHeader(header));

        Self {
            cmdbuf,
            _state: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn read_result<R: IpcResult>(&mut self) -> R {
        R::decode(self.cmdbuf.read())
    }

    pub(crate) fn read_word(&mut self) -> u32 {
        self.read_result()
    }

    #[inline]
    pub(crate) fn finish_results(self) -> IpcReply<state::Translate> {
        IpcReply {
            cmdbuf: self.cmdbuf,
            _state: PhantomData,
        }
    }
}

impl IpcReply<state::Translate> {
    #[inline]
    pub(crate) unsafe fn read_translate_result<R: TranslateResult>(&mut self) -> R {
        R::decode(&mut self.cmdbuf)
    }

    #[inline]
    pub(crate) unsafe fn read_handle(&mut self) -> OwnedHandle {
        self.read_translate_result()
    }
}
