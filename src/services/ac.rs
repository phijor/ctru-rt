// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::ports::srv::Srv;
use crate::{
    ipc::IpcRequest,
    os::Handle,
    result::{CommonDescription, ErrorCode, Level, Module, Result, Summary},
};

#[derive(Debug)]
pub struct Ac {
    handle: Handle,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WifiStatus {
    NoConnection,
    Old3dsConnection,
    New3dsConnection,
}

impl WifiStatus {
    pub fn is_connected(&self) -> bool {
        !matches!(self, Self::NoConnection)
    }
}

impl Ac {
    pub fn init(srv: &Srv) -> Result<Self> {
        Ok(Self {
            handle: srv
                .get_service_handle("ac:i")
                .or_else(|_| srv.get_service_handle("ac:u"))?,
        })
    }

    pub fn wifi_status(&self) -> Result<WifiStatus> {
        let mut reply = IpcRequest::command(0xd).dispatch(self.handle.handle())?;

        let status = match reply.read_result::<u32>() {
            0 => WifiStatus::NoConnection,
            1 => WifiStatus::Old3dsConnection,
            2 => WifiStatus::New3dsConnection,
            _ => {
                return Err(ErrorCode::new(
                    Level::Fatal,
                    Summary::InvalidResultValue,
                    Module::Ac,
                    CommonDescription::InvalidResultValue.to_value(),
                ));
            }
        };

        Ok(status)
    }
}
