// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::svc::Timeout;

/// Events that [`Soc::poll`] waits for and reports.
///
/// The values follow 3dbrew [`SOCU:poll`][poll].
///
/// [poll]: https://www.3dbrew.org/wiki/SOCU:poll#Events
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(super) struct PollEvents(u32);

impl PollEvents {
    /// Data can be read.
    pub(super) const IN: Self = Self(0x01);
    /// Urgent data can be read.
    pub(super) const PRI: Self = Self(0x02);
    /// Normal data can be written.
    pub(super) const WRNORM: Self = Self(0x08);
    /// Priority data can be written.
    pub(super) const WRBAND: Self = Self(0x10);
    /// The socket handle is invalid.
    pub(super) const NVAL: Self = Self(0x20);
    /// Data can be written.
    pub(super) const OUT: Self = Self::WRNORM;

    pub(super) const fn empty() -> Self {
        Self(0)
    }

    pub(super) const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(super) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl core::ops::BitOr for PollEvents {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// An entry of the array passed to [`Soc::poll`], laid out like the socket module's `pollfd`.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(super) struct PollFd {
    pub fd: u32,
    pub events: PollEvents,
    pub revents: PollEvents,
}

const _: () = assert!(size_of::<PollFd>() == 0xc);

impl PollFd {
    pub const fn new(fd: u32, events: PollEvents) -> Self {
        Self {
            fd,
            events,
            revents: PollEvents::empty(),
        }
    }

    pub fn as_in_bytes(fds: &[Self]) -> &[u8] {
        let data = fds.as_ptr().cast::<u8>();
        // SAFETY: `PollFd` consists of three `u32`s, so it has no padding.
        unsafe { core::slice::from_raw_parts(data, size_of_val(fds)) }
    }

    pub fn as_out_bytes(fds: &mut [Self]) -> &mut [u8] {
        let data = fds.as_mut_ptr().cast::<u8>();
        // SAFETY: `PollFd` has no padding, and any bit pattern is a valid `PollFd`.
        unsafe { core::slice::from_raw_parts_mut(data, size_of_val(fds)) }
    }
}

/// Convert a timeout to whole milliseconds for `poll`, rounding up; -1 waits indefinitely.
pub(super) fn timeout_to_ms(timeout: Timeout) -> i32 {
    let ns = timeout.as_nanoseconds();
    if ns < 0 || timeout == Timeout::infinite() {
        return -1;
    }

    (ns as u64)
        .div_ceil(1_000_000)
        .try_into()
        .unwrap_or(i32::MAX)
}
