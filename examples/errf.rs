// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::{
    debug, entry,
    ports::errf::{ErrF, ErrorInfo},
    result::ResultCode,
    svc::UserBreakReason,
};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = debug::SvcDebugLog::default();
    let _ = writeln!(log, "[PANIC] {}", info);

    ctru_rt::svc::user_break(UserBreakReason::Panic)
}

#[entry]
fn main() {
    debug::init_log().ok();

    if let Ok(errf) = ErrF::init() {
        let info = ErrorInfo::from_result_code_with_message(
            ResultCode::success(),
            "This is technically an error",
        );
        errf.throw(&info).expect("Failed to throw error");
    }
}
