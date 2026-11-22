// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

use ctru_rt::{
    debug::log::{error, info},
    entry,
    graphics::Graphics,
    ports::srv::Srv,
    result::Result,
    services::{
        apt::AptLock,
        gsp::gpu::{Gpu, Screen},
        hid::Hid,
    },
};

mod game_of_life;

use crate::game_of_life::GameOfLife;

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    ctru_rt::panic::user_break(info)
}

fn run(srv: &Srv, apt: AptLock) -> Result<!> {
    let mut gpu = Gpu::init(srv)?;

    let mut gfx = Graphics::init_default(&mut gpu)?;

    let hid = Hid::init(srv)?;

    let mut game_bot = GameOfLife::new(gfx.dimensions(Screen::Bottom));
    game_bot.seed(0x1234_5678);
    let mut game_top = GameOfLife::new(gfx.dimensions(Screen::Top));
    game_top.seed(0x8765_4321);

    info!("Press START to exit");
    while !hid.last_keypad().start() {
        gfx.with_pixels_rgb_mut(Screen::Bottom, |pixels| game_bot.draw(pixels));
        gfx.with_pixels_rgb_mut(Screen::Top, |pixels| game_top.draw(pixels));

        gfx.swap_buffers()?;
        gfx.wait_vblank0()?;

        game_bot.update();
        game_top.update();
    }

    apt.exit()
}

fn init() -> Result<(Srv, AptLock)> {
    let _ = ctru_rt::debug::init_log();
    let srv = Srv::init()?;
    let apt = AptLock::init(srv.try_duplicate()?)?;

    Ok((srv, apt))
}

#[entry]
fn main() {
    let (srv, apt) = init().expect("Basic services should get initialized");

    let Err(err) = run(&srv, apt);
    error!("Failed to run: {err:?}");
}
