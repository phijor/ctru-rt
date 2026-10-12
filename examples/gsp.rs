// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::{
    entry,
    graphics::Grapics,
    ports::srv::Srv,
    result::Result,
    services::{
        gsp::gpu::{Gpu, Screen},
        hid::Hid,
    },
    svc,
};
use log::{error, info};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    use ctru_rt::{debug::SvcDebugLog, svc::UserBreakReason};
    let _ = writeln!(SvcDebugLog, "[PANIC] {}", info);

    svc::user_break(UserBreakReason::Panic)
}

fn run() -> Result<()> {
    let srv = Srv::init()?;
    info!("Initialized `srv`: {:#0x?}", srv);

    let mut gpu = Gpu::init(&srv)?;
    info!("Initialized `gsp::Gpu`: {:#0x?}", gpu);

    let mut gfx = Grapics::init_default(&mut gpu)?;
    info!("Initialized graphics: {:#0x?}", gfx);

    let hid = Hid::init(&srv)?;

    let mut color: u8 = 0;
    info!("Press START to exit");
    while !hid.last_keypad().start() {
        // Draw into the off-screen buffer, publish it, then wait for the scanout to pick it
        // up.  `swap_buffers` flushes the data cache, so the GPU sees what we just wrote.
        gfx.fill_screen(Screen::Top, color);
        gfx.fill_screen(Screen::Bottom, 0xFF - color);
        gfx.swap_buffers()?;
        gfx.wait_vblank0()?;

        color = color.wrapping_add(0x04);
    }

    info!("Exiting...");

    Ok(())
}

#[entry]
fn main() {
    let _ = ctru_rt::debug::init_log();

    match run() {
        Ok(_) => {}
        Err(e) => {
            error!("Failed to run: {:?}", e);
            svc::exit_process()
        }
    }
}
