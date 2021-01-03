use core::{num::NonZeroU32, ops::Try};

use alloc::fmt;

pub type Result<T> = core::result::Result<T, ErrorCode>;

#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
#[must_use = "result codes must be checked for failure"]
pub struct ResultCode(u32);

impl ResultCode {
    pub fn and<T>(self, value: T) -> Result<T> {
        self.into_result().map(|_: ()| value)
    }

    pub fn and_then<T, F: FnOnce() -> T>(self, f: F) -> Result<T> {
        self.into_result().map(|_: ()| f())
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
#[must_use = "error codes indicate failure that must be dealt with"]
pub struct ErrorCode(NonZeroU32);

impl ErrorCode {
    pub unsafe fn new_unchecked(ec: u32) -> Self {
        Self(NonZeroU32::new_unchecked(ec))
    }

    pub fn new(level: Level, summary: Summary, module: Module, description: u32) -> Self {
        let result = ResultCode::new(level, summary, module, description);

        // Result code 0 is a success.
        Self(
            NonZeroU32::new(result.value())
                .expect("Cannot construct a ErrorCode code that is a success in disguise"),
        )
    }
}

impl Try for ResultCode {
    type Ok = ();
    type Error = ErrorCode;

    #[inline]
    fn into_result(self) -> Result<()> {
        if self.is_err() {
            Err(unsafe { ErrorCode::new_unchecked(self.0) })
        } else {
            Ok(())
        }
    }

    #[inline]
    fn from_error(ec: Self::Error) -> Self {
        ec.into()
    }

    #[inline]
    fn from_ok(_: Self::Ok) -> Self {
        Self::success()
    }
}

impl From<u32> for ResultCode {
    fn from(code: u32) -> Self {
        Self(code)
    }
}

impl From<ErrorCode> for ResultCode {
    fn from(ec: ErrorCode) -> Self {
        Self(ec.0.into())
    }
}

impl From<Result<()>> for ResultCode {
    #[inline]
    fn from(res: Result<()>) -> ResultCode {
        match res {
            Ok(()) => ResultCode::from_ok(()),
            Err(e) => ResultCode::from_error(e),
        }
    }
}

impl ResultCode {
    pub fn new(level: Level, summary: Summary, module: Module, description: u32) -> Self {
        let level: u32 = level.into();
        let summary: u32 = summary.into();
        let module: u8 = module.into();
        Self(level << 27 | summary << 21 | (module as u32) << 10 | (description & 0b11_1111_1111))
    }

    pub const fn success() -> Self {
        Self(0)
    }
}

pub trait ResultValue {
    fn value(&self) -> u32;

    fn is_err(&self) -> bool {
        self.value() != 0
    }

    fn is_ok(&self) -> bool {
        !self.is_err()
    }

    fn level(&self) -> Level {
        match ((self.value() >> 27) & 0b1_1111) as u8 {
            0 => Level::Success,
            1 => Level::Info,
            25 => Level::Status,
            26 => Level::Temporary,
            27 => Level::Permanent,
            28 => Level::Usage,
            29 => Level::Reinitialize,
            30 => Level::Reset,
            31 => Level::Fatal,
            level => Level::Unknown(level),
        }
    }

    fn summary(&self) -> Summary {
        ((self.value() >> 21) & 0b11_1111).into()
    }

    fn module(&self) -> Module {
        (((self.value() >> 10) & 0b1111_1111) as u8).into()
    }

    fn description(&self) -> u32 {
        self.value() & 0b11_1111_1111
    }
}

impl ResultValue for ResultCode {
    fn value(&self) -> u32 {
        self.0
    }
}

impl ResultValue for ErrorCode {
    fn value(&self) -> u32 {
        self.0.into()
    }

    fn is_err(&self) -> bool {
        true
    }

    fn is_ok(&self) -> bool {
        false
    }
}

macro_rules! result_value_dbg_fmt {
    ($rv_type: ty) => {
        impl fmt::Debug for $rv_type {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                if self.is_err() {
                    f.debug_struct(stringify!($rv_type))
                        .field("value", &self.value())
                        .field("level", &self.level())
                        .field("module", &self.module())
                        .field("summary", &self.summary())
                        .field("description", &self.description())
                        .finish()
                } else {
                    f.debug_struct(stringify!($rv_type))
                        .field("value", &self.value())
                        .field("level", &self.level())
                        .finish()
                }
            }
        }
    };
}

result_value_dbg_fmt!(ResultCode);
result_value_dbg_fmt!(ErrorCode);

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Level {
    Success,
    Info,
    Status,
    Temporary,
    Permanent,
    Usage,
    Reinitialize,
    Reset,
    Fatal,
    Unknown(u8),
}

impl Into<u32> for Level {
    fn into(self) -> u32 {
        match self {
            Self::Success => 0,
            Self::Info => 1,
            Self::Status => 25,
            Self::Temporary => 26,
            Self::Permanent => 27,
            Self::Usage => 28,
            Self::Reinitialize => 29,
            Self::Reset => 30,
            Self::Fatal => 31,
            Self::Unknown(code) => (code & 0b1_1111) as u32,
        }
    }
}

impl From<u32> for Level {
    fn from(code: u32) -> Level {
        match code as u8 {
            0 => Self::Success,
            1 => Self::Info,
            25 => Self::Status,
            26 => Self::Temporary,
            27 => Self::Permanent,
            28 => Self::Usage,
            29 => Self::Reinitialize,
            30 => Self::Reset,
            31 => Self::Fatal,
            code => Self::Unknown(code),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Summary {
    Success,
    Nop,
    WouldBlock,
    OutOfResource,
    NotFound,
    InvalidState,
    NotSupported,
    InvalidArgument,
    WrongArgument,
    Canceled,
    StatusChanged,
    Internal,
    InvalidResultValue,
    Unknown(u8),
}

impl From<u32> for Summary {
    fn from(code: u32) -> Summary {
        match code as u8 {
            0 => Self::Success,
            1 => Self::Nop,
            2 => Self::WouldBlock,
            3 => Self::OutOfResource,
            4 => Self::NotFound,
            5 => Self::InvalidState,
            6 => Self::NotSupported,
            7 => Self::InvalidArgument,
            8 => Self::WrongArgument,
            9 => Self::Canceled,
            10 => Self::StatusChanged,
            11 => Self::Internal,
            63 => Self::InvalidResultValue,
            code => Self::Unknown(code),
        }
    }
}

impl Into<u32> for Summary {
    fn into(self) -> u32 {
        match self {
            Self::Success => 0,
            Self::Nop => 1,
            Self::WouldBlock => 2,
            Self::OutOfResource => 3,
            Self::NotFound => 4,
            Self::InvalidState => 5,
            Self::NotSupported => 6,
            Self::InvalidArgument => 7,
            Self::WrongArgument => 8,
            Self::Canceled => 9,
            Self::StatusChanged => 10,
            Self::Internal => 11,
            Self::InvalidResultValue => 63,
            Self::Unknown(code) => (code & 0b11_1111) as u32,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(u8)]
pub enum Module {
    Common,
    Kernel,
    Util,
    FileServer,
    LoaderServer,
    Tcb,
    Os,
    Dbg,
    Dmnt,
    Pdn,
    Gsp,
    I2c,
    Gpio,
    Dd,
    Codec,
    Spi,
    Pxi,
    Fs,
    Di,
    Hid,
    Cam,
    Pi,
    Pm,
    PmLow,
    Fsi,
    Srv,
    Ndm,
    Nwm,
    Soc,
    Ldr,
    Acc,
    RomFs,
    Am,
    Hio,
    Updater,
    Mic,
    Fnd,
    Mp,
    Mpwl,
    Ac,
    Http,
    Dsp,
    Snd,
    Dlp,
    HioLow,
    Csnd,
    Ssl,
    AmLow,
    Nex,
    Friends,
    Rdt,
    Applet,
    Nim,
    Ptm,
    Midi,
    Mc,
    Swc,
    FatFs,
    Ngc,
    Card,
    CardNor,
    Sdmc,
    Boss,
    Dbm,
    Config,
    Ps,
    Cec,
    Ir,
    Uds,
    Pl,
    Cup,
    Gyroscope,
    Mcu,
    Ns,
    News,
    Ro,
    Gd,
    CardSpi,
    Ec,
    WebBrowser,
    Test,
    Enc,
    Pia,
    Act,
    VctL,
    Olv,
    Neia,
    Npns,
    Avd,
    L2b,
    Mvd,
    Nfc,
    Uart,
    Spm,
    Qtm,
    Nfp,
    Application,
    Invalid,
    Unknown(u8),
}

impl Into<u8> for Module {
    fn into(self) -> u8 {
        match self {
            Module::Common => 0,
            Module::Kernel => 1,
            Module::Util => 2,
            Module::FileServer => 3,
            Module::LoaderServer => 4,
            Module::Tcb => 5,
            Module::Os => 6,
            Module::Dbg => 7,
            Module::Dmnt => 8,
            Module::Pdn => 9,
            Module::Gsp => 10,
            Module::I2c => 11,
            Module::Gpio => 12,
            Module::Dd => 13,
            Module::Codec => 14,
            Module::Spi => 15,
            Module::Pxi => 16,
            Module::Fs => 17,
            Module::Di => 18,
            Module::Hid => 19,
            Module::Cam => 20,
            Module::Pi => 21,
            Module::Pm => 22,
            Module::PmLow => 23,
            Module::Fsi => 24,
            Module::Srv => 25,
            Module::Ndm => 26,
            Module::Nwm => 27,
            Module::Soc => 28,
            Module::Ldr => 29,
            Module::Acc => 30,
            Module::RomFs => 31,
            Module::Am => 32,
            Module::Hio => 33,
            Module::Updater => 34,
            Module::Mic => 35,
            Module::Fnd => 36,
            Module::Mp => 37,
            Module::Mpwl => 38,
            Module::Ac => 39,
            Module::Http => 40,
            Module::Dsp => 41,
            Module::Snd => 42,
            Module::Dlp => 43,
            Module::HioLow => 44,
            Module::Csnd => 45,
            Module::Ssl => 46,
            Module::AmLow => 47,
            Module::Nex => 48,
            Module::Friends => 49,
            Module::Rdt => 50,
            Module::Applet => 51,
            Module::Nim => 52,
            Module::Ptm => 53,
            Module::Midi => 54,
            Module::Mc => 55,
            Module::Swc => 56,
            Module::FatFs => 57,
            Module::Ngc => 58,
            Module::Card => 59,
            Module::CardNor => 60,
            Module::Sdmc => 61,
            Module::Boss => 62,
            Module::Dbm => 63,
            Module::Config => 64,
            Module::Ps => 65,
            Module::Cec => 66,
            Module::Ir => 67,
            Module::Uds => 68,
            Module::Pl => 69,
            Module::Cup => 70,
            Module::Gyroscope => 71,
            Module::Mcu => 72,
            Module::Ns => 73,
            Module::News => 74,
            Module::Ro => 75,
            Module::Gd => 76,
            Module::CardSpi => 77,
            Module::Ec => 78,
            Module::WebBrowser => 79,
            Module::Test => 80,
            Module::Enc => 81,
            Module::Pia => 82,
            Module::Act => 83,
            Module::VctL => 84,
            Module::Olv => 85,
            Module::Neia => 86,
            Module::Npns => 87,
            // ???
            Module::Avd => 90,
            Module::L2b => 91,
            Module::Mvd => 92,
            Module::Nfc => 93,
            Module::Uart => 94,
            Module::Spm => 95,
            Module::Qtm => 96,
            Module::Nfp => 97,
            Module::Application => 254,
            Module::Invalid => 0,
            Module::Unknown(m) => m,
        }
    }
}

impl From<u8> for Module {
    fn from(m: u8) -> Module {
        match m {
            0 => Module::Common,
            1 => Module::Kernel,
            2 => Module::Util,
            3 => Module::FileServer,
            4 => Module::LoaderServer,
            5 => Module::Tcb,
            6 => Module::Os,
            7 => Module::Dbg,
            8 => Module::Dmnt,
            9 => Module::Pdn,
            10 => Module::Gsp,
            11 => Module::I2c,
            12 => Module::Gpio,
            13 => Module::Dd,
            14 => Module::Codec,
            15 => Module::Spi,
            16 => Module::Pxi,
            17 => Module::Fs,
            18 => Module::Di,
            19 => Module::Hid,
            20 => Module::Cam,
            21 => Module::Pi,
            22 => Module::Pm,
            23 => Module::PmLow,
            24 => Module::Fsi,
            25 => Module::Srv,
            26 => Module::Ndm,
            27 => Module::Nwm,
            28 => Module::Soc,
            29 => Module::Ldr,
            30 => Module::Acc,
            31 => Module::RomFs,
            32 => Module::Am,
            33 => Module::Hio,
            34 => Module::Updater,
            35 => Module::Mic,
            36 => Module::Fnd,
            37 => Module::Mp,
            38 => Module::Mpwl,
            39 => Module::Ac,
            40 => Module::Http,
            41 => Module::Dsp,
            42 => Module::Snd,
            43 => Module::Dlp,
            44 => Module::HioLow,
            45 => Module::Csnd,
            46 => Module::Ssl,
            47 => Module::AmLow,
            48 => Module::Nex,
            49 => Module::Friends,
            50 => Module::Rdt,
            51 => Module::Applet,
            52 => Module::Nim,
            53 => Module::Ptm,
            54 => Module::Midi,
            55 => Module::Mc,
            56 => Module::Swc,
            57 => Module::FatFs,
            58 => Module::Ngc,
            59 => Module::Card,
            60 => Module::CardNor,
            61 => Module::Sdmc,
            62 => Module::Boss,
            63 => Module::Dbm,
            64 => Module::Config,
            65 => Module::Ps,
            66 => Module::Cec,
            67 => Module::Ir,
            68 => Module::Uds,
            69 => Module::Pl,
            70 => Module::Cup,
            71 => Module::Gyroscope,
            72 => Module::Mcu,
            73 => Module::Ns,
            74 => Module::News,
            75 => Module::Ro,
            76 => Module::Gd,
            77 => Module::CardSpi,
            78 => Module::Ec,
            79 => Module::WebBrowser,
            80 => Module::Test,
            81 => Module::Enc,
            82 => Module::Pia,
            83 => Module::Act,
            84 => Module::VctL,
            85 => Module::Olv,
            86 => Module::Neia,
            87 => Module::Npns,
            // ???,
            90 => Module::Avd,
            91 => Module::L2b,
            92 => Module::Mvd,
            93 => Module::Nfc,
            94 => Module::Uart,
            95 => Module::Spm,
            96 => Module::Qtm,
            97 => Module::Nfp,
            254 => Module::Application,
            255 => Module::Invalid,
            m => Module::Unknown(m),
        }
    }
}
