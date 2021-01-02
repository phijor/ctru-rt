#![no_std]
#![feature(try_trait)]
#![feature(optin_builtin_traits)]
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

        static __init_array_start: extern "C" fn();
        static __init_array_end: extern "C" fn();
    }

    output_debug_string("Zeroing BSS");
    r0::zero_bss(&mut __bss_start__, &mut __bss_end__);
    output_debug_string("Running init array");
    r0::run_init_array(&__init_array_start, &__init_array_end);

    #[cfg(feature = "heap")]
    {
        output_debug_string("Initializing heap");
        crate::heap::init().expect("Failed to initialize heap");
    }
    // output_debug_string("Initializing shared memory");
    // crate::os::sharedmem::init();

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

#[doc(hidden)]
/// libcore depends on someone (libc usually?) providing memcmp().  We do not build on top of libc,
/// so this is a workaround to get libcore to compile with std-aware cargo.  Note that this is a
/// highly inefficient memcmp implementation.
///
/// See also: https://github.com/rust-lang/rust/issues/32610
#[no_mangle]
pub unsafe extern "C" fn memcmp(mut s1: *const u8, mut s2: *const u8, size: usize) -> i32 {
    let end_ptr = s1.offset(size as isize);
    while s1 != end_ptr {
        let v1: i32 = s1.read().into();
        let v2: i32 = s2.read().into();

        match v1 - v2 {
            0 => {
                s1 = s1.offset(1);
                s2 = s2.offset(1);
            }
            diff => return diff,
        }
    }

    0
}
