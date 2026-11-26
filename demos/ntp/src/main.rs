// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]
#![feature(option_get_or_try_insert_with)]

extern crate alloc;

mod ntp;

use core::{fmt::Write as _, panic::PanicInfo};

use ctru_console::ConsoleLog;
use ctru_rt::{
    debug::{
        SvcDebugLog,
        log::{LevelFilter, error, info},
    },
    entry,
    graphics::Graphics,
    heap::PageBufferSize,
    ports::srv::Srv,
    result::Result,
    services::{
        apt::AptLock,
        gsp::gpu::{Gpu, Screen},
        hid::{Hid, KeyPad},
        soc::Soc,
    },
};

use crate::ntp::Ntp;

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    ctru_rt::panic::exit_process(info)
}

struct App {
    console: &'static ConsoleLog,
    soc: Soc,
    hid: Hid,
    gpu: Gpu,
    apt: AptLock,
}

impl App {
    fn init() -> Result<Self> {
        let console = ctru_console::init_log(Screen::Top, LevelFilter::Info)
            .expect("Failed to initialize console logger");
        let srv = Srv::init()?;
        let hid = Hid::init(&srv)?;
        let gpu = Gpu::init(&srv)?;
        let soc = Soc::init(&srv, PageBufferSize::from_mb(1))?;
        let apt = AptLock::init(srv)?;

        Ok(Self {
            console,
            soc,
            hid,
            gpu,
            apt,
        })
    }

    fn run(&mut self) -> Result<()> {
        let mut gfx = Graphics::init_default(&mut self.gpu)?;

        let mut keys = KeyPad::new(0);
        let mut done = false;

        let mut ntp: Option<Ntp<'_>> = None;

        info!("Press A to get the time, press START to exit.");
        while !done {
            let down = keys.update(self.hid.last_keypad()).down;

            if down.start() {
                done = true;
                info!("Exiting...");
            }

            if down.a() {
                match ntp.get_or_try_insert_with(|| Ntp::init(&self.soc)) {
                    Err(err) => error!("Failed to initialize time lookup: {err}"),
                    Ok(ntp) => {
                        info!("Getting the time...");
                        match ntp.get_time() {
                            Err(err) => error!("Failed to get the time: {err}"),
                            Ok(timestamp) => info!("Time (UTC): {timestamp}"),
                        }
                    }
                }
            }

            self.console
                .with_console(|console| console.render(&mut gfx));

            gfx.swap_buffers()?;
            gfx.wait_vblank0()?;
        }

        Ok(())
    }

    fn exit(self) -> ! {
        #[rustfmt::skip]
        let App { soc, hid, gpu, apt, console: _ } = self;
        drop(soc);
        drop(hid);
        drop(gpu);
        apt.exit()
    }
}

#[entry]
fn main() {
    let mut app = App::init().expect("Failed to initialize app");
    if let Err(err) = app.run() {
        let _ = write!(SvcDebugLog, "Failed to run app: {err:?}");
    }

    app.exit();
}
