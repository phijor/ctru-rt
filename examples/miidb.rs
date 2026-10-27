// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use log::{error, info};

use ctru_rt::{
    debug::init_log,
    entry,
    ports::srv::Srv,
    result::Result,
    services::fs::{self, ArchiveId, Attributes, MediaType, OpenFlags, Path},
    svc::{self, Timeout},
};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    ctru_rt::panic::exit_process(info)
}

fn run() -> Result<()> {
    let srv = Srv::init()?;

    let fs = fs::Fs::init(&srv)?;

    let archive = {
        const SHARED_EXTDATA_ID: u32 = 0xF000_000B;
        let path = Path::binary(&[MediaType::Nand as u32, SHARED_EXTDATA_ID, 0x00048000]);
        fs.open_archive(ArchiveId::SHARED_EXT_SAVEDATA, path)?
    };

    info!("Got archive handle: {archive:?}");

    const PATH: Path = ctru_rt::utf16_path!("/CFL_DB.dat");
    let cfldb = fs.open_file(
        &archive,
        PATH,
        OpenFlags::READ | OpenFlags::WRITE,
        Attributes::NONE,
    )?;

    info!("Got CFL_DB handle: {cfldb:?}");

    Ok(())
}

#[entry]
fn main() -> ! {
    init_log().expect("Failed to initialize logger");

    if let Err(e) = run() {
        error!("Failed to run MiiDB example: {:?}", e);
    }

    loop {
        svc::sleep_thread(Timeout::forever());
    }
}
