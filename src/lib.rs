#![no_std]
#![feature(try_trait)]
#![cfg_attr(feature = "heap", feature(alloc_error_handler))]

pub mod debug;
pub mod env;
pub mod os;
pub mod result;
pub mod svc;

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
    extern "C" {
        static mut __bss_start__: u32;
        static mut __bss_end__: u32;

        static __init_array_start: extern "C" fn();
        static __init_array_end: extern "C" fn();
    }

    r0::zero_bss(&mut __bss_start__, &mut __bss_end__);
    r0::run_init_array(&__init_array_start, &__init_array_end);

    #[cfg(feature = "heap")]
    crate::heap::init().expect("Failed to initialize heap");

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
pub fn __aeabi_unwind_cpp_pr0() {}
