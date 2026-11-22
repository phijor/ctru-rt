// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::ffi::CStr;
use core::num::NonZeroUsize;
use core::panic::PanicInfo;

extern crate alloc;
use alloc::string::String;

use ctru_console::ConsoleLog;
use ctru_rt::graphics::Graphics;
use ctru_rt::services::gsp::gpu::Gpu;
use ctru_rt::services::hid::{Hid, KeyPad};
use ctru_rt::services::http::Method;
use log::{LevelFilter, error, info};

use ctru_rt::{
    entry,
    ports::srv::Srv,
    result::Result,
    services::{ac::Ac, apt::AptLock, gsp::gpu::Screen, http::Http},
};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    ctru_rt::panic::exit_process(info)
}

fn run(srv: &Srv, console: &ConsoleLog) -> Result<()> {
    let ac = Ac::init(srv)?;
    info!("Initialized ac: {:#0x?}", ac);

    let mut gpu = Gpu::init(srv)?;
    let mut gfx = Graphics::init_default(&mut gpu)?;

    let hid = Hid::init(srv)?;

    const HTTP_POST_BUFFER_PAGES: NonZeroUsize = NonZeroUsize::new(1).unwrap();
    let http = Http::init(srv.try_duplicate()?, HTTP_POST_BUFFER_PAGES)?;

    let mut keys = KeyPad::new(0);
    let mut done = false;
    while !done {
        let down = keys.update(hid.last_keypad()).down;

        if down.start() {
            done = true;
            info!("Exiting...");
        }

        if down.a()
            && let Err(err) = request(&http)
        {
            error!("Request failed: {err:?}");
        }

        console.with_console(|console| console.render(&mut gfx));

        gfx.swap_buffers()?;
        gfx.wait_vblank0()?;
    }

    Ok(())
}

fn request(http: &Http) -> Result<()> {
    info!("Creating context");
    let ctx = http.context(Method::GET, c"http://icanhazip.com/")?;

    info!("Setting keep-alive");
    ctx.set_keep_alive(true)?;

    info!("Adding user agent");
    const USER_AGENT: &CStr =
        c"Mozilla/5.0 (Nintendo 3DS; Mobile; rv:10.0) Gecko/20100101 ctru-rt-http-example/1.0";
    ctx.add_request_header_field(c"User-Agent", USER_AGENT)?;

    info!("Beginning request...");
    ctx.begin_request()?;

    info!("Getting status code");
    let status_code = ctx.get_status_code()?;
    if status_code != 200 {
        error!("Request returned code {status_code}!");
        return Ok(());
    }

    info!("Downloading data...");
    let data = ctx.download_all()?;

    if let Ok(ip_str) = String::from_utf8(data) {
        let ip_str = ip_str.trim();
        info!("IP: {ip_str}")
    }

    Ok(())
}

fn init() -> Result<(Srv, AptLock)> {
    let srv = Srv::init()?;
    let apt = AptLock::init(srv.try_duplicate()?)?;

    Ok((srv, apt))
}

#[entry]
fn main() {
    let console = ctru_console::init_log(Screen::Top, LevelFilter::Trace)
        .expect("Failed to initialize console logger");
    let (srv, apt) = init().unwrap();
    if let Err(err) = run(&srv, console) {
        error!("Failed to run echo server: {err:#0x?}");
    }
    apt.exit()
}
