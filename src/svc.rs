// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::os::AsHandle;
use crate::os::mem::MemoryState;
use crate::os::reslimit::LimitType;
use crate::{
    os::{
        BorrowedHandle, OwnedHandle, RawHandle, SystemTick,
        mem::{MemoryOperation, MemoryPermission, QueryResult},
    },
    result::Result,
    sync::{ResetType, arbitrate::ArbitrationType},
};

use core::arch::asm;
use core::{
    convert::{TryFrom, TryInto},
    time::Duration,
};

use ctru_rt_macros::svc;

pub trait FromRegister {
    unsafe fn from_register(reg: u32) -> Self;
}

impl FromRegister for u32 {
    unsafe fn from_register(reg: u32) -> Self {
        reg
    }
}

impl FromRegister for usize {
    unsafe fn from_register(reg: u32) -> Self {
        reg as usize
    }
}

impl FromRegister for i32 {
    unsafe fn from_register(reg: u32) -> Self {
        reg as i32
    }
}

pub trait IntoRegister {
    type Register;
    unsafe fn into_register(self) -> Self::Register;
}

macro_rules! into_register_implicit {
    ($($t:ty as $into:ty),* $(,)?) => {
        $(
            impl IntoRegister for $t {
                type Register = $into;

                unsafe fn into_register(self) -> Self::Register {
                    self as Self::Register
                }
            }
        )*
    }
}

into_register_implicit! {
    u32 as u32,
    i32 as i32,
    usize as usize,
    bool as u32,
    MemoryPermission as u32,
}

impl IntoRegister for unsafe extern "C" fn(usize) {
    type Register = u32;

    unsafe fn into_register(self) -> Self::Register {
        self as usize as u32
    }
}

impl<T> IntoRegister for *mut T {
    type Register = *mut T;
    unsafe fn into_register(self) -> *mut T {
        self
    }
}

impl<T> IntoRegister for *const T {
    type Register = *const T;
    unsafe fn into_register(self) -> *const T {
        self
    }
}

pub unsafe fn control_memory(
    op: MemoryOperation,
    addr0: usize,
    addr1: usize,
    size: usize,
    permission: MemoryPermission,
) -> Result<usize> {
    let op = op.0;
    let dest_addr = svc!(0x01: (op, addr0, addr1, size, permission) -> usize)?;
    Ok(dest_addr)
}

pub unsafe fn query_memory(addr: usize) -> Result<QueryResult> {
    let (base_process_virtual_address, size, permission, state, page_flags) =
        svc!(0x02: (_, _, addr) -> (usize, usize, u32, u32, u32))?;

    // TODO: yeah, let's hope the kernel always returns valid values here
    let permission: MemoryPermission = unsafe { core::mem::transmute(permission) };
    let state: MemoryState = unsafe { core::mem::transmute(state) };

    Ok(QueryResult {
        base_process_virtual_address,
        size,
        permission,
        state,
        page_flags,
    })
}

pub fn exit_process() -> ! {
    unsafe { svc!(0x03: () -> !) }
}

/// Create a new thread on the specified processor.
///
/// In the newly spawned thread, `argument` is passed to `entry_point`.
/// Thread `priority` is in the range of `[0x18, 0x30]`, lower values mean higher priority.
///
/// # Safety
///
/// `stacktop` must point to an 8-byte aligned allocation.
pub unsafe fn create_thread(
    priority: i32,
    entry_point: unsafe extern "C" fn(usize),
    argument: usize,
    stacktop: *mut u8,
    processor_id: i32,
) -> Result<OwnedHandle> {
    svc!(0x08: (priority, entry_point, argument, stacktop, processor_id) -> OwnedHandle)
}

pub fn exit_thread() -> ! {
    unsafe { svc!(0x09: () -> !) }
}

/// Pause the current thread for at least `duration`.
///
/// Saturates at `i64::MAX` nanoseconds (~292 years).
/// A zero duration yields to other threads, see [`yield_thread`].
/// To sleep indefinitely, use [`sleep_thread_forever`].
#[inline]
pub fn sleep_thread(duration: Duration) {
    // This system call takes a single argument: a signed 64-bit integer of nanoseconds. According
    // to [kernel-decomp], it sleeps for positive durations, yields for zero, and ignores negative
    // values. Valid durations thus range from `0` to `i64::MAX` nanoseconds.
    //
    // [kernel-decomp]: https://gist.github.com/TuxSH/f1f270710ed84576823fd1da348acb38/3dd82f31299c999ede0ea8f899aafefc974b7fb0#file-k11_latest_n3ds-cpp-L10588-L10597
    let ns = duration.as_nanos().min(i64::MAX as u128) as u64;
    let (ns_low, ns_high) = (ns as u32, (ns >> 32) as u32);

    unsafe {
        let _ = svc!(0x0a: (ns_low, ns_high));
    }
}

/// Give up the rest of the current thread's time slice to other threads.
#[inline]
pub fn yield_thread() {
    sleep_thread(Duration::ZERO)
}

/// Pause the current thread indefinitely.
pub fn sleep_thread_forever() -> ! {
    // It is not possible to pass a truly infinite timeout to `sleep_thread`.
    // Instead, sleep for the longest possible finite duration in a loop.
    loop {
        sleep_thread(Duration::MAX)
    }
}

pub fn get_thread_priority(handle: BorrowedHandle) -> Result<i32> {
    unsafe { svc!(0x0b: (_, handle) -> i32) }
}

pub fn create_mutex(initially_locked: bool) -> Result<OwnedHandle> {
    unsafe { svc!(0x13: (_, initially_locked) -> OwnedHandle) }
}

pub fn release_mutex(handle: BorrowedHandle) -> Result<()> {
    unsafe { svc!(0x14: (handle)) }
}

pub fn create_event(reset_type: ResetType) -> Result<OwnedHandle> {
    let reset_type = reset_type as u32;

    unsafe { svc!(0x17: (_, reset_type) -> OwnedHandle) }
}

pub fn signal_event(handle: BorrowedHandle) -> Result<()> {
    unsafe { svc!(0x18: (handle)) }
}

pub fn clear_event(handle: BorrowedHandle) -> Result<()> {
    unsafe { svc!(0x19: (handle)) }
}

/// Create a handle to a memory block to be shared with other processes.
///
/// The block is backed by an allocation at `ptr`. Its contents may be uninitialized.
///
/// # Safety
///
/// - `ptr` must be page-aligned, and `ptr.len()` must be a nonzero multiple of the page size (0x1000).
/// - The memory must stay allocated until the block is gone, i.e. until the returned handle
///   is closed and all processes that mapped the block have unmapped it.
/// - While the block exists, other processes may access the memory at any time (subject to
///   `other_permissions`). The caller must not create references to it; access it only
///   through raw pointers.
pub unsafe fn create_memory_block(
    ptr: *mut [u8],
    my_permissions: MemoryPermission,
    other_permissions: MemoryPermission,
) -> Result<OwnedHandle> {
    let address = ptr as *mut u8;
    let size = ptr.len();
    svc!(0x1e: (other_permissions, address, size, my_permissions) -> OwnedHandle)
}

pub unsafe fn map_memory_block(
    handle: BorrowedHandle,
    address: usize,
    my_permissions: MemoryPermission,
    other_permissions: MemoryPermission,
) -> Result<()> {
    svc!(0x1f: (handle, address, my_permissions, other_permissions))
}

pub unsafe fn unmap_memory_block(handle: BorrowedHandle, addr: usize) -> Result<()> {
    svc!(0x20: (handle, addr))
}

pub fn create_address_arbiter() -> Result<OwnedHandle> {
    unsafe { svc!(0x21: () -> OwnedHandle) }
}

pub fn arbitrate_address(
    handle: BorrowedHandle,
    address: usize,
    arbitration_type: ArbitrationType,
    value: i32,
    timeout: Timeout,
) -> Result<()> {
    let (ns_low, ns_high) = (timeout.reg_low(), timeout.reg_high());
    unsafe { svc!(0x22: (handle, address, arbitration_type, value, ns_low, ns_high)) }
}

pub unsafe fn close_handle(handle: RawHandle) -> Result<()> {
    svc!(0x23: (handle))
}

pub fn wait_synchronization(handle: BorrowedHandle, timeout: Timeout) -> Result<()> {
    debug_assert!(
        handle.is_open(),
        "svc::wait_synchronization(): Received closed handle!"
    );
    let (ns_low, ns_high) = (timeout.reg_low(), timeout.reg_high());

    // The 64-bit timeout is passed in an even register pair: low word in r2, high word in r3.
    unsafe { svc!(0x24: (handle, _, ns_low, ns_high)) }
}

pub fn wait_synchronization_many(
    handles: &[BorrowedHandle],
    wait_all: bool,
    timeout: Timeout,
) -> Result<i32> {
    let (ns_high, ns_low) = (timeout.reg_high(), timeout.reg_low());
    let num_handles = handles.len();
    let handles: *const BorrowedHandle = handles.as_ptr();

    let signaled = unsafe { svc!(0x25: (ns_low, handles, num_handles, wait_all, ns_high) -> i32) }?;

    Ok(signaled)
}

const WAIT_FIRST: bool = false;
const WAIT_ALL: bool = true;

pub fn wait_synchronization_all(handles: &[BorrowedHandle], timeout: Timeout) -> Result<()> {
    let _ = wait_synchronization_many(handles, WAIT_ALL, timeout)?;

    Ok(())
}

pub fn wait_synchronization_any(handles: &[BorrowedHandle], timeout: Timeout) -> Result<usize> {
    let signaled = wait_synchronization_many(handles, WAIT_FIRST, timeout)?;

    match usize::try_from(signaled) {
        Ok(index) => Ok(index),
        Err(_) => panic!(
            "svc::wait_synchronization_any returned negative handle index {}, \
            expected a non-negative index into a list of {} handles",
            signaled,
            handles.len(),
        ),
    }
}

pub fn duplicate_handle(handle: BorrowedHandle) -> Result<OwnedHandle> {
    unsafe { svc!(0x27: (_, handle) -> OwnedHandle) }
}

pub fn get_system_tick_count() -> u64 {
    let tick_low: u32;
    let tick_high: u32;
    unsafe {
        asm!(
            "svc 0x28",
            lateout("r0") tick_low,
            lateout("r1") tick_high,
            // The kernel only preserves r4–r11.
            lateout("r2") _,
            lateout("r3") _,
            lateout("r12") _,
        );
    }
    (tick_high as u64) << 32 | tick_low as u64
}

pub unsafe fn get_system_info(sysinfo_type: u32, param: i32) -> Result<i64> {
    let (out_low, out_high) = svc!(0x2a: (_, sysinfo_type, param) -> (u32, u32))?;

    Ok(((out_high as i64) << 32) | out_low as i64)
}

pub fn connect_to_port(port_name: &str) -> Result<OwnedHandle> {
    let port_name = port_name.as_ptr();

    unsafe { svc!(0x2d: (_, port_name) -> OwnedHandle) }
}

#[inline]
pub unsafe fn send_sync_request(
    handle: BorrowedHandle,
    command_buffer: *mut u32,
) -> Result<*mut u32> {
    svc!(0x32: (handle))?;
    Ok(command_buffer)
}

pub fn get_process_id(process_handle: BorrowedHandle) -> Result<u32> {
    unsafe { svc!(0x35: (_, process_handle) -> u32) }
}

pub fn get_resource_limit(process_handle: BorrowedHandle) -> Result<OwnedHandle> {
    let mut out_handle: u32 = 0;
    let out_handle_ptr = &mut out_handle as *mut u32;
    unsafe {
        svc!(
            0x38:
            (
                out_handle_ptr, // TODO: is it really necessary to pass this in?
                process_handle,
            ) -> OwnedHandle
        )
    }
}

pub fn get_resource_limit_values<const N: usize>(
    limits_handle: BorrowedHandle,
    values: &mut [i64; N],
    limit_types: &[LimitType; N],
) -> Result<()> {
    let values = values.as_mut_ptr();
    let limit_types = limit_types.as_ptr();

    unsafe { svc!(0x39: (values, limits_handle, limit_types, N)) }
}

pub fn get_resource_limit_current_values<const N: usize>(
    limits_handle: BorrowedHandle,
    values: &mut [i64; N],
    limit_types: &[LimitType; N],
) -> Result<()> {
    let values = values.as_mut_ptr();
    let limit_types = limit_types.as_ptr();

    unsafe { svc!(0x3a: (values, limits_handle, limit_types, N)) }
}

#[derive(Debug)]
pub enum UserBreakReason {
    Panic = 0,
    Assert = 1,
    User = 2,
    LoadRo = 3,
    UnloadRo = 4,
}

pub fn user_break(reason: UserBreakReason) -> ! {
    let reason = reason as u32;
    unsafe { svc!(0x3c: (reason) -> !) }
    #[allow(unreachable_code)]
    loop {
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}

#[inline(always)]
#[doc(hidden)]
pub fn output_debug_bytes(bytes: &[u8]) {
    let ptr = bytes.as_ptr();
    let len = bytes.len();
    unsafe {
        let _ = svc!(0x3d: (ptr, len));
    }
}

pub fn output_debug_string(message: &str) {
    output_debug_bytes(message.as_bytes())
}

/// Invalidate the CPU data cache at `[addr, addr + size)` for a given process.
///
/// Discards cache lines so that subsequent CPU reads go to memory.  This makes the given range of
/// main memory visible to the process.  Use it before reading memory that another bus master (e.g.
/// GPU, DSP, ARM9) has written.
///
/// The kernel writes back (see [`clean_process_data_cache`]) dirty cache lines before discarding
/// them depending on `size`. This is unlike an ARM [_invalidate_ cache operation][arm-trm].
/// Do not use this to discard CPU writes.
///
/// # Errors
///
/// * [`crate::result::ERROR_INVALID_POINTER`]: `addr` is null.
/// * [`crate::result::ERROR_INVALID_ADDRESS`]: `addr >= 0x4000_0000`, or `addr + size` exceeds that bound.
/// * [`crate::result::ERROR_INVALID_HANDLE`]:  `process` does not refer to a process.
///
/// # References
///
/// `KPageTable::InvalidateDataCacheRange` in [TuxSH's Kernel11 decompilation][tuxsh-kernel].
///
/// Section 3.4.18 "c7, Cache Operations Register" (page 3-45) of the [ARM11 MPCore Processor
/// Technical Reference Manual][arm-trm] (ARM DDI 0360F).
///
/// [arm-trm]: https://support.arm.com/documentation/ddi0360/f/control-coprocessor-cp15/register-descriptions/c7--cache-operations-register?lang=en
/// [tuxsh-kernel]: https://gist.github.com/TuxSH/f1f270710ed84576823fd1da348acb38
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn invalidate_process_data_cache(
    process: BorrowedHandle,
    addr: *mut u8,
    size: usize,
) -> Result<()> {
    unsafe { svc!(0x52: (process, addr, size)) }
}

/// Clean (write back) the CPU data cache at `[addr, addr + size)` for a given process.
///
/// Use this to publish writes made by a process to another bus master (e.g. GPU, DSP, ARM9) while
/// letting it continue to read the same data itself.  Dirty cache lines are written out to main
/// memory but stay valid, so subsequent CPU reads by this process still hit the cache.
///
/// # Errors
///
/// See [`invalidate_process_data_cache`].
///
/// # References
///
/// This is called `StoreProcessDataCache` in the [3DBrew docs](https://www.3dbrew.org/wiki/SVC),
/// but corresponds to an ARM [_clean_ operation][arm-trm].
///
/// [arm-trm]: https://support.arm.com/documentation/ddi0360/f/control-coprocessor-cp15/register-descriptions/c7--cache-operations-register?lang=en
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn clean_process_data_cache(
    process: BorrowedHandle,
    addr: *const u8,
    size: usize,
) -> Result<()> {
    unsafe { svc!(0x53: (process, addr, size)) }
}

/// Flush (clean and invalidate) CPU data cache at `[addr, addr + size)` for a given process.
///
/// Use this to make modifications within this memory range performed by this process visible in
/// main memory.
///
/// This is a combination of [`clean_process_data_cache`] and [`invalidate_process_data_cache`]:
/// Dirty cache lines are written back to main memory *and* dropped from the cache, so this both
/// publishes writes made by this process and forces the next read to come from memory.
///
/// # Errors
///
/// See [`invalidate_process_data_cache`].
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn flush_process_data_cache(
    process: BorrowedHandle,
    addr: *const u8,
    size: usize,
) -> Result<()> {
    unsafe { svc!(0x54: (process, addr, size)) }
}

pub fn stop_point() {
    // The kernel only preserves r4–r11.
    unsafe {
        asm!(
            "svc 0xff",
            lateout("r0") _,
            lateout("r1") _,
            lateout("r2") _,
            lateout("r3") _,
            lateout("r12") _,
        )
    }
}

/// A system call timeout.
///
/// A timeout represents either a finite number of nanoseconds ([`Self::from_nanoseconds`]),
/// or an infinite duration, [`Self::infinite()`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timeout {
    /// The kernel encodes timeouts as *signed* 64-bit integers.
    /// The only valid bit patterns are, however:
    /// * a sentinel value encoding an infinite timeout (-1, [`Self::INFINITE`])
    /// * non-negative numbers of nanoseconds.
    ///
    /// The constructors never produce other negative values.
    /// Different syscalls interpret negative values besides -1 differently,
    /// so it is best to forbid encoding them at all.
    nanoseconds: i64,
}

impl Timeout {
    const INFINITE: i64 = -1;
    /// Roughly ~292 years.
    const MAX_FINITE_TIMEOUT: Self = Self {
        nanoseconds: i64::MAX,
    };

    /// Create a timeout from a number of nanoseconds.
    ///
    /// Saturates to [`Self::infinite`] if the duration exceeds the largest representable finite timeout.
    #[inline]
    pub const fn from_nanoseconds(nanoseconds: u64) -> Self {
        if nanoseconds > Self::MAX_FINITE_TIMEOUT.nanoseconds as u64 {
            Self::infinite()
        } else {
            Self {
                nanoseconds: nanoseconds as i64,
            }
        }
    }

    /// Create a *finite* timeout from a number of nanoseconds.
    ///
    /// Saturates to the largest representable finite timeout.
    /// Use [`Self::infinite`] to represent an infinite duration.
    #[inline]
    pub const fn from_nanoseconds_finite(nanoseconds: u64) -> Self {
        if nanoseconds > Self::MAX_FINITE_TIMEOUT.nanoseconds as u64 {
            Self::MAX_FINITE_TIMEOUT
        } else {
            Self {
                nanoseconds: nanoseconds as i64,
            }
        }
    }

    pub const fn from_seconds(seconds: u64) -> Self {
        Self::from_nanoseconds(seconds.saturating_mul(1_000_000_000))
    }

    /// Create a timeout lasting the given number of system ticks.
    ///
    /// Saturates to [`Self::infinite`], like [`Self::from_nanoseconds`].
    #[inline]
    pub const fn from_system_tick(ticks: SystemTick) -> Self {
        Self::from_nanoseconds(ticks.as_nanoseconds())
    }

    /// A timeout representing an infinite duration.
    pub const fn infinite() -> Self {
        Self {
            nanoseconds: Self::INFINITE,
        }
    }

    pub const fn none() -> Self {
        Self { nanoseconds: 0 }
    }

    /// The timeout in nanoseconds.
    ///
    /// If the timeout is [`Self::infinite()`], returns [`None`].
    pub const fn as_nanoseconds(self) -> Option<u64> {
        if self.nanoseconds == Self::INFINITE {
            None
        } else {
            Some(self.nanoseconds as u64)
        }
    }

    /// The closest *finite* timeout.
    ///
    /// If this timeout is finite, returns the value unchanged.
    /// If the timeout is [`Self::infinite()`], returns the largest representable finite timeout instead.
    pub const fn closest_finite_timeout(self) -> Self {
        if self.nanoseconds == Self::INFINITE {
            Self::MAX_FINITE_TIMEOUT
        } else {
            self
        }
    }

    #[inline]
    pub(crate) const fn reg_high(self) -> u32 {
        ((self.nanoseconds as u64) >> 32) as u32
    }

    #[inline]
    pub(crate) const fn reg_low(self) -> u32 {
        self.nanoseconds as u64 as u32
    }
}

impl From<Duration> for Timeout {
    /// Saturates at the longest finite timeout; a `Duration` is never infinite.
    fn from(duration: Duration) -> Self {
        Self::from_nanoseconds_finite(duration.as_nanos().try_into().unwrap_or(u64::MAX))
    }
}

impl From<u64> for Timeout {
    fn from(nanoseconds: u64) -> Self {
        Self::from_nanoseconds_finite(nanoseconds)
    }
}
