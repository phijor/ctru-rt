// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::result::{self, ErrorCode as SystemErrorCode};

use core::{fmt, num::NonZeroU32};

/// Error number returned (negated) by the socket module.
///
/// The numbering is not POSIX/Linux; it follows `_net_error_code_map` in libctru
/// (`source/services/soc/soc_common.c`), which is based on libogc's `network_wii.c`.
///
/// This is a plain `u32` rather than an enum so that error numbers outside the table still fit
/// without a separate tag. Known numbers are available as associated constants and can be used
/// in patterns.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct SocketErrorCode(NonZeroU32);

const _: () = assert!(size_of::<Option<SocketErrorCode>>() == size_of::<u32>());

impl SocketErrorCode {
    pub(super) const fn new(errno: NonZeroU32) -> Self {
        Self(errno)
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }

    const fn from_raw(errno: u32) -> Self {
        match NonZeroU32::new(errno) {
            Some(errno) => Self(errno),
            None => panic!("Socket error number must be non-zero"),
        }
    }
}

macro_rules! socket_error_codes {
    ($($name:ident = $errno:literal,)*) => {
        #[allow(clippy::upper_case_acronyms)]
        impl SocketErrorCode {
            $(pub const $name: Self = Self::from_raw($errno);)*

            /// The symbolic name of this error number, if it is in libctru's table.
            pub const fn name(self) -> Option<&'static str> {
                match self.get() {
                    $($errno => Some(stringify!($name)),)*
                    _ => None,
                }
            }
        }
    };
}

socket_error_codes! {
    E2BIG = 1,
    EACCES = 2,
    EADDRINUSE = 3,
    EADDRNOTAVAIL = 4,
    EAFNOSUPPORT = 5,
    EAGAIN = 6,
    EALREADY = 7,
    EBADF = 8,
    EBADMSG = 9,
    EBUSY = 10,
    ECANCELED = 11,
    ECHILD = 12,
    ECONNABORTED = 13,
    ECONNREFUSED = 14,
    ECONNRESET = 15,
    EDEADLK = 16,
    EDESTADDRREQ = 17,
    EDOM = 18,
    EDQUOT = 19,
    EEXIST = 20,
    EFAULT = 21,
    EFBIG = 22,
    EHOSTUNREACH = 23,
    EIDRM = 24,
    EILSEQ = 25,
    EINPROGRESS = 26,
    EINTR = 27,
    EINVAL = 28,
    EIO = 29,
    EISCONN = 30,
    EISDIR = 31,
    ELOOP = 32,
    EMFILE = 33,
    EMLINK = 34,
    EMSGSIZE = 35,
    EMULTIHOP = 36,
    ENAMETOOLONG = 37,
    ENETDOWN = 38,
    ENETRESET = 39,
    ENETUNREACH = 40,
    ENFILE = 41,
    ENOBUFS = 42,
    ENODATA = 43,
    ENODEV = 44,
    ENOENT = 45,
    ENOEXEC = 46,
    ENOLCK = 47,
    ENOLINK = 48,
    ENOMEM = 49,
    ENOMSG = 50,
    ENOPROTOOPT = 51,
    ENOSPC = 52,
    ENOSR = 53,
    ENOSTR = 54,
    ENOSYS = 55,
    ENOTCONN = 56,
    ENOTDIR = 57,
    ENOTEMPTY = 58,
    ENOTSOCK = 59,
    ENOTSUP = 60,
    ENOTTY = 61,
    ENXIO = 62,
    EOPNOTSUPP = 63,
    EOVERFLOW = 64,
    EPERM = 65,
    EPIPE = 66,
    EPROTO = 67,
    EPROTONOSUPPORT = 68,
    EPROTOTYPE = 69,
    ERANGE = 70,
    EROFS = 71,
    ESPIPE = 72,
    ESRCH = 73,
    ESTALE = 74,
    ETIME = 75,
    ETIMEDOUT = 76,
}

impl fmt::Debug for SocketErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => f.debug_tuple("Unknown").field(&self.get()).finish(),
        }
    }
}

#[derive(Debug)]
pub enum SocketError {
    /// The IPC request failed, or the socket module rejected it with a result code.
    SystemErr(SystemErrorCode),
    /// The socket operation itself failed.
    SocketErr(SocketErrorCode),
}

impl From<SystemErrorCode> for SocketError {
    fn from(e: SystemErrorCode) -> Self {
        SocketError::SystemErr(e)
    }
}

impl From<SocketErrorCode> for SocketError {
    fn from(e: SocketErrorCode) -> Self {
        SocketError::SocketErr(e)
    }
}

impl From<SocketError> for result::ErrorCode {
    fn from(error: SocketError) -> Self {
        match error {
            SocketError::SystemErr(error_code) => error_code,
            SocketError::SocketErr(socket_error_code) => {
                use result::{CommonDescription, ErrorCode, Level, Module, Summary};
                type E = SocketErrorCode;

                let summary = match socket_error_code {
                    E::EAGAIN => Summary::WouldBlock,
                    E::EMFILE | E::ENFILE | E::ENOBUFS | E::ENOMEM => Summary::OutOfResource,
                    E::EINVAL => Summary::InvalidArgument,
                    E::ENOTSUP | E::EOPNOTSUPP | E::EAFNOSUPPORT | E::EPROTONOSUPPORT => {
                        Summary::NotSupported
                    }
                    _ => Summary::Internal,
                };

                // `Module::Soc` would let the errno collide with the socket module's own result
                // codes (descriptions 4, 7, 10, 11), so mark this as an encoding of our own.
                // The description field is 10 bits wide; don't let large unknown errnos wrap around
                // onto known ones.
                let description = match socket_error_code.get() {
                    errno @ 0..=1023 => errno,
                    _ => CommonDescription::InvalidResultValue.into(),
                };

                ErrorCode::new(Level::Permanent, summary, Module::Application, description)
            }
        }
    }
}

pub type Result<T> = ::core::result::Result<T, SocketError>;
