// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

extern crate alloc;

use core::panic::PanicInfo;
use log::{LevelFilter, error, info};

use ctru_console::Console;
use ctru_rt::{
    entry,
    graphics::Graphics,
    ports::srv::Srv,
    result::Result,
    services::{
        apt::AptLock,
        gsp::gpu::{Gpu, Screen},
        hid::{Hid, KeyPad},
    },
    svc::{self, Timeout, sleep_thread},
};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    ctru_rt::panic::exit_process(info)
}

fn run() -> Result<()> {
    let mut bottom = Console::new(Screen::Bottom);
    let top = ctru_console::init_log(Screen::Top, LevelFilter::Trace)
        .expect("Failed to initialize console logger");

    let srv = Srv::init()?;
    let mut gpu = Gpu::init(&srv)?;
    let mut gfx = Graphics::init_default(&mut gpu)?;
    let hid = Hid::init(&srv)?;
    let apt = AptLock::init(srv)?;

    gfx.clear_screen(Screen::Top);
    gfx.clear_screen(Screen::Bottom);

    bottom.put_strln("Press START to exit.");
    bottom.put_strln("The quick brown fox jumps over the lazy dog.");

    if ctru_rt::thread::spawn(bottles).is_err() {
        error!("Failed to spawn bottles thread")
    };

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
            info!("Exiting...");
        }

        if down.a() {
            let screaming = if (tick & 0b1) != 0 { 'A' } else { 'g' };
            bottom.put_char(screaming);
        }
        if down.b() {
            bottom.put_char('\n');
        }

        bottom.render(&mut gfx);
        top.with_console(|top| top.render(&mut gfx));

        gfx.swap_buffers()?;
        gfx.wait_vblank0()?;
        tick += 1;
    }

    apt.exit()
}

fn bottles() {
    loop {
        for i in (3..100).rev() {
            info!("{i} bottles of beer on the wall,");
            sleep_thread(Timeout::from_seconds(1));
            info!("{i} bottles of beer.");
            sleep_thread(Timeout::from_seconds(1));
            info!("Take one down and pass it around,");
            sleep_thread(Timeout::from_seconds(1));
            info!("now there's {} more bottles of beer on the wall!", i - 1);
            sleep_thread(Timeout::from_seconds(1));
        }

        info!("2 bottles of beer on the wall,");
        sleep_thread(Timeout::from_seconds(1));
        info!("2 bottles of beer.");
        sleep_thread(Timeout::from_seconds(1));
        info!("Take one down and pass it around,");
        sleep_thread(Timeout::from_seconds(1));
        info!("now there's 1 more bottle of beer on the wall!");
        sleep_thread(Timeout::from_seconds(1));

        info!("1 bottle of beer on the wall,");
        sleep_thread(Timeout::from_seconds(1));
        info!("1 bottle of beer.");
        sleep_thread(Timeout::from_seconds(1));
        info!("Take one down and pass it around,");
        sleep_thread(Timeout::from_seconds(1));
        info!("now there's no more bottles of beer on the wall!");
        sleep_thread(Timeout::from_seconds(1));
    }
}

#[entry]
fn main() -> ! {
    if let Err(e) = run() {
        error!("Failed to run console example: {:?}", e);
    }

    loop {
        svc::sleep_thread(Timeout::infinite());
    }
}
