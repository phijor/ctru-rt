// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use core::{
    fmt::{self, Write as _},
    mem::offset_of,
    panic::PanicInfo,
};

use zerocopy::{
    FromBytes, Immutable, KnownLayout, Unaligned,
    byteorder::{big_endian as be, little_endian as le},
};

use ctru_console::Console;
use ctru_rt::{
    debug::{
        SvcDebugLog,
        log::{LevelFilter, error, info},
    },
    entry,
    graphics::Graphics,
    ports::srv::Srv,
    result::{ErrorCode, Level, Module, Result, Summary},
    services::{
        apt::AptLock,
        fs::{self, ArchiveId, Attributes, MediaType, OpenFlags, Path},
        gsp::gpu::{Gpu, Screen},
        hid::Hid,
    },
    thread,
};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    ctru_rt::panic::exit_process(info)
}

/// Layout of the Mii Maker (`"CFOG"`) section at the start of `CFL_DB.dat`.
///
/// See <https://www.3dbrew.org/wiki/Mii#Mii_Database>.
#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct CflDb {
    /// `"CFOG"`
    magic: [u8; 4],
    version: be::U32,
    /// Miis saved in Mii Maker. Their order in the file is unrelated to the one shown in-app,
    /// see [`Mii::position`].
    miis: [Mii; 100],
}

const _: () = assert!(size_of::<CflDb>() == 0x23f8);

/// A NUL-terminated UTF-16LE string, decoded on the fly when formatted.
///
/// Unpaired surrogates are shown as [`char::REPLACEMENT_CHARACTER`].
#[derive(Clone, Copy)]
struct Utf16Str<'a>(&'a [le::U16]);

impl Utf16Str<'_> {
    fn chars(self) -> impl Iterator<Item = char> {
        let units = self.0.iter().map(|c| c.get()).take_while(|&c| c != 0);
        char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
    }
}

impl fmt::Display for Utf16Str<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.chars().try_for_each(|c| f.write_char(c))
    }
}

impl fmt::Debug for Utf16Str<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('"')?;
        for c in self.chars() {
            write!(f, "{}", c.escape_debug())?;
        }
        f.write_char('"')
    }
}

/// A Mii as stored in the Mii Maker section of the database.
///
/// See <https://www.3dbrew.org/wiki/Mii#Mii_format>.
#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct Mii {
    /// Always 3, or 0 for an empty slot.
    version: u8,
    _flags: u8,
    /// Position on the selection screen: page index in bits 0-3, slot index in bits 4-7.
    position: u8,
    _device: u8,
    system_id: be::U64,
    mii_id: be::U32,
    _creator_mac: [u8; 6],
    _padding: [u8; 2],
    _details: [u8; 2],
    name: [le::U16; 10],
    _appearance: [u8; 0x48 - 0x2e],
    author: [le::U16; 10],
}

const _: () = assert!(offset_of!(Mii, name) == 0x1a);
const _: () = assert!(size_of::<Mii>() == 0x5c);

impl Mii {
    fn is_empty(&self) -> bool {
        self.version == 0
    }

    fn is_special(&self) -> bool {
        (self.mii_id.get() & (1 << 31)) == 0
    }

    fn page(&self) -> u8 {
        self.position & 0xf
    }

    fn slot(&self) -> u8 {
        self.position >> 4
    }

    fn name(&self) -> Utf16Str<'_> {
        Utf16Str(&self.name)
    }

    fn author(&self) -> Utf16Str<'_> {
        Utf16Str(&self.author)
    }
}

impl fmt::Debug for Mii {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mii")
            .field("id", &format_args!("{:#0x}", self.mii_id.get()))
            .field("system_id", &format_args!("{:#0x}", self.system_id.get()))
            .field("is_special", &(if self.is_special() { "★" } else { "∘" }))
            .field("name", &self.name())
            .field("author", &self.author())
            .finish()
    }
}

impl fmt::Display for Mii {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let special = if self.is_special() { '*' } else { ' ' };

        write!(f, "[{special}] {} by {}", self.name(), self.author())
    }
}

/// Read the Mii database and log the Miis in it, returning how many there are.
fn load_miis(srv: Srv) -> Result<usize> {
    let fs = fs::Fs::init(&srv)?;

    let archive = {
        const SHARED_EXTDATA_ID: u32 = 0xF000_000B;
        let path = Path::binary(&[MediaType::Nand as u32, SHARED_EXTDATA_ID, 0x00048000]);
        fs.open_archive(ArchiveId::SHARED_EXT_SAVEDATA, path)?
    };

    info!("Got archive handle: {archive:?}");

    const PATH: Path = ctru_rt::utf16_path!("/CFL_DB.dat");
    let cfldb = fs.open_file(&archive, PATH, OpenFlags::READ, Attributes::NONE)?;

    info!("Got CFL_DB handle: {cfldb:0x?}");

    let size = cfldb.size()?;
    info!("File size: {size:#0x}");

    let contents = cfldb.read_vec()?;
    info!("Read {:#0x} bytes from the DB", contents.len());

    const NOT_FOUND: ErrorCode =
        ErrorCode::new(Level::Info, Summary::NotFound, Module::Application, 0);

    let Ok((db, _)) = CflDb::ref_from_prefix(&contents) else {
        error!("Database is truncated");
        return Err(NOT_FOUND);
    };

    info!("Header: {:0x?}", db.magic);
    info!("Version: {:#0x}", db.version.get());

    let mut miis: Vec<&Mii> = db.miis.iter().filter(|mii| !mii.is_empty()).collect();
    miis.sort_by_key(|mii| (mii.page(), mii.slot()));

    for mii in &miis {
        info!("{}/{}: {mii}", mii.page(), mii.slot());
    }

    Ok(miis.len())
}

fn run(srv: &Srv, apt: AptLock) -> Result<!> {
    let log = ctru_console::init_log(Screen::Top, LevelFilter::Info)
        .expect("Failed to initialize console logger");
    let mut bottom = Console::new(Screen::Bottom);

    let mut gpu = Gpu::init(srv)?;
    let mut gfx = Graphics::init_default(&mut gpu)?;
    let hid = Hid::init(srv)?;

    let mut loader = Some({
        let srv = srv.try_duplicate()?;
        thread::spawn(move || load_miis(srv))?
    });

    bottom.put_strln("Loading Mii database...");
    while !hid.last_keypad().start() {
        if let Some(loader) = loader.take_if(|loader| !loader.is_running()) {
            let _ = match loader.join()? {
                Some(Ok(count)) => writeln!(bottom, "Found {count} Miis."),
                Some(Err(err)) => writeln!(bottom, "Failed to load: {err:?}"),
                None => writeln!(bottom, "Loader thread did not return."),
            };
            bottom.put_strln("Press START to exit.");
        }

        bottom.render(&mut gfx);
        log.with_console(|top| top.render(&mut gfx));

        gfx.swap_buffers()?;
        gfx.wait_vblank0()?;
    }

    apt.exit()
}

fn init() -> Result<(Srv, AptLock)> {
    let srv = Srv::init()?;
    let apt = AptLock::init(srv.try_duplicate()?)?;

    Ok((srv, apt))
}

#[entry]
fn main() {
    let (srv, apt) = init().expect("Basic services should get initialized");

    let Err(err) = run(&srv, apt);
    let _ = write!(SvcDebugLog, "Failed to run: {err:?}");
}
