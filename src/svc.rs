// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::os::mem::MemoryState;
use crate::os::reslimit::LimitType;
use crate::os::AsHandle;
use crate::{
    os::{
        mem::{MemoryOperation, MemoryPermission, QueryResult},
        BorrowedHandle, OwnedHandle, RawHandle,
    },
    result::Result,
    sync::{ArbitrationType, ResetType},
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
    let permission: MemoryPermission = core::mem::transmute(permission);
    let state: MemoryState = core::mem::transmute(state);

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

/// # SAFETY:
///     * `stacktop` must be aligned to 8 bytes
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

/// Pause the current thread for the specified duration.
pub fn sleep_thread(duration: Timeout) {
    let (ns_high, ns_low) = (duration.reg_high(), duration.reg_low());

    unsafe {
        let _ = svc!(0x0a: (ns_low, ns_high));
    }
}

pub fn get_thread_priority(handle: BorrowedHandle) -> Result<i32> {
    unsafe { svc!(0x0b: (_, handle) -> i32) }
}

pub fn create_mutex(initially_locked: bool) -> Result<OwnedHandle> {
    unsafe { svc!(0x13: (initially_locked) -> OwnedHandle) }
}

pub fn release_mutex(handle: BorrowedHandle) -> Result<()> {
    unsafe { svc!(0x14: (handle)) }
}

pub fn create_event(reset_type: ResetType) -> Result<OwnedHandle> {
    let reset_type = reset_type as u32;

    unsafe { svc!(0x17: (reset_type) -> OwnedHandle) }
}

pub fn signal_event(handle: BorrowedHandle) -> Result<()> {
    unsafe { svc!(0x18: (handle)) }
}

pub fn clear_event(handle: BorrowedHandle) -> Result<()> {
    unsafe { svc!(0x19: (handle)) }
}

pub unsafe fn create_memory_block(
    address: usize,
    size: usize,
    my_permissions: MemoryPermission,
    other_permissions: MemoryPermission,
) -> Result<OwnedHandle> {
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
    let (ns_high, ns_low) = (timeout.reg_high(), timeout.reg_low());

    unsafe { svc!(0x24: (handle, _, ns_high, ns_low)) }
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
    let signaled = wait_synchronization_many(handles, WAIT_ALL, timeout)?;

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
        asm!("svc 0x28", lateout("r0") tick_high, lateout("r1") tick_low);
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
    unsafe { asm!("svc 0xff") }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timeout(i64);

impl Timeout {
    pub const fn from_nanoseconds(nanoseconds: i64) -> Self {
        Self(nanoseconds)
    }

    pub const fn from_seconds(seconds: i64) -> Self {
        Self(seconds * 1_000_000_000)
    }

    pub const fn forever() -> Self {
        Self::from_nanoseconds(i64::MAX)
    }

    pub const fn none() -> Self {
        Self::from_nanoseconds(0)
    }

    #[inline]
    pub(crate) const fn reg_high(self) -> u32 {
        ((self.0 as u64) >> 32) as u32
    }

    #[inline]
    pub(crate) const fn reg_low(self) -> u32 {
        self.0 as u64 as u32
    }
}

impl From<Duration> for Timeout {
    fn from(duration: Duration) -> Self {
        match duration.as_nanos().try_into() {
            Ok(ns) => Self::from_nanoseconds(ns),
            Err(_) => Self::forever(),
        }
    }
}

impl From<i64> for Timeout {
    fn from(nanoseconds: i64) -> Self {
        Self::from_nanoseconds(nanoseconds)
    }
}
