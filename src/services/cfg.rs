// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::ipc::IpcRequest;
use crate::os::OwnedHandle;
use crate::ports::srv::Srv;
use crate::result::Result;

use ctru_rt_macros::EnumCast;

const CFG_SERVICE_NAMES: [&str; 3] = ["cfg:i", "cfg:s", "cfg:u"];

#[derive(Debug, EnumCast, PartialEq, Eq)]
#[enum_cast(value_type = "u32")]
pub enum Region {
    Japan,
    America,
    Europe,
    Australia,
    China,
    Korea,
    Taiwan,
}

#[derive(Debug, EnumCast, PartialEq, Eq)]
#[enum_cast(value_type = "u32")]
pub enum SystemModel {
    Ctr,
    Spr,
    Ktr,
    Ftr,
    Red,
    Jan,
}

#[derive(Debug)]
pub struct Cfg {
    service_handle: OwnedHandle,
}

impl Cfg {
    pub fn init(srv: &Srv) -> Result<Self> {
        let (service_handle, _) = srv.get_service_handle_alternatives(&CFG_SERVICE_NAMES)?;

        Ok(Self { service_handle })
    }

    pub fn secure_info_region(&self) -> Result<Region> {
        let mut reply = IpcRequest::command(0x02).dispatch(&self.service_handle)?;

        match Region::from_value(reply.read_word()) {
            Ok(region) => Ok(region),
            Err(unk) => panic!("Got unknown region value {unk:02x}"),
        }
    }

    pub fn generate_console_unique_hash(&self, salt: u32) -> Result<u64> {
        let mut reply = IpcRequest::command(0x03)
            .parameter(salt)
            .dispatch(&self.service_handle)?;

        let hash_low = reply.read_word();
        let hash_high = reply.read_word();

        Ok((u64::from(hash_high) << 32) | u64::from(hash_low))
    }

    pub fn is_subregion_canada_or_usa(&self) -> Result<bool> {
        let mut reply = IpcRequest::command(0x04).dispatch(&self.service_handle)?;

        Ok(reply.read_result())
    }

    pub fn system_model(&self) -> Result<SystemModel> {
        let mut reply = IpcRequest::command(0x05).dispatch(&self.service_handle)?;

        match SystemModel::from_value(reply.read_word()) {
            Ok(model) => Ok(model),
            Err(unk) => panic!("Got unknown system model value {unk:02x}"),
        }
    }

    pub fn is_system_model_2ds(&self) -> Result<bool> {
        let mut reply = IpcRequest::command(0x06).dispatch(&self.service_handle)?;

        Ok(reply.read_result())
    }
}
