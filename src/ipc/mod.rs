// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! # Inter-process communication
mod reply;
mod request;

use self::reply::CommandBufferReader;
pub(crate) use self::request::{CommandBufferWriter, IpcRequest};

use crate::os::{BorrowedHandle, OwnedHandle, RawHandle};
use crate::result::{ResultCode, ResultValue};
use crate::tls;

use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ops::BitOr;
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
mod state {
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
    fn encode(&self, cmdbuf: &mut CommandBufferWriter);
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
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(*self)
    }
}

impl IpcParameter for u64 {
    #[inline(always)]
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(*self as u32);
        cmdbuf.write((*self >> 32) as u32);
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
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(*self as u32)
    }
}

impl IpcParameter for ResultCode {
    #[inline(always)]
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(self.value())
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
            const EMPTY: [OwnedHandle; 0] = [];
            let handles = &EMPTY as *const [_; 0] as *const [_; N];
            return handles.read();
        }

        let header = cmdbuf.read();
        let num_handles = (header >> 26) + 1;
        debug_assert_eq!(num_handles, N as u32);

        let mut handles: [MaybeUninit<OwnedHandle>; N] = MaybeUninit::uninit().assume_init();

        for uninit_handle in &mut handles {
            let raw_handle: RawHandle = cmdbuf.read();
            let new_handle =
                OwnedHandle::new(raw_handle).expect("Command buffer contained an invalid Handle");

            uninit_handle.write(new_handle);
        }

        MaybeUninit::array_assume_init(handles)
    }
}

impl TranslateResult for OwnedHandle {
    #[inline(always)]
    unsafe fn decode(cmdbuf: &mut CommandBufferReader) -> Self {
        let header = cmdbuf.read();
        let num_handles = (header >> 26) + 1;
        debug_assert_eq!(num_handles, 1);

        OwnedHandle::new(cmdbuf.read()).expect("Command buffer contained an invalid Handle")
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
            cmdbuf.write(handle.handle)
        }
    }
}

impl<'h> TranslateParameter for BorrowedHandle<'h> {
    #[inline(always)]
    fn encode(self, cmdbuf: &mut CommandBufferWriter) {
        let header = TYPE_HANDLE;
        cmdbuf.write(header);
        cmdbuf.write(self.handle)
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
    source: *const u8,
    size: u32,
    target_id: u8,
    _lifetime: PhantomData<&'buf [u8]>,
}

impl<'buf> StaticBuffer<'buf> {
    pub(crate) const fn new(source: &'buf [u8], target_id: u8) -> Self {
        // The descriptor encodes the buffer size in *bytes* (max 0x3FFFF), not in
        // elements.  See `IPC_Desc_StaticBuffer()` in libctru.
        let size = core::mem::size_of_val(source);
        let size = if size <= 0x3FFFF {
            size as u32
        } else {
            panic!("Static buffer size must fit 18 bits");
        };

        let source = source.as_ptr();
        Self {
            source,
            size,
            target_id,
            _lifetime: PhantomData,
        }
    }

    pub(crate) const fn from_u32_slice(source: &'buf [u32], target_id: u8) -> Self {
        let source = unsafe {
            core::slice::from_raw_parts(source.as_ptr().cast::<u8>(), size_of_val(source))
        };
        Self::new(source, target_id)
    }

    pub(crate) const fn from_u32(value: &'buf u32, target_id: u8) -> Self {
        Self::from_u32_slice(core::slice::from_ref(value), target_id)
    }

    pub(crate) const fn null(target_id: u8) -> Self {
        Self {
            source: core::ptr::null(),
            size: 0,
            target_id,
            _lifetime: PhantomData,
        }
    }
}

impl TranslateParameter for StaticBuffer<'_> {
    #[inline]
    fn encode(self, cmdbuf: &mut CommandBufferWriter) {
        let index = self.target_id as u32;
        if index >= 16 {
            panic!("Static buffer target index must be in 0..16, not {}", index);
        }

        let header = (self.size << 14) | (index << 10) | TYPE_STATIC_BUFFER;

        // If size == 0, explicitly pass a null pointer.
        // A zero-sized slice might still point somewhere;
        // prevent the kernel from accidentally accessing
        // invalid memory.
        let ptr: *const u32 = if self.size != 0 {
            self.source.cast::<u32>()
        } else {
            core::ptr::null()
        };

        cmdbuf.write(header);
        cmdbuf.write(ptr as u32)
    }
}

#[derive(Debug)]
pub(crate) struct MappedWritableBuffer<'buf> {
    source: *mut u8,
    size: u32,
    _lifetime: PhantomData<&'buf mut [u8]>,
}

impl<'buf> MappedWritableBuffer<'buf> {
    pub fn new(buf: &'buf mut [u8]) -> Self {
        // Maximum buffer size (in bytes) is 0x0FFF_FFFF.
        let size = core::mem::size_of_val(buf);
        let size = if size <= 0x0FFF_FFFF {
            size as u32
        } else {
            panic!("Static buffer size must fit 26 bits");
        };

        let source = buf.as_mut_ptr();

        Self {
            source,
            size,
            _lifetime: PhantomData,
        }
    }
}

impl TranslateParameter for MappedWritableBuffer<'_> {
    fn encode(self, cmdbuf: &mut CommandBufferWriter) {
        let header = (self.size << 4) | 0b1000 | BufferRights::WRITE.0;

        let ptr: *mut u32 = self.source.cast();

        cmdbuf.write(header);
        cmdbuf.write(ptr as u32);
    }
}

#[derive(Debug)]
pub struct BufferRights(u32);

#[rustfmt::skip]
impl BufferRights {
    const READ:  Self = Self(0b010);
    const WRITE: Self = Self(0b100);
}

impl BitOr for BufferRights {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}
