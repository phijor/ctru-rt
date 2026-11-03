// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

extern crate alloc;

use core::{fmt::Write as _, panic::PanicInfo};
use log::error;

use ctru_console::Console;
use ctru_rt::{
    debug::init_log,
    entry,
    graphics::Graphics,
    ports::srv::Srv,
    result::Result,
    services::{
        gsp::gpu::{Gpu, Screen},
        hid::{Hid, KeyPad},
    },
    svc::{self, Timeout},
};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    ctru_rt::panic::exit_process(info)
}

fn run() -> Result<()> {
    let srv = Srv::init()?;
    let mut gpu = Gpu::init(&srv)?;
    let mut gfx = Graphics::init_default(&mut gpu)?;
    let hid = Hid::init(&srv)?;

    gfx.clear_screen(Screen::Top);
    gfx.clear_screen(Screen::Bottom);

    let mut top = Console::new(Screen::Top);
    let mut bottom = Console::new(Screen::Bottom);

    bottom.put_strln("Press START to exit.");
    bottom.put_strln("The quick brown fox jumps over the lazy dog.");

    for _ in 0..bottom.width() {
        bottom.put_char('a');
    }
    bottom.put_char('\n');
    bottom.put_strln("x");
    bottom.put_strln("a\tbb\tccc\tdddd\t|");
    bottom.put_strln("xxxxx\rab");
    bottom.put_strln("äöü→ẞ");

    let mut keys = KeyPad::new(0);
    let mut done = false;
    let mut tick = 0usize;
    while !done {
        let down = keys.update(hid.last_keypad()).down;

        if down.start() {
            done = true;
            let _ = writeln!(&mut bottom, "Exiting...");
        }

        if down.a() {
            let screaming = if (tick & 0b1) != 0 { 'A' } else { 'g' };
            top.put_char(screaming);
        }
        if down.b() {
            top.put_char('\n');
        }

        top.render(&mut gfx);
        bottom.render(&mut gfx);

        gfx.swap_buffers()?;
        gfx.wait_vblank0()?;
        tick += 1;
    }

    Ok(())
}

#[entry]
fn main() -> ! {
    init_log().expect("Failed to initialize logger");

    if let Err(e) = run() {
        error!("Failed to run console example: {:?}", e);
    }

    loop {
        svc::sleep_thread(Timeout::forever());
    }
}
