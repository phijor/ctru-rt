// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::ops::Deref;

use num_enum::IntoPrimitive;

use crate::ipc::{CommandBufferWriter, IpcRequest, Parameter, StaticBuffer};
use crate::os::{OwnedHandle, ValidRawHandle};
use crate::ports::srv::{LazyServiceSession, Srv};
use crate::result::Result;
use crate::sync::{Event, Mutex, MutexGuard, OsMutex};

const APT_SERVICE_NAMES: [&str; 3] = ["APT:S", "APT:A", "APT:U"];

#[derive(Clone, Copy)]
struct AppletAttributes(u8);

impl AppletAttributes {
    const fn new() -> Self {
        Self(0)
    }

    const fn position(self, position: AppPosition) -> Self {
        Self(self.0 | (position as u8))
    }

    const fn manual_gpu_rights(self) -> Self {
        Self(self.0 | (1 << 3))
    }

    const fn manual_dsp_rights(self) -> Self {
        Self(self.0 | (1 << 4))
    }
}

impl Parameter for AppletAttributes {
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(self.0.into())
    }
}

pub struct Apt {
    handle: OwnedHandle,
}

impl Apt {
    fn new(handle: OwnedHandle) -> Self {
        Self { handle }
    }

    fn get_lock(&self, flags: u16) -> Result<OsMutex> {
        let mut reply = IpcRequest::command(0x01)
            .parameter(u32::from(flags))
            .dispatch(&self.handle)?;

        let _applet_attributes = reply.read_word();
        let _apt_state = reply.read_word();

        let mut reply = reply.finish_results();
        let lock_handle = unsafe { reply.read_handle() };

        Ok(unsafe { OsMutex::from_handle(lock_handle) })
    }

    fn init(&self, app_id: AppId, attributes: AppletAttributes) -> Result<Events> {
        let reply = IpcRequest::command(0x02)
            .parameter(app_id)
            .parameter(attributes)
            .dispatch(&self.handle)?;

        let mut reply = reply.finish_results();
        let signal = unsafe { Event::from_handle(reply.read_handle()) };
        let resume = unsafe { Event::from_handle(reply.read_handle()) };

        Ok(Events { signal, resume })
    }

    fn enable(&self, attributes: AppletAttributes) -> Result<()> {
        let _ = IpcRequest::command(0x03)
            .parameter(attributes)
            .dispatch(&self.handle)?;
        Ok(())
    }

    fn finalize(&self, app_id: AppId) -> Result<()> {
        let _ = IpcRequest::command(0x04)
            .parameter(app_id)
            .dispatch(&self.handle)?;
        Ok(())
    }

    fn is_registered(&self, app_id: AppId) -> Result<bool> {
        let mut reply = IpcRequest::command(0x09)
            .parameter(app_id)
            .dispatch(&self.handle)?;
        let is_registered = reply.read_word() != 0;

        Ok(is_registered)
    }

    fn prepare_to_close_application(&self, cancel_preload: bool) -> Result<()> {
        let _ = IpcRequest::command(0x22)
            .parameter(cancel_preload as u32)
            .dispatch(&self.handle)?;
        Ok(())
    }

    fn close_application_raw(&self, payload: &[u32], handle: Option<ValidRawHandle>) -> Result<()> {
        let _ = IpcRequest::command(0x27)
            .parameter(size_of_val(payload))
            .translate_parameter(handle)
            .translate_parameter(StaticBuffer::from_u32_slice(payload, 0))
            .dispatch(&self.handle)?;
        Ok(())
    }

    fn close_application(&self, payload: &[u32]) -> Result<()> {
        self.close_application_raw(payload, None)
    }

    fn prepare_to_do_application_jump(
        &self,
        flags: u8,
        program_id: u64,
        media_type: u8,
    ) -> Result<()> {
        let _ = IpcRequest::command(0x31)
            .parameters(&[
                flags as u32,
                program_id as u32,
                (program_id >> 32) as u32,
                media_type as u32,
            ])
            .dispatch(&self.handle)?;
        Ok(())
    }
}

struct LazyApt {
    session: LazyServiceSession,
}

impl LazyApt {
    fn new(srv: Srv) -> Self {
        Self {
            session: LazyServiceSession::new(srv, &APT_SERVICE_NAMES),
        }
    }

    fn init(srv: Srv) -> Result<Mutex<Self>> {
        let mut apt = Self::new(srv);

        const FLAGS: u16 = 0x0;
        let mutex = apt.open_session()?.get_lock(FLAGS)?;

        Ok(Mutex::const_new(mutex, apt))
    }

    fn open_session(&mut self) -> Result<Apt> {
        self.session.open().map(Apt::new)
    }
}

#[repr(C)]
struct Events {
    signal: Event,
    resume: Event,
}

pub struct AptLock {
    events: Events,
    apt: Mutex<LazyApt>,
}

impl AptLock {
    pub fn init(srv: Srv) -> Result<Self> {
        let apt = LazyApt::init(srv)?;

        let events = apt.lock().open_session()?.init(
            AppId::Application,
            AppletAttributes::new()
                .position(AppPosition::App)
                .manual_gpu_rights()
                .manual_dsp_rights(),
        )?;

        Ok(Self { events, apt })
    }

    pub fn lock(&self) -> Result<AptGuard<'_>> {
        let mut guard = self.apt.lock();
        let apt = guard.open_session()?;

        Ok(AptGuard { apt, _guard: guard })
    }
}

pub struct AptGuard<'lock> {
    // Fields in drop order:
    apt: Apt,                           // 1. close the APT session,
    _guard: MutexGuard<'lock, LazyApt>, // 2. release the access lock.
}

impl Deref for AptGuard<'_> {
    type Target = Apt;

    fn deref(&self) -> &Self::Target {
        &self.apt
    }
}

#[derive(Debug, IntoPrimitive, Clone, Copy)]
#[repr(u16)]
enum AppId {
    HomeMenu = 0x101,
    Camera = 0x110,
    FriendsList = 0x112,
    GameNotes = 0x113,
    Web = 0x114,
    InstructionManual = 0x115,
    Notifications = 0x116,
    MiiVerse = 0x117,
    MiiVersePosting = 0x118,
    AmiiboSettings = 0x119,
    Application = 0x300,
    EShop = 0x301,
    SoftwareKeyboard = 0x401,
    AppletEd = 0x402,
    PNoteAp = 0x404,
    SNoteAp = 0x405,
    Error = 0x406,
    Mint = 0x407,
    Extrapad = 0x408,
    Memolib = 0x409,
}

#[derive(Debug, IntoPrimitive)]
#[repr(u8)]
enum AppPosition {
    App,
    AppLib,
    System,
    SystemLib,
    Resident,
    None = 0x7,
}

impl Parameter for AppId {
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(*self as u16 as u32)
    }
}
