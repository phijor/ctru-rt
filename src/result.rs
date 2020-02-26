use core::{fmt, num::NonZeroU32, ops::Try};

pub type Result<T> = core::result::Result<T, ErrorCode>;

#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
#[must_use = "result codes must be checked for failure"]
pub struct ResultCode(u32);

#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
#[must_use = "error codes indicate failure that must be dealt with"]
pub struct ErrorCode(NonZeroU32);

impl ErrorCode {
    pub unsafe fn new_unchecked(ec: u32) -> Self {
        Self(NonZeroU32::new_unchecked(ec))
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
    pub fn new(level: Level, summary: Summary, module: u8, description: u32) -> Self {
        let level: u32 = level.into();
        let summary: u32 = summary.into();
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

    fn module(&self) -> u8 {
        ((self.value() >> 10) & 0b1111_1111) as u8
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
