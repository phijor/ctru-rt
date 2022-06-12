// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! # Inter-process communication
mod reply;
mod request;

use self::reply::CommandBufferReader;
use self::request::CommandBufferWriter;
pub(crate) use self::request::IpcRequest;

use crate::os::{OwnedHandle, BorrowedHandle};
use crate::result::{ResultCode, ResultValue};
use crate::tls;

use core::convert::TryFrom;
use core::mem::MaybeUninit;
use core::{fmt, ops::Range};

#[derive(Copy, Clone)]
pub struct IpcHeader(u32);

#[allow(clippy::identity_op)]
impl IpcHeader {
    pub const fn new(
        command_id: u16,
        normal_param_words: usize,
        translate_param_words: usize,
    ) -> Self {
        let header = (command_id as u32) << 16
            | (((normal_param_words & 0b0011_1111) as u32) << 6)
            | (((translate_param_words & 0b0011_1111) as u32) << 0);
        Self(header)
    }

    pub const fn command_id(&self) -> u16 {
        (self.0 >> 16) as u16
    }

    pub const fn normal_param_words(&self) -> usize {
        ((self.0 >> 6) & 0b0011_1111) as usize
    }

    pub const fn translate_param_words(&self) -> usize {
        ((self.0 >> 0) & 0b0011_1111) as usize
    }
}

impl From<u32> for IpcHeader {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl From<IpcHeader> for u32 {
    fn from(header: IpcHeader) -> Self {
        header.0
    }
}

impl fmt::Debug for IpcHeader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("IpcHeader")
            .field("code", &self.0)
            .field("command", &self.command_id())
            .field("param_words", &self.normal_param_words())
            .field("translate_param_words", &self.translate_param_words())
            .finish()
    }
}
const COMMAND_BUFFER_LENGTH: usize = 0x80;

#[derive(Debug)]
struct CommandBuffer(*mut u32);

impl CommandBuffer {
    #[inline]
    pub(crate) fn get() -> Self {
        let command_buffer = tls::get_thread_local_storage().command_buffer();
        Self(command_buffer)
    }

    pub(crate) const fn start(&self) -> *mut u32 {
        self.0
    }

    pub(crate) fn range(&self) -> Range<*const u32> {
        Range {
            start: self.start(),
            end: unsafe { self.start().add(COMMAND_BUFFER_LENGTH) },
        }
    }

    pub(crate) fn into_inner(self) -> *mut u32 {
        self.0
    }
}

#[doc(hidden)]
pub(self) mod state {
    pub(crate) trait State {}

    macro_rules! state {
        ($name: ident) => {
            pub(crate) struct $name;

            impl State for $name {}
        };
        ($($name: ident),+) => {
            $(state!($name);)*
        }
    }

    state!(Normal, Translate);
}

pub(crate) trait IpcParameter {
    #[doc(hidden)]
    fn encode(&self) -> u32;
}

pub(crate) trait IpcResult {
    #[doc(hidden)]
    fn decode(result: u32) -> Self;
}

pub(crate) trait TranslateParameter {
    #[doc(hidden)]
    fn encode(self, cmdbuf: &mut CommandBufferWriter);
}

pub(crate) trait TranslateResult {
    #[doc(hidden)]
    unsafe fn decode(cmdbuf: &mut CommandBufferReader) -> Self;
}

impl IpcParameter for u32 {
    #[inline(always)]
    fn encode(&self) -> u32 {
        *self
    }
}

impl IpcResult for u32 {
    #[inline(always)]
    fn decode(result: u32) -> Self {
        result
    }
}

impl IpcParameter for usize {
    #[inline(always)]
    fn encode(&self) -> u32 {
        *self as u32
    }
}

impl IpcParameter for ResultCode {
    #[inline(always)]
    fn encode(&self) -> u32 {
        self.value()
    }
}

impl IpcResult for ResultCode {
    #[inline(always)]
    fn decode(result: u32) -> Self {
        ResultCode::from(result)
    }
}

const TYPE_HANDLE: u32 = 0 << 1;
const TYPE_STATIC_BUFFER: u32 = 1 << 1;

const FLAG_MOVE_HANDLE: u32 = 1 << 4;
const FLAG_REPLACE_PID: u32 = 1 << 5;

impl TranslateParameter for OwnedHandle {
    #[inline(always)]
    fn encode(self, cmdbuf: &mut CommandBufferWriter) {
        let handle: [OwnedHandle; 1] = unsafe { core::mem::transmute(self) };
        handle.encode(cmdbuf)
    }
}

impl<const N: usize> TranslateParameter for [OwnedHandle; N] {
    #[inline]
    fn encode(self, cmdbuf: &mut CommandBufferWriter) {
        if N == 0 {
            return;
        }

        let header = (N as u32 - 1) << 26 | FLAG_MOVE_HANDLE | TYPE_HANDLE;
        cmdbuf.write(header);

        for handle in self {
            cmdbuf.write(handle.leak())
        }
    }
}

impl<const N: usize> TranslateResult for [OwnedHandle; N] {
    #[inline]
    unsafe fn decode(cmdbuf: &mut CommandBufferReader) -> Self {
        if N == 0 {
            const CLOSED: OwnedHandle = OwnedHandle::new_closed();
            return [CLOSED; N];
        }

        let header = cmdbuf.read();
        let num_handles = (header >> 26) + 1;
        debug_assert_eq!(num_handles, N as u32);

        let mut handles = MaybeUninit::<Self>::uninit();

        for i in 0..N {
            let handles = &mut *handles.as_mut_ptr();
            handles[i] = OwnedHandle::new(cmdbuf.read());
        }

        handles.assume_init()
    }
}

impl TranslateResult for OwnedHandle {
    #[inline(always)]
    unsafe fn decode(cmdbuf: &mut CommandBufferReader) -> Self {
        let header = cmdbuf.read();
        let num_handles = (header >> 26) + 1;
        debug_assert_eq!(num_handles, 1);

        Self::new(cmdbuf.read())
    }
}

impl<'h, const N: usize> TranslateParameter for [BorrowedHandle<'h>; N] {
    #[inline(always)]
    fn encode(self, cmdbuf: &mut CommandBufferWriter) {
        if N == 0 {
            return;
        }

        let header = (N as u32 - 1) << 26 | TYPE_HANDLE;
        cmdbuf.write(header);

        for handle in self {
            cmdbuf.write(handle.as_raw())
        }
    }
}

impl<'h> TranslateParameter for BorrowedHandle<'h> {
    #[inline(always)]
    fn encode(self, cmdbuf: &mut CommandBufferWriter) {
        let header = TYPE_HANDLE;
        cmdbuf.write(header);
        cmdbuf.write(self.as_raw())
    }
}

#[derive(Debug)]
pub(crate) struct ThisProcessId;

impl TranslateParameter for ThisProcessId {
    #[inline]
    fn encode(self, cmdbuf: &mut CommandBufferWriter) {
        const HEADER: u32 = FLAG_REPLACE_PID | TYPE_HANDLE;
        const PLACEHOLDER: u32 = 0x0;
        cmdbuf.write(HEADER);
        cmdbuf.write(PLACEHOLDER);
    }
}

#[derive(Debug)]
pub(crate) struct StaticBuffer<'buf> {
    source: &'buf [u32],
    target_id: u8,
}

impl<'buf> StaticBuffer<'buf> {
    pub(crate) fn new(source: &'buf [u32], target_id: u8) -> Self {
        Self { source, target_id }
    }
}

impl TranslateParameter for StaticBuffer<'_> {
    #[inline]
    fn encode(self, cmdbuf: &mut CommandBufferWriter) {
        let index = self.target_id as u32;
        if index >= 16 {
            panic!("Static buffer target index must be in 0..16, not {}", index);
        }

        let size: u32 = u16::try_from(self.source.len())
            .expect("Static buffer length must fit 16 bits")
            .into();

        let header = (size << 14) | (index << 10) | TYPE_STATIC_BUFFER;

        cmdbuf.write(header);
        cmdbuf.write(self.source.as_ptr() as u32)
    }
}
