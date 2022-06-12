// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::marker::PhantomData;
use core::ops::Deref;

use crate::ipc::{IpcParameter, IpcRequest};
use crate::os::{BorrowHandle, OwnedHandle, WeakHandle};
use crate::ports::srv::Srv;
use crate::result::{Result, ERROR_NOT_AUTHORIZED};
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

pub struct Apt<'access, 'srv> {
    handle: OwnedHandle,
    _access: PhantomData<&'access mut AptAccess<'srv>>,
}
impl<'access, 'srv> Apt<'access, 'srv> {
    fn new(handle: OwnedHandle, access: &'access mut AptAccess<'srv>) -> Self {
        drop(access);
        Self {
            handle,
            _access: PhantomData,
        }
    }

    fn get_lock(&self, flags: u16) -> Result<OsMutex> {
        let mut reply = IpcRequest::command(0x01)
            .parameter(u32::from(flags))
            .dispatch(self.borrow_handle())?;

        let _applet_attributes = reply.read_word();
        let _apt_state = reply.read_word();

        let mut reply = reply.finish_results();
        let lock_handle = unsafe { reply.read_handle() };

        Ok(unsafe { OsMutex::from_handle(lock_handle) })
    }

    fn init(&self, app_id: AppId, attributes: AppletAttributes) -> Result<(Event, Event)> {
        let reply = IpcRequest::command(0x02)
            .parameter(app_id)
            .parameter(attributes)
            .dispatch(self.borrow_handle())?;

        let mut reply = reply.finish_results();
        let signal_event = unsafe { Event::from_handle(reply.read_handle()) };
        let resume_event = unsafe { Event::from_handle(reply.read_handle()) };

        Ok((signal_event, resume_event))
    }

    fn enable(&self, attributes: AppletAttributes) -> Result<()> {
        let _ = IpcRequest::command(0x03)
            .parameter(attributes)
            .dispatch(self.borrow_handle())?;
        Ok(())
    }
}

impl BorrowHandle for Apt<'_, '_> {
    fn borrow_handle(&self) -> WeakHandle {
        self.handle.borrow_handle()
    }
}

pub struct AptAccess<'srv> {
    srv: &'srv Srv,
    service_name_index: u8,
}

impl<'srv> AptAccess<'srv> {
    fn aquire<'access>(&'access mut self) -> Result<Apt<'access, 'srv>> {
        let mut result = ERROR_NOT_AUTHORIZED;
        for (service_name, i) in APT_SERVICE_NAMES
            .iter()
            .zip(0..)
            .skip(self.service_name_index as usize)
        {
            match self.srv.get_service_handle(service_name) {
                Ok(handle) => {
                    self.service_name_index = i;
                    return Ok(Apt::new(handle, self));
                }
                Err(err) => {
                    result = err;
                }
            }
        }

        Err(result)
    }
}

pub struct AptLock<'srv> {
    access: Mutex<AptAccess<'srv>>,
}

impl<'srv> AptLock<'srv> {
    pub fn init(srv: &'srv mut Srv) -> Result<Self> {
        let mut access = AptAccess {
            srv,
            service_name_index: 0,
        };

        let apt = access.aquire()?;

        const FLAGS: u16 = 0x0;
        let mutex = apt.get_lock(FLAGS)?;

        let (_signal_event, _resume_event) = apt.init(
            AppId::Application,
            AppletAttributes::new()
                .position(AppPosition::App)
                .manual_gpu_rights()
                .manual_dsp_rights(),
        )?;

        let access = Mutex::const_new(mutex, access);

        Ok(Self { access })
    }
}

impl<'srv> Deref for AptLock<'srv> {
    type Target = Mutex<AptAccess<'srv>>;

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
