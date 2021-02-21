use crate::{
    os::{
        mem::{MemoryOperation, MemoryPermission, QueryResult},
        Handle, WeakHandle,
    },
    result::{Result, ResultCode},
    sync::ResetType,
};

use core::{convert::TryInto, time::Duration};

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

pub trait IntoRegister {
    type Register;
    unsafe fn into_register(self) -> Self::Register;
}

impl IntoRegister for u32 {
    type Register = u32;
    unsafe fn into_register(self) -> u32 {
        self
    }
}

impl IntoRegister for i32 {
    type Register = i32;
    unsafe fn into_register(self) -> i32 {
        self
    }
}

impl IntoRegister for usize {
    type Register = usize;
    unsafe fn into_register(self) -> usize {
        self
    }
}

impl IntoRegister for bool {
    type Register = u32;
    unsafe fn into_register(self) -> u32 {
        self as u32
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
    let permission = permission as u32;
    let dest_addr = svc!(0x01: (op, addr0, addr1, size, permission) -> usize)?;
    Ok(dest_addr)
}

pub unsafe fn query_memory(addr: usize) -> Result<QueryResult> {
    let (base_process_virtual_address, size, permission, state, page_flags) =
        svc!(0x02: (_, _, addr) -> (usize, usize, u32, u32, u32))?;

    // TODO: yeah, let's hope the kernel always returns valid values here
    let permission = core::mem::transmute(permission);
    let state = core::mem::transmute(state);

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

/// SAFETY:
///     * `stacktop` must be aligned to 8 bytes
pub unsafe fn create_thread(
    priority: i32,
    entry_point: unsafe extern "C" fn(usize),
    argument: usize,
    stacktop: *mut u8,
    processor_id: i32,
) -> Result<Handle> {
    let entry_point = entry_point as u32;

    let thread_handle = svc!(0x08: (entry_point, argument, stacktop, processor_id) -> Handle)?;
    Ok(thread_handle)
}

pub fn exit_thread() -> ! {
    unsafe { svc!(0x09: () -> !) }
}

pub fn sleep_thread(duration: Duration) {
    let (ns_high, ns_low) = into_ns(duration);

    unsafe {
        let _ = svc!(0x0a: (ns_low, ns_high) -> ());
    }
}

pub fn create_mutex(initially_locked: bool) -> Result<Handle> {
    unsafe { svc!(0x13: (initially_locked) -> Handle) }
}

pub fn release_mutex(handle: WeakHandle) -> Result<()> {
    unsafe { svc!(0x14: (handle) -> ()) }
}

pub fn create_event(reset_type: ResetType) -> Result<Handle> {
    let reset_type = reset_type as u32;

    unsafe { svc!(0x17: (reset_type) -> Handle) }
}

pub unsafe fn create_memory_block(
    address: usize,
    size: usize,
    my_permissions: MemoryPermission,
    other_permissions: MemoryPermission,
) -> Result<Handle> {
    let my_permissions = my_permissions as u32;
    let other_permissions = other_permissions as u32;

    svc!(0x1e: (_ /*address*/, address, size, my_permissions, other_permissions) -> Handle)
}

pub unsafe fn map_memory_block<'h>(
    handle: WeakHandle<'h>,
    address: usize,
    my_permissions: MemoryPermission,
    other_permissions: MemoryPermission,
) -> Result<()> {
    let my_permissions = my_permissions as u32;
    let other_permissions = other_permissions as u32;

    svc!(0x1f: (handle, address, my_permissions, other_permissions) -> ())
}

pub unsafe fn unmap_memory_block<'h>(handle: WeakHandle<'h>, addr: usize) -> Result<()> {
    svc!(0x20: (handle, addr) -> ())
}

pub fn close_handle(handle: WeakHandle) -> Result<()> {
    unsafe { svc!(0x23: (handle) -> ()) }
}

pub fn wait_synchronization(handle: WeakHandle, timeout: Duration) -> Result<()> {
    let (ns_high, ns_low) = into_ns(timeout);

    unsafe { svc!(0x24: (handle, ns_high, ns_low) -> ()) }
}

pub fn duplicate_handle(handle: WeakHandle) -> Result<Handle> {
    unsafe { svc!(0x27: (_, handle) -> Handle) }
}

pub unsafe fn get_system_info(sysinfo_type: u32, param: i32) -> Result<i64> {
    let (out_low, out_high) = svc!(0x2a: (_, sysinfo_type, param) -> (u32, u32))?;

    Ok(((out_high as i64) << 32) | out_low as i64)
}

pub fn connect_to_port(port_name: &str) -> Result<Handle> {
    let port_name = port_name.as_ptr();

    unsafe { svc!(0x2d: (_, port_name) -> Handle) }
}

#[inline]
pub unsafe fn send_sync_request(handle: WeakHandle, command_buffer: *mut u32) -> Result<*mut u32> {
    svc!(0x32: (handle) -> ())?;
    Ok(command_buffer)
}

pub fn get_resource_limit(process_handle: WeakHandle) -> Result<Handle> {
    let mut out_handle: u32 = 0;
    let out_handle_ptr = &mut out_handle as *mut u32;
    unsafe {
        svc!(
            0x38:
            (
                out_handle_ptr, // TODO: is it really necessary to pass this in?
                process_handle,
            ) -> Handle
        )
    }
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
}

#[inline(always)]
pub(crate) fn output_debug_bytes(bytes: &[u8]) {
    let ptr = bytes.as_ptr();
    let len = bytes.len();
    unsafe {
        let _ = svc!(0x3d: (ptr, len) -> ());
    }
}

pub fn output_debug_string(message: &str) {
    output_debug_bytes(message.as_bytes())
}

#[inline]
fn into_ns(duration: Duration) -> (u32, u32) {
    let ns: u64 = duration.as_nanos().try_into().unwrap_or(u64::max_value());

    let ns_low = ns as u32;
    let ns_high = (ns >> 32) as u32;

    (ns_high, ns_low)
}
