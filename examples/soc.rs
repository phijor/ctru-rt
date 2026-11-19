// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::net::{Ipv4Addr, SocketAddr};
use core::num::NonZeroUsize;
use core::panic::PanicInfo;

extern crate alloc;
use alloc::vec;

use ctru_console::ConsoleLog;
use ctru_rt::graphics::Graphics;
use ctru_rt::services::gsp::gpu::Gpu;
use ctru_rt::services::hid::{Hid, KeyPad};
use ctru_rt::services::soc::Socket;
use ctru_rt::thread;
use log::{LevelFilter, error, info};

use ctru_rt::{
    entry,
    ports::srv::Srv,
    result::Result,
    services::{
        ac::Ac,
        apt::AptLock,
        gsp::gpu::Screen,
        soc::{AddressFamily, Protocol, Soc, Type},
    },
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

    let _serve = {
        let srv = srv.try_duplicate()?;
        thread::spawn(move || {
            let Err(err) = serve(srv);
            error!("Failed to run server: {err:?}");
        })?
    };

    let mut keys = KeyPad::new(0);
    let mut done = false;
    while !done {
        let down = keys.update(hid.last_keypad()).down;

        if down.start() {
            done = true;
            info!("Exiting...");
        }

        console.with_console(|console| console.render(&mut gfx));

        gfx.swap_buffers()?;
        gfx.wait_vblank0()?;
    }

    Ok(())
}

fn echo(recv_buf: &mut [u8], client: Socket) -> Result<()> {
    loop {
        let bytes_received = client.recv(0, recv_buf)?;

        if bytes_received == 0 {
            return Ok(());
        }

        let msg = &recv_buf[..bytes_received];

        if let Ok(msg) = str::from_utf8(msg).map(str::trim) {
            info!("client:\n{msg}")
        }
        client.sendto(msg, 0, None)?;
    }
}

fn serve(srv: Srv) -> Result<!> {
    const CONTEXT_PAGES: NonZeroUsize = NonZeroUsize::new(0x100).unwrap();
    let soc = Soc::init(&srv, CONTEXT_PAGES)?;
    info!("Initialized soc: {:#0x?}", soc);

    let server = soc.socket(AddressFamily::AF_INET, Type::Stream, Protocol::Default)?;
    info!("Initialized socket: {:0x?}", server);

    info!("gethostid() = {:03?}", soc.gethostid()?.to_ne_bytes());

    server.bind(&SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 8000))?;
    server.listen(1)?;

    let mut buf = vec![0u8; 0x4000].into_boxed_slice();

    loop {
        let (client, client_addr) = server.accept()?;
        if let Some(addr) = client_addr {
            info!("Accepted connection from {addr}");
        }
        if let Err(err) = echo(&mut buf, client) {
            error!("echo: {err:0x?}");
        }
    }
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
