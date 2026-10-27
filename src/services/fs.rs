use alloc::{vec, vec::Vec};
use core::{ffi::CStr, ops::BitOr};

use crate::{
    ipc::{IpcParameter, IpcRequest, MappedWritableBuffer, StaticBuffer, ThisProcessId},
    os::{AsHandle, OwnedHandle},
    ports::srv::Srv,
    result::Result,
};

use log::debug;

#[doc(hidden)]
pub use utf16_lit::utf16_null as __utf16_null;

/// Creates a nul-terminated UTF-16 [`Path`] from a string literal at compile time.
#[macro_export]
macro_rules! utf16_path {
    ($text:expr) => {{
        const DATA: &[u16] = &$crate::services::fs::__utf16_null!($text);
        const PATH: $crate::services::fs::Path<'static> =
            $crate::services::fs::Path::from_utf16_nul(DATA).unwrap();
        PATH
    }};
}

pub struct Fs {
    handle: OwnedHandle,
}

impl Fs {
    pub fn init(srv: &Srv) -> Result<Self> {
        debug!("Connecting to fs:USER...");
        let handle = srv.get_service_handle("fs:USER")?;

        debug!("Got fs:USER handle: {handle:?}");

        // IPC: FS:Initialize
        let _ = IpcRequest::command(0x801)
            .translate_parameter(ThisProcessId)
            .dispatch(handle.as_handle())?;

        debug!("FS:Initialize succeeded");

        Ok(Self { handle })
    }

    pub fn open_file(
        &self,
        archive: &Archive,
        path: Path,
        flags: OpenFlags,
        attrs: Attributes,
    ) -> Result<File> {
        const TRANSACTION: u32 = 0;

        let handle_low = archive.0 as u32;
        let handle_high = (archive.0 >> 32) as u32;

        let (buffer, size) = path.as_static_buffer(0);

        let result = IpcRequest::command(0x802)
            .parameter(TRANSACTION)
            .parameter(handle_low)
            .parameter(handle_high)
            .parameter(path.kind)
            .parameter(size)
            .parameter(flags)
            .parameter(attrs)
            .translate_parameter(buffer)
            .dispatch(&self.handle)?;

        let handle = unsafe { result.finish_results().read_handle() };
        Ok(File { handle })
    }

    pub fn open_archive(&self, id: ArchiveId, path: Path) -> Result<Archive> {
        let (buffer, size) = path.as_static_buffer(0);

        let mut result = IpcRequest::command(0x80C)
            .parameter(id)
            .parameter(path.kind)
            .parameter(size)
            .translate_parameter(buffer)
            .dispatch(&self.handle)?;

        let handle_low = result.read_word();
        let handle_high = result.read_word();

        Ok(Archive((handle_high as u64) << 32 | (handle_low as u64)))
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum PathKind {
    Invalid,
    Empty,
    Binary,
    Ascii,
    Utf16,
}

impl IpcParameter for PathKind {
    fn encode(&self) -> u32 {
        *self as u32
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Path<'p> {
    data: Option<&'p [u8]>,
    kind: PathKind,
}

fn as_nul_terminated_ascii(str: &str) -> Option<&[u8]> {
    let mut nul_terminated = true;
    for c in str.chars() {
        if !c.is_ascii() {
            return None;
        }
        nul_terminated = c == '\0';
    }

    match nul_terminated {
        false => None,
        true => Some(str.as_bytes()),
    }
}

impl<'p> Path<'p> {
    pub const fn binary(data: &[u32]) -> Self {
        let data = unsafe { core::slice::from_raw_parts(data.as_ptr().cast(), size_of_val(data)) };
        Self {
            data: Some(data),
            kind: PathKind::Binary,
        }
    }

    pub const fn from_ascii_cstr(str: &'p CStr) -> Option<Self> {
        let data = str.to_bytes_with_nul();

        match data.is_ascii() {
            true => Some(Self {
                kind: PathKind::Ascii,
                data: Some(data),
            }),
            false => None,
        }
    }

    pub const fn from_ascii_str(str: &'p str) -> Option<Self> {
        match CStr::from_bytes_until_nul(str.as_bytes()) {
            Ok(cstr) => Self::from_ascii_cstr(cstr),
            Err(_) => None,
        }
    }

    /// Creates a UTF-16 path from `data`, which must end in a nul terminator.
    ///
    /// See [`utf16_path!`](crate::utf16_path!) for creating paths from literals.
    pub const fn from_utf16_nul(data: &'p [u16]) -> Option<Self> {
        if !matches!(data.last(), Some(0)) {
            return None;
        }

        let data = unsafe { core::slice::from_raw_parts(data.as_ptr().cast(), size_of_val(data)) };
        Some(Self {
            data: Some(data),
            kind: PathKind::Utf16,
        })
    }

    #[inline]
    fn as_static_buffer(&'p self, target_id: u8) -> (StaticBuffer<'p>, usize) {
        match self.data {
            Some(data) => (StaticBuffer::new(data, target_id), size_of_val(data)),
            None => (StaticBuffer::null(target_id), 0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OpenFlags(u32);

impl OpenFlags {
    pub const READ: Self = Self(0b001);
    pub const WRITE: Self = Self(0b010);
    pub const CREATE: Self = Self(0b100);
}

impl BitOr for OpenFlags {
    type Output = OpenFlags;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl IpcParameter for OpenFlags {
    fn encode(&self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ArchiveId(u32);

impl ArchiveId {
    pub const SHARED_EXT_SAVEDATA: Self = Self(0x0000_0007);
}

impl IpcParameter for ArchiveId {
    fn encode(&self) -> u32 {
        self.0
    }
}

#[derive(Debug)]
pub struct Archive(u64);

#[derive(Debug)]
#[repr(u32)]
pub enum MediaType {
    Nand = 0x0,
    SdCard = 0x1,
    GameCard = 0x2,
}

#[derive(Debug, Clone, Copy)]
pub struct Attributes(u32);

#[rustfmt::skip]
impl Attributes {
    pub const NONE:      Self = Attributes(0x00_00_00_00);
    pub const DIRECTORY: Self = Attributes(0x00_00_00_01);
    pub const HIDDEN:    Self = Attributes(0x00_00_01_00);
    pub const ARCHIVE:   Self = Attributes(0x00_01_00_00);
    pub const READ_ONLY: Self = Attributes(0x01_00_00_00);
}

impl BitOr for Attributes {
    type Output = Attributes;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl IpcParameter for Attributes {
    fn encode(&self) -> u32 {
        self.0
    }
}

pub type FileOffset = u64;

#[derive(Debug)]
pub struct File {
    handle: OwnedHandle,
}

impl File {
    pub fn read(&self, buf: &mut [u8], offset: FileOffset) -> Result<usize> {
        let offset_low = offset as u32;
        let offset_high = (offset >> 32) as u32;

        let mut reply = IpcRequest::command(0x802)
            .parameter(offset_low)
            .parameter(offset_high)
            .parameter(size_of_val(buf))
            .translate_parameter(MappedWritableBuffer::new(buf))
            .dispatch(&self.handle)?;

        Ok(reply.read_word() as usize)
    }

    pub fn read_vec(&self) -> Result<Vec<u8>> {
        let size = self.size()?;
        let mut buf = vec![0; size];

        let bytes_read = self.read(&mut buf, 0)?;
        buf.truncate(bytes_read);

        Ok(buf)
    }

    pub fn size(&self) -> Result<usize> {
        let mut reply = IpcRequest::command(0x804).dispatch(&self.handle)?;

        Ok(reply.read_word() as usize)
    }
}
