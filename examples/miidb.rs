// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, string::String};
use core::{fmt, panic::PanicInfo};
use log::{error, info};

use ctru_rt::{
    debug::init_log,
    entry,
    ports::srv::Srv,
    result::{ErrorCode, Level, Module, Result, Summary},
    services::fs::{self, ArchiveId, Attributes, MediaType, OpenFlags, Path},
    svc::{self, Timeout},
};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    ctru_rt::panic::exit_process(info)
}

struct ClfDb {
    data: Box<[u8]>,
}

impl ClfDb {
    pub fn version(&self) -> u32 {
        u32::from_be_bytes(self.data.get(0x4..0x8).unwrap().try_into().unwrap())
    }

    pub fn miis(&self) -> Option<&[u8]> {
        self.data.get(0x8..0x8 + (100 * 0x5c))
    }

    pub fn mii_indices(&self) -> Option<&[u8]> {
        let count = *self.data.get(0xc824)? as usize;
        self.data.get(0xC828..0xC828 + count.min(100))
    }
}

fn decode_utf16_bytes(bytes: &[u8]) -> Option<String> {
    let mut name = String::with_capacity(bytes.len() / 2);

    let chars = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c));

    for c in char::decode_utf16(chars) {
        match c {
            Ok('\0') => return Some(name),
            Ok(c) => name.push(c),
            Err(_) => return None,
        }
    }

    Some(name)
}

struct Mii<'mii> {
    data: &'mii [u8; 0x5c],
}

impl Mii<'_> {
    fn get_u32(&self, idx: usize) -> u32 {
        let bytes: &[u8; 4] = self.data.get(idx..idx + 4).unwrap().try_into().unwrap();
        u32::from_be_bytes(*bytes)
    }

    fn get_u64(&self, idx: usize) -> u64 {
        let bytes: &[u8; 8] = self.data.get(idx..idx + 8).unwrap().try_into().unwrap();
        u64::from_be_bytes(*bytes)
    }

    fn id(&self) -> u32 {
        self.get_u32(0x0)
    }

    fn system_id(&self) -> u64 {
        self.get_u64(0x4)
    }

    fn is_special(&self) -> bool {
        (self.get_u32(0xc) & (1 << 31)) == 0
    }

    fn name(&self) -> Option<String> {
        self.data
            .get(0x1A..0x1A + 0x14)
            .and_then(decode_utf16_bytes)
    }

    fn author(&self) -> Option<String> {
        self.data
            .get(0x48..0x48 + 0x14)
            .and_then(decode_utf16_bytes)
    }
}

impl fmt::Debug for Mii<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mii")
            .field("id", &format_args!("{:#0x}", &self.id()))
            .field("system_id", &format_args!("{:#0x}", &self.system_id()))
            .field("is_special", &(if self.is_special() { "★" } else { "∘" }))
            .field("name", &self.name())
            .field("author", &self.author())
            .finish()
    }
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

    info!("Got CFL_DB handle: {cfldb:#0x?}");

    let size = cfldb.size()?;
    info!("File size: {size:#0x}");

    let contents = cfldb.read_vec()?;
    info!("Read {:#0x} bytes from the DB", contents.len());

    if let Some(header) = contents.get(0..4) {
        info!("Header: {:0x?}", header);
    }

    let db = ClfDb {
        data: contents.into_boxed_slice(),
    };

    info!("Version: {:#0x}", db.version());

    const NOT_FOUND: ErrorCode =
        ErrorCode::new(Level::Info, Summary::NotFound, Module::Application, 0);

    let Some(miis) = db.miis() else {
        error!("No Miis in database");
        return Err(NOT_FOUND);
    };

    let Some(indices) = db.mii_indices() else {
        error!("No Miis in database");
        return Err(NOT_FOUND);
    };

    for ix in indices {
        let ix = *ix as usize;
        if let Some(mii) = miis.get(ix * 0x5c..(ix + 1) * 0x5c) {
            let mii = Mii {
                data: mii.try_into().unwrap(),
            };
            if mii.id() == 0 {
                continue;
            }
            info!("Mii at index {ix:02}: {mii:?}");
        }
    }

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
