// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo};

use ctru_rt::debug::{init_log, SvcDebugLog};
use ctru_rt::entry;
use ctru_rt::ports::srv;
use ctru_rt::result::Result;
use ctru_rt::services::cfg;
use ctru_rt::svc::{self, Timeout};

use log::{error, info};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let mut log = SvcDebugLog;
    let _ = write!(log, "{}", info);

    svc::exit_process()
}

fn run() -> Result<()> {
    let srv = srv::Srv::init()?;
    let cfg = cfg::Cfg::init(&srv)?;

    let model = cfg.system_model()?;
    let hash = cfg.generate_console_unique_hash(0x0badf00d)?;

    info!("Model: {model:?}, hash: {hash:16x}");

    svc::sleep_thread(Timeout::forever());

    Ok(())
}

#[entry]
fn main() {
    let _ = init_log().expect("Failed to initialize logger");

    match run() {
        Ok(_) => {}
        Err(e) => {
            let _ = error!("Failed to run: {:?}", e);
            svc::exit_process()
        }
    }
}
