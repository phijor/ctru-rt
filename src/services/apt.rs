// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Applet Services
//!
//! [`Apt`] provides access to the family of [`APT Services`] `APT:{S,A,U}`.
//!
//! [`APT Services`]: https://www.3dbrew.org/wiki/NS_and_APT_Services#APT_Services

use core::ops::Deref;

use crate::ipc::{IpcParameter, IpcRequest};
use crate::os::{AsHandle, BorrowedHandle, OwnedHandle};
use crate::ports::srv::Srv;
use crate::result::Result;
use crate::sync::{Event, Mutex, OsMutex};

use ctru_rt_macros::EnumCast;

const APT_SERVICE_NAMES: [&str; 3] = ["APT:S", "APT:A", "APT:U"];

#[derive(Clone, Copy)]
struct AppletAttributes(u8);

impl AppletAttributes {
    const fn new() -> Self {
        Self(0)
    }

    const fn position(self, position: AppPosition) -> Self {
        Self(self.0 | position.to_value())
    }

    const fn manual_gpu_rights(self) -> Self {
        Self(self.0 | (1 << 3))
    }

    const fn manual_dsp_rights(self) -> Self {
        Self(self.0 | (1 << 4))
    }
}

impl IpcParameter for AppletAttributes {
    fn encode(&self) -> u32 {
        self.0.into()
    }
}

pub struct AppletManagementInfo {
    requested_position: AppPosition,
    requested_id: AppId,
    menu_id: AppId,
    active_app_id: AppId,
}

pub struct Apt {
    handle: OwnedHandle,
}

impl Apt {
    fn new(handle: OwnedHandle) -> Self {
        Self { handle }
    }

    fn get_lock(&mut self, flags: u16) -> Result<OsMutex> {
        let mut reply = IpcRequest::command(0x01)
            .parameter(u32::from(flags))
            .dispatch(&self.handle)?;

        let _applet_attributes = reply.read_word();
        let _apt_state = reply.read_word();

        let mut reply = reply.finish_results();
        let lock_handle = unsafe { reply.read_handle() };

        Ok(unsafe { OsMutex::from_handle(lock_handle) })
    }

    fn init(&mut self, app_id: AppId, attributes: AppletAttributes) -> Result<(Event, Event)> {
        let reply = IpcRequest::command(0x02)
            .parameter(app_id)
            .parameter(attributes)
            .dispatch(&self.handle)?;

        let mut reply = reply.finish_results();

        let events = unsafe {
            let [signal_handle, resume_handle] = reply.read_handles();

            (
                Event::from_handle(signal_handle),
                Event::from_handle(resume_handle),
            )
        };

        Ok(events)
    }

    fn enable(&mut self, attributes: AppletAttributes) -> Result<()> {
        let _ = IpcRequest::command(0x03)
            .parameter(attributes)
            .dispatch(&self.handle)?;
        Ok(())
    }

    // TODO: Should this consume `self`?
    fn finalize(&mut self, app_id: AppId) -> Result<()> {
        let _ = IpcRequest::command(0x04)
            .parameter(app_id)
            .dispatch(&self.handle)?;

        Ok(())
    }

    fn applet_management_info(&mut self, app_id: AppId) -> Result<AppletManagementInfo> {
        let mut reply = IpcRequest::command(0x05)
            .parameter(app_id)
            .dispatch(&self.handle)?;

        let requested_position = AppPosition::from_value(reply.read_word() as u8)
            .expect("APT returned an invalid App position");
        let requested_id =
            AppId::from_value(reply.read_word() as u16).expect("APT returned an invalid App ID");
        let menu_id = AppId::from_value(reply.read_word() as u16)
            .expect("APT returned an invalid Menu App ID");
        let active_app_id = AppId::from_value(reply.read_word() as u16)
            .expect("APT returned an invalid active App ID");

        Ok(AppletManagementInfo {
            requested_position,
            requested_id,
            menu_id,
            active_app_id,
        })
    }
}

impl AsHandle for Apt {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

#[derive(Debug)]
pub struct AptAccess {
    service_name_index: usize,
}

impl AptAccess {
    fn new() -> Self {
        Self {
            service_name_index: 0,
        }
    }

    fn aquire(&mut self, srv: &Srv) -> Result<Apt> {
        let (handle, matched_offset) =
            srv.get_service_handle_alternatives(&APT_SERVICE_NAMES[self.service_name_index..])?;
        self.service_name_index += matched_offset;

        Ok(Apt::new(handle))
    }
}

#[derive(Debug)]
pub struct AptLock {
    access: Mutex<AptAccess>,
    signal_event: Event,
    resume_event: Event,
}

impl AptLock {
    pub fn init(srv: &Srv) -> Result<Self> {
        let mut access = AptAccess::new();

        let mut apt = access.aquire(srv)?;

        const FLAGS: u16 = 0x0;
        let mutex = apt.get_lock(FLAGS)?;

        let (signal_event, resume_event) = apt.init(
            AppId::Application,
            AppletAttributes::new().position(AppPosition::App),
        )?;

        let access = Mutex::const_new(mutex, access);

        Ok(Self {
            access,
            signal_event,
            resume_event,
        })
    }
}

impl Deref for AptLock {
    type Target = Mutex<AptAccess>;

    fn deref(&self) -> &Self::Target {
        &self.access
    }
}

#[derive(Debug, EnumCast)]
#[enum_cast(value_type = "u16")]
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

#[derive(Debug, EnumCast)]
#[enum_cast(value_type = "u8")]
enum AppPosition {
    App,
    AppLib,
    System,
    SystemLib,
    Resident,
    None = 0x7,
}

impl IpcParameter for AppId {
    fn encode(&self) -> u32 {
        self.to_value().into()
    }
}
