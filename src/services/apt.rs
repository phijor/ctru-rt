// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::marker::PhantomData;

use crate::ipc::IpcRequest;
use crate::os::{BorrowHandle, Handle, WeakHandle};
use crate::ports::srv::Srv;
use crate::result::{Result, ERROR_NOT_AUTHORIZED};
use crate::sync::{Mutex, OsMutex};

const APT_SERVICE_NAMES: [&str; 3] = ["APT:S", "APT:A", "APT:U"];

struct AptInstance<'access, 'srv> {
    handle: Handle,
    _access: PhantomData<&'access mut AptAccess<'srv>>,
}

impl<'access, 'srv> AptInstance<'access, 'srv> {
    fn new(handle: Handle, access: &'access AptAccess<'srv>) -> Self {
        drop(access);
        Self {
            handle,
            _access: PhantomData,
        }
    }

    fn get_lock(&self, flags: u16) -> Result<OsMutex> {
        let reply = IpcRequest::command(0x01)
            .parameter(u32::from(flags))
            .dispatch(self.borrow_handle())?;

        let _applet_attributes = reply.read_word();
        let _apt_state = reply.read_word();

        let reply = reply.finish_results();
        let lock_handle = unsafe { reply.read_handle() };

        Ok(unsafe { OsMutex::from_handle(lock_handle) })
    }
}

impl BorrowHandle for AptInstance<'_, '_> {
    fn borrow_handle(&self) -> WeakHandle {
        self.handle.borrow_handle()
    }
}

struct AptAccess<'srv> {
    srv: &'srv Srv,
    service_name_index: u8,
}

impl AptAccess<'_> {
    fn aquire(&mut self) -> Result<AptInstance> {
        let mut result = ERROR_NOT_AUTHORIZED;
        for (service_name, i) in APT_SERVICE_NAMES
            .iter()
            .zip(0..)
            .skip(self.service_name_index as usize)
        {
            match self.srv.get_service_handle(service_name) {
                Ok(handle) => {
                    self.service_name_index = i;
                    return Ok(AptInstance::new(handle, &self));
                }
                Err(err) => {
                    result = err;
                }
            }
        }

        Err(result)
    }
}

pub struct Apt<'srv> {
    access: Mutex<AptAccess<'srv>>,
}

impl<'srv> Apt<'srv> {
    pub fn init(srv: &'srv mut Srv) -> Result<Self> {
        let mut access = AptAccess {
            srv,
            service_name_index: 0,
        };

        const FLAGS: u16 = 0x0;
        let mutex = access.aquire()?.get_lock(FLAGS)?;

        let access = Mutex::const_new(mutex, access);

        Ok(Self { access })
    }
}
