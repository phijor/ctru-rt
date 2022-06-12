// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![feature(try_trait_v2)]
#![feature(control_flow_enum)]
#![feature(auto_traits)]
#![feature(alloc_error_handler, allocator_api)]
#![feature(new_uninit, maybe_uninit_array_assume_init)]
#![feature(atomic_from_mut)]
#![feature(link_llvm_intrinsics)]
// Allow dead code for now
#![allow(dead_code)]
#![allow(clippy::missing_safety_doc)]

pub mod debug;
pub mod env;
pub mod graphics;
pub mod heap;
pub mod ipc;
pub mod os;
pub mod ports;
pub mod result;
pub mod services;
pub mod svc;
pub mod sync;
pub mod thread;
pub mod tls;

extern crate alloc;
extern crate core;

pub use ctru_rt_macros::entry;

use core::arch::global_asm;

global_asm! {
    include_str!("../rsrt0.S"),
    options(raw),
}

// #[macro_export]
// macro_rules! entry {
//     ($entry: path) => {
//         #[export_name = "main"]
//         pub unsafe fn __main() {
//             let f: fn() = $entry;
//
//             f()
//         }
//     };
// }

#[no_mangle]
unsafe extern "C" fn _ctru_rt_start() {
    crate::heap::init().expect("Failed to initialize heap");
    crate::early_debug!("Mapped heap.");
    crate::graphics::vram::init();
    crate::early_debug!("Mapped VRAM linear memory.");

    extern "Rust" {
        fn _ctru_rt_entry();
    }

    _ctru_rt_entry();
}

#[doc(hidden)]
/// Please the ARM ABI gods.
///
/// Until we implement stack unwinding, this shouldn't be necessary.  But trying to use the alloc
/// crate requires this symbol and the linker gets angry if it can't find it.  So some time in the
/// future we should figure out how and under which circumstances we can get rid of it.
#[no_mangle]
pub fn __aeabi_unwind_cpp_pr1() {}

#[no_mangle]
pub fn __aeabi_unwind_cpp_pr0() {}

pub mod util {
    trait EnumCast: Sized {
        type Value;
        fn parse_value(value: Self::Value) -> core::result::Result<Self, Self::Value>;

        fn as_value(&self) -> Self::Value;
    }
}
