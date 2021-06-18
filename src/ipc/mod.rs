mod reply;

pub use self::reply::Reply;
use self::reply::ReplyBuffer;

use crate::{os::WeakHandle, result::Result, svc, tls};

use log::{debug, trace};

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

pub struct IpcRequest<'p, 'h> {
    command_id: u16,
    params: Option<&'p [u32]>,
    translate_params: Option<&'h [TranslateParameterSet<'h>]>,
}

impl<'p, 'h> IpcRequest<'p, 'h> {
    #[inline]
    pub fn command(id: u16) -> Self {
        Self {
            command_id: id,
            params: None,
            translate_params: None,
        }
    }

    #[inline]
    pub fn with_params(self, params: &'p [u32]) -> Self {
        Self {
            params: Some(params),
            ..self
        }
    }

    #[inline]
    pub fn with_translate_params(self, translate_params: &'h [TranslateParameterSet<'h>]) -> Self {
        Self {
            translate_params: Some(translate_params),
            ..self
        }
    }

    #[inline(always)]
    pub fn dispatch(self, client_handle: WeakHandle<'_>) -> Result<Reply<'_>> {
        self.dispatch_no_parse(client_handle).and_then(Reply::parse)
    }

    #[inline(always)]
    pub(crate) fn dispatch_no_parse(
        self,
        client_handle: WeakHandle<'_>,
    ) -> Result<ReplyBuffer<'_>> {
        let mut cmdbuf_writer = CommandBufferWriter::new(get_command_buffer());

        let header = {
            let normal_param_words = self.params.map(|p| p.len()).unwrap_or(0);
            let translate_param_words = self
                .translate_params
                .map(|tp| {
                    tp.iter()
                        .fold(0, |total, param_set| total + param_set.size())
                })
                .unwrap_or(0);
            IpcHeader::new(self.command_id, normal_param_words, translate_param_words)
        };

        trace!("Dispatching RPC with header {:08x?}", header);

        cmdbuf_writer.write(header.into());

        if let Some(params) = self.params {
            for param in params {
                trace!("Writing param {:08x?}", param);
                cmdbuf_writer.write(*param);
            }
        }

        if let Some(translate_params) = self.translate_params {
            for translate_param in translate_params.into_iter() {
                match translate_param {
                    TranslateParameterSet::Handle(handles) => {
                        cmdbuf_writer.write(
                            TranslationDescriptor::new(handles.len(), HandleTranslationType::Move)
                                .into_raw(),
                        );
                        for handle in handles.into_iter() {
                            cmdbuf_writer.write(handle.as_raw());
                        }
                    }
                    TranslateParameterSet::HandleRef(handle_refs) => {
                        cmdbuf_writer.write(
                            TranslationDescriptor::new(
                                handle_refs.len(),
                                HandleTranslationType::Clone,
                            )
                            .into_raw(),
                        );
                        for handle in handle_refs.into_iter() {
                            cmdbuf_writer.write(handle.as_raw());
                        }
                    }
                    TranslateParameterSet::ProcessId => {
                        cmdbuf_writer.write(0x20);
                        cmdbuf_writer.write(0x0);
                    }
                    TranslateParameterSet::StaticBuffer(buf, id) => {
                        trace!("Writing static buffer (id = {}): {:02x?}", id, buf);
                        let size = buf.len() as u32;
                        let id = (id & 0x0f) as u32;

                        cmdbuf_writer.write((size << 14) | (id << 10) | 0x2);
                        cmdbuf_writer.write(buf.as_ptr() as usize as u32)
                    }
                }
            }
        }

        unsafe {
            let reply_buffer =
                svc::send_sync_request(client_handle, cmdbuf_writer.finish().into_inner())?;
            Ok(ReplyBuffer::new(reply_buffer))
        }
    }
}

#[inline(always)]
pub fn ipc_dispatch<'h>(
    service_handle: WeakHandle<'h>,
    id: u16,
    params: &[u32],
    translate_params: &[TranslateParameterSet],
) -> Result<Reply<'h>> {
    IpcRequest::command(id)
        .with_params(params)
        .with_translate_params(translate_params)
        .dispatch(service_handle)
}

pub enum TranslateParameterSet<'h> {
    Handle(&'h [WeakHandle<'h>]),
    HandleRef(&'h [WeakHandle<'h>]),
    ProcessId,
    StaticBuffer(&'h [u32], u8),
}

impl TranslateParameterSet<'_> {
    #[inline]
    pub fn size(&self) -> usize {
        match *self {
            Self::Handle(handles) => 1 + handles.len(),
            Self::HandleRef(handle_refs) => 1 + handle_refs.len(),
            Self::ProcessId => 2,
            Self::StaticBuffer(_, _) => 2,
        }
    }

    fn write_to(&self, cmdbuf_writer: &mut CommandBufferWriter) {
        match *self {
            Self::Handle(handles) => {
                cmdbuf_writer.write(
                    TranslationDescriptor::new(handles.len(), HandleTranslationType::Move)
                        .into_raw(),
                );
                for handle in handles.into_iter() {
                    cmdbuf_writer.write(handle.as_raw());
                }
            }
            Self::HandleRef(handle_refs) => {
                cmdbuf_writer.write(
                    TranslationDescriptor::new(handle_refs.len(), HandleTranslationType::Clone)
                        .into_raw(),
                );
                for handle in handle_refs.into_iter() {
                    cmdbuf_writer.write(handle.as_raw());
                }
            }
            Self::ProcessId => {
                cmdbuf_writer.write(0x20);
                cmdbuf_writer.write(0x0);
            }
            Self::StaticBuffer(buf, id) => {
                let size = buf.len() as u32;
                let id = (id & 0x0f) as u32;

                cmdbuf_writer.write((size << 14) | (id << 10) | 0x2);
                cmdbuf_writer.write(buf.as_ptr() as usize as u32)
            }
        }
    }
}

const COMMAND_BUFFER_LENGTH: usize = 0x80;

#[derive(Debug)]
pub struct CommandBuffer(*mut u32);

impl CommandBuffer {
    pub(crate) const fn start(&self) -> *mut u32 {
        self.0
    }

    pub(crate) fn range(&self) -> Range<*const u32> {
        Range {
            start: self.start(),
            end: unsafe { self.start().add(COMMAND_BUFFER_LENGTH) },
        }
    }
}

struct CommandBufferWriter {
    buf: CommandBuffer,
    end_ptr: *mut u32,
}

impl CommandBufferWriter {
    #[inline]
    pub(crate) fn write(&mut self, arg: u32) {
        if self.buf.range().contains(&(self.end_ptr as *const u32)) {
            unsafe {
                self.end_ptr.write(arg);
                self.end_ptr = self.end_ptr.add(1);
            }
        } else {
            panic!(
                "Detected attempt to access command buffer out of bounds: {:?} is outside of {:?}",
                self.end_ptr,
                self.buf.range()
            )
        }
    }

    pub(crate) const fn new(buf: CommandBuffer) -> Self {
        let end_ptr = buf.start();
        Self { buf, end_ptr }
    }

    pub(crate) const fn finish(self) -> CommandBuffer {
        self.buf
    }
}

impl CommandBuffer {
    pub(crate) fn into_inner(self) -> *mut u32 {
        self.0
    }
}

#[inline]
pub fn get_command_buffer() -> CommandBuffer {
    let command_buffer = tls::get_thread_local_storage().command_buffer();
    CommandBuffer(command_buffer)
}

#[derive(Debug, Copy, Clone)]
enum HandleTranslationType {
    Clone = 0,
    Move = 1,
}
#[derive(Copy, Clone)]
struct TranslationDescriptor(u32);

impl TranslationDescriptor {
    pub const fn new(len: usize, handle_translation: HandleTranslationType) -> Self {
        Self((((len as isize) - 1) as u32) << 26 | (handle_translation as u32) << 4)
    }

    pub const fn len(&self) -> usize {
        (self.0 >> 26) as usize
    }

    pub const fn handle_translation_type(&self) -> HandleTranslationType {
        if ((self.0 >> 4) & 0b1) == 0 {
            HandleTranslationType::Clone
        } else {
            HandleTranslationType::Move
        }
    }

    pub const fn into_raw(self) -> u32 {
        self.0
    }
}

impl From<TranslationDescriptor> for u32 {
    fn from(descriptor: TranslationDescriptor) -> Self {
        descriptor.into_raw()
    }
}

impl From<u32> for TranslationDescriptor {
    fn from(desc: u32) -> Self {
        Self(desc)
    }
}

impl fmt::Debug for TranslationDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("TranslationDescriptor")
            .field("raw", &self.0)
            .field("len", &self.len())
            .field("handle_translation_type", &self.handle_translation_type())
            .finish()
    }
}
