#![no_std]
#![feature(lang_items)]
#![feature(try_trait)]

extern crate core;

pub mod env;
pub mod result;
pub mod svc;

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

    extern "Rust" {
        fn start();
    }

    start();
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[no_mangle]
unsafe extern "C" fn __aeabi_unwind_cpp_pr0() {
    loop {}
}
