// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::ipc::IpcRequest;
use crate::os::{OwnedHandle, BorrowedHandle};
use crate::result::{Result, ResultCode};
use crate::svc;

use ctru_rt_macros::EnumCast;

use log::debug;

use core::mem::{size_of, size_of_val};

extern "C" {
    #[link_name = "llvm.returnaddress"]
    fn returnaddress(frame: i32) -> *const u8;
}

#[derive(Debug, EnumCast)]
#[enum_cast(value_type = "u8")]
pub enum ErrorType {
    Generic,
    SystemMemoryDamaged,
    CardRemoved,
    Exception,
    Failure,
    Logged,
}

impl Default for ErrorType {
    fn default() -> Self {
        ErrorType::Generic
    }
}

#[derive(Debug, EnumCast)]
#[enum_cast(value_type = "u8")]
pub enum ExceptionType {
    PrefetchAbort,
    DataAbort,
    Undefined,
    Vfp,
}

#[repr(C, align(4))]
pub struct ErrorInfo {
    type_: ErrorType,
    revision_high: u8,
    revision_low: u8,
    result_code: ResultCode,
    pc_addr: u32,
    process_id: u32,
    title_id: u64,
    application_title_id: u64,
    failure_message: [u8; 0x60],
}

impl ErrorInfo {
    const fn zeroed() -> Self {
        Self {
            type_: ErrorType::Generic,
            revision_high: 0,
            revision_low: 0,
            result_code: ResultCode::success(),
            pc_addr: 0,
            process_id: 0,
            title_id: 0,
            application_title_id: 0,
            failure_message: [0; 0x60],
        }
    }
}

impl Default for ErrorInfo {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl ErrorInfo {
    const PARAMETER_SIZE: usize = size_of::<Self>() / size_of::<u32>();

    fn as_words(&self) -> &[u32; Self::PARAMETER_SIZE] {
        let this: &Self = self; // Prevent deref-coercion doing weird stuff
        unsafe { core::mem::transmute(this) }
    }

    fn as_slice(&self) -> &[u32] {
        // SAFETY: ErrorInfo is aligned to 4 bytes
        unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u32,
                size_of_val(self) / size_of::<u32>(),
            )
        }
    }

    fn current_process_id() -> u32 {
        svc::get_process_id(BorrowedHandle::active_process()).unwrap_or(0)
    }

    #[inline(never)]
    pub fn from_result_code(result_code: ResultCode) -> Self {
        Self {
            type_: ErrorType::Generic,
            result_code,
            pc_addr: unsafe { returnaddress(0) as u32 },
            process_id: Self::current_process_id(),
            ..Self::zeroed()
        }
    }

    #[inline]
    fn message_from(message: &str) -> [u8; 0x60] {
        let mut dest = [b'\0'; 0x60];
        let message = message.as_bytes();
        let size = message.len().min(size_of_val(&dest) - 1);
        dest[..size].copy_from_slice(&message[..size]);

        dest
    }

    #[inline(never)]
    pub fn from_result_code_with_message(result_code: ResultCode, message: &str) -> Self {
        Self {
            type_: ErrorType::Failure,
            result_code,
            pc_addr: unsafe { returnaddress(0) as u32 },
            process_id: Self::current_process_id(),
            failure_message: Self::message_from(message),
            ..Self::zeroed()
        }
    }
}

#[derive(Debug)]
pub struct ErrF {
    port: OwnedHandle,
}

impl ErrF {
    pub fn init() -> Result<Self> {
        debug!("Connecting to err:f...");
        let port = svc::connect_to_port("err:f\0")?;

        debug!("Opened err:f port: {:?}", port);

        Ok(Self { port })
    }

    pub fn throw(&self, error: &ErrorInfo) -> Result<()> {
        let _ = IpcRequest::command(0x1)
            .parameters(error.as_words())
            .dispatch(&self.port)?;

        Ok(())
    }
}
