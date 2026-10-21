// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::{
    fmt::Write as _,
    panic::PanicInfo,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{
    debug::FixedSizeBufferWriter,
    svc::{self, output_debug_bytes, output_debug_string},
};

pub fn panic_log(info: &PanicInfo) {
    let mut buffer = FixedSizeBufferWriter::<512>::new();
    let _ = write!(&mut buffer, "{}", info);

    output_debug_bytes(buffer.occupied());
}

#[inline]
fn panic_with<F: FnOnce() -> !>(info: &PanicInfo, f: F) -> ! {
    static PANICKING: AtomicBool = AtomicBool::new(false);

    if PANICKING.swap(true, Ordering::Relaxed) {
        output_debug_string("Nested panic!");
    } else {
        panic_log(info);
    }

    f()
}

pub fn user_break(info: &PanicInfo) -> ! {
    panic_with(info, || svc::user_break(svc::UserBreakReason::Panic))
}

pub fn exit_process(info: &PanicInfo) -> ! {
    panic_with(info, svc::exit_process)
}

pub fn exit_thread(info: &PanicInfo) -> ! {
    panic_with(info, svc::exit_thread)
}
