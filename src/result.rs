// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::ops::{ControlFlow, FromResidual};
use core::{num::NonZeroU32, ops::Try};

use ctru_rt_macros::EnumCast;

use alloc::fmt;

pub type Result<T> = core::result::Result<T, ErrorCode>;

#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
#[must_use = "result codes must be checked for failure"]
pub struct ResultCode(u32);

impl ResultCode {
    pub fn into_result(self) -> Result<()> {
        match NonZeroU32::new(self.0) {
            None => Ok(()),
            Some(ec) => Err(ErrorCode(ec)),
        }
    }

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
    pub const unsafe fn new_unchecked(ec: u32) -> Self {
        Self(NonZeroU32::new_unchecked(ec))
    }

    pub const fn new(level: Level, summary: Summary, module: Module, description: u32) -> Self {
        let result = ResultCode::new(level, summary, module, description);

        const SUCCESS: ResultCode = ResultCode::success();
        match result {
            SUCCESS => {
                panic!("Cannot construct a ErrorCode code that is a success in disguise")
            }
            ResultCode(ec) => unsafe { Self::new_unchecked(ec) },
        }
    }
}

impl FromResidual for ResultCode {
    fn from_residual(ec: ErrorCode) -> Self {
        Self(ec.0.into())
    }
}

impl Try for ResultCode {
    type Output = ();
    type Residual = ErrorCode;

    fn branch(self) -> ControlFlow<Self::Residual, Self::Output> {
        match NonZeroU32::new(self.0) {
            Some(ec) => ControlFlow::Break(ErrorCode(ec)),
            None => ControlFlow::Continue(()),
        }
    }

    fn from_output(_: Self::Output) -> Self {
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

// impl From<Result<()>> for ResultCode {
//     #[inline]
//     fn from(res: Result<()>) -> ResultCode {
//         match res {
//             Ok(()) => ResultCode::from_ok(()),
//             Err(e) => ResultCode::from_error(e),
//         }
//     }
// }

impl ResultCode {
    pub const fn new(level: Level, summary: Summary, module: Module, description: u32) -> Self {
        let level: u32 = level.to_value();
        let summary: u32 = summary.to_value();
        let module: u8 = module.to_value();
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

    fn level(&self) -> ::core::result::Result<Level, u32> {
        Level::from_value((self.value() >> 27) & 0b1_1111)
    }

    fn summary(&self) -> ::core::result::Result<Summary, u32> {
        Summary::from_value((self.value() >> 21) & 0b11_1111)
    }

    fn module(&self) -> ::core::result::Result<Module, u8> {
        Module::from_value(((self.value() >> 10) & 0b1111_1111) as u8)
    }

    fn description(&self) -> ::core::result::Result<CommonDescription, u32> {
        CommonDescription::from_value(self.value() & 0b11_1111_1111)
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
                        .field("level", result_value_dbg_fmt!(@ match self.level()))
                        .field("module", result_value_dbg_fmt!(@ match self.module()))
                        .field("summary", result_value_dbg_fmt!(@ match self.summary()))
                        .field("description", result_value_dbg_fmt!(@ match self.description()))
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
    (@ match $field:expr) => {
        match $field {
            Ok(ref known) => known,
            Err(ref unknown) => unknown,
        }
    };
}

result_value_dbg_fmt!(ResultCode);
result_value_dbg_fmt!(ErrorCode);

#[derive(Debug, Copy, Clone, PartialEq, EnumCast)]
#[enum_cast(value_type = "u32")]
pub enum Level {
    Success,
    Info,
    Status = 25,
    Temporary,
    Permanent,
    Usage,
    Reinitialize,
    Reset,
    Fatal,
}

#[derive(Debug, Copy, Clone, PartialEq, EnumCast)]
#[enum_cast(value_type = "u32")]
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
    InvalidResultValue = 63,
}

#[derive(Debug, Copy, Clone, PartialEq, EnumCast)]
#[enum_cast(value_type = "u8")]
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
    Avd = 90,
    L2b,
    Mvd,
    Nfc,
    Uart,
    Spm,
    Qtm,
    Nfp,
    Application = 254,
    Invalid = 255,
}

pub trait Description {
    fn into_code(self) -> u32;
}

#[derive(Debug, EnumCast)]
#[enum_cast(value_type = "u32")]
pub enum CommonDescription {
    Success = 0,
    InvalidSection = 1000,
    TooLarge = 1001,
    NotAuthorized = 1002,
    AlreadyDone = 1003,
    InvalidSize = 1004,
    InvalidEnumValue = 1005,
    InvalidCombination = 1006,
    NoData = 1007,
    Busy = 1008,
    MisalignedAddress = 1009,
    MisalignedSize = 1010,
    OutOfMemory = 1011,
    NotImplemented = 1012,
    InvalidAddress = 1013,
    InvalidPointer = 1014,
    InvalidHandle = 1015,
    NotInitialized = 1016,
    AlreadyInitialized = 1017,
    NotFound = 1018,
    CancelRequested = 1019,
    AlreadyExists = 1020,
    OutOfRange = 1021,
    Timeout = 1022,
    InvalidResultValue = 1023,
}

impl Description for CommonDescription {
    fn into_code(self) -> u32 {
        self.to_value()
    }
}

pub const ERROR_OUT_OF_MEMORY: ErrorCode = ErrorCode::new(
    Level::Fatal,
    Summary::OutOfResource,
    Module::Application,
    CommonDescription::OutOfMemory.to_value(),
);
pub const ERROR_NOT_AUTHORIZED: ErrorCode = ErrorCode::new(
    Level::Fatal,
    Summary::Internal,
    Module::Application,
    CommonDescription::NotAuthorized.to_value(),
);
