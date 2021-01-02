#![no_std]
#![feature(try_trait)]
#![feature(auto_traits)]
#![feature(asm)]
#![cfg_attr(feature = "heap", feature(alloc_error_handler, allocator_api))]

pub mod debug;
pub mod env;
pub mod ipc;
pub mod os;
pub mod result;
pub mod services;
pub mod svc;
pub mod tls;

#[cfg(feature = "heap")]
pub mod heap;

#[cfg(feature = "heap")]
extern crate alloc;

extern crate core;

#[macro_export]
macro_rules! entry {
    ($entry: path) => {
        #[export_name = "main"]
        pub unsafe fn __main() {
            let f: fn() = $entry;

            f()
        }
    };
}

#[no_mangle]
unsafe extern "C" fn _ctru_rt_start() {
    use crate::svc::output_debug_string;

    extern "C" {
        static mut __bss_start__: u32;
        static mut __bss_end__: u32;
    }

    output_debug_string("Zeroing BSS");
    r0::zero_bss(&mut __bss_start__, &mut __bss_end__);

    #[cfg(feature = "heap")]
    {
        output_debug_string("Initializing heap");
        crate::heap::init().expect("Failed to initialize heap");
    }

    extern "Rust" {
        fn main();
    }

    main();
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
