use super::srv::Srv;
use crate::{
    ipc::IpcRequest,
    os::Handle,
    result::{ErrorCode, Level, Module, Result, Summary},
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
        match self {
            Self::NoConnection => false,
            _ => true,
        }
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
        let result = IpcRequest::command(0xd).dispatch(self.handle.handle())?;

        let status = match result.values[0] {
            0 => WifiStatus::NoConnection,
            1 => WifiStatus::Old3dsConnection,
            2 => WifiStatus::New3dsConnection,
            _ => {
                return Err(ErrorCode::new(
                    Level::Fatal,
                    Summary::InvalidResultValue,
                    Module::Ac,
                    1023,
                ));
            }
        };

        Ok(status)
    }
}