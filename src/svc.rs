use crate::{
    os::{
        mem::{MemoryOperation, MemoryPermission, QueryResult},
        Handle, WeakHandle,
    },
    result::{Result, ResultCode},
};

use core::{convert::TryInto, time::Duration};

pub unsafe fn control_memory(
    op: MemoryOperation,
    addr0: usize,
    addr1: usize,
    size: usize,
    permission: MemoryPermission,
) -> Result<usize> {
    let result_code: u32;
    let mut dest_addr: usize;

    asm!(
        "svc 0x01",
        in("r0") op.0,
        in("r1") addr0,
        in("r2") addr1,
        in("r3") size,
        in("r4") permission as u32,
        lateout("r0") result_code,
        lateout("r1") dest_addr,
    );

    ResultCode::from(result_code).and(dest_addr)
}

pub unsafe fn query_memory(addr: usize) -> Result<QueryResult> {
    let mut result_code: u32;
    let mut base_process_virtual_address: usize;
    let mut size: usize;
    let mut permission: u32;
    let mut state: u32;
    let mut page_flags: u32;

    asm!(
        "svc 0x02",
        out("r0") result_code,
        out("r1") base_process_virtual_address,
        inout("r2") addr => size,
        out("r3") permission,
        out("r4") state,
        out("r5") page_flags,
    );

    // TODO: yeah, let's hope the kernel always returns valid values here
    let (permission, state) = unsafe {
        (
            core::mem::transmute(permission),
            core::mem::transmute(state),
        )
    };

    ResultCode::from(result_code).and_then(|| QueryResult {
        base_process_virtual_address,
        size,
        permission,
        state: state,
        page_flags,
    })
}

pub fn exit_process() -> ! {
    unsafe { asm!("svc 0x03") }

    loop {}
}

pub fn sleep_thread(duration: Duration) {
    let ns: u64 = duration.as_nanos().try_into().unwrap_or(u64::max_value());

    let ns_low = ns as u32;
    let ns_high = (ns >> 32) as u32;

    unsafe {
        asm!(
            "svc 0x0a",
            in("r0") ns_low,
            in("r1") ns_high,
        );
    }
}

pub unsafe fn create_memory_block(
    address: usize,
    size: usize,
    my_permissions: MemoryPermission,
    other_permissions: MemoryPermission,
) -> Result<Handle> {
    let mut result_code: u32;
    let mut memory_handle: u32;

    let my_permissions = my_permissions as u32;
    let other_permissions = other_permissions as u32;

    asm!(
        "svc 0x1e",
        // in("r0") address,
        in("r1") address,
        in("r2") size,
        in("r3") my_permissions,
        in("r4") other_permissions,
        lateout("r0") result_code,
        lateout("r1") memory_handle,
    );

    ResultCode::from(result_code)?;
    Ok(Handle::new(memory_handle))
}

pub unsafe fn map_memory_block<'h>(
    handle: WeakHandle<'h>,
    address: usize,
    my_permissions: MemoryPermission,
    other_permissions: MemoryPermission,
) -> ResultCode {
    let mut result_code: u32;
    let raw_handle = handle.as_raw();

    let my_permissions = my_permissions as u32;
    let other_permissions = other_permissions as u32;
    asm!(
        "svc 0x1f",
        in("r0") raw_handle,
        in("r1") address,
        in("r2") my_permissions,
        in("r3") other_permissions,
        lateout("r0") result_code,
    );

    ResultCode::from(result_code)
}

pub unsafe fn unmap_memory_block<'h>(handle: WeakHandle<'h>, addr: usize) -> ResultCode {
    let mut result_code: u32;
    let raw_handle = handle.as_raw();

    asm!(
        "svc 0x20",
        in("r0") raw_handle,
        in("r1") addr,
        lateout("r0")result_code,
    );

    ResultCode::from(result_code)
}

pub fn close_handle(handle: WeakHandle) -> ResultCode {
    let result_code: u32;
    let raw_handle = handle.into_raw();

    unsafe {
        asm!(
            "svc 0x23",
            in("r0") raw_handle,
            lateout("r0") result_code,
        );
    }

    ResultCode::from(result_code)
}

pub fn duplicate_handle(handle: WeakHandle) -> Result<Handle> {
    let result_code: u32;
    let raw_handle = handle.into_raw();
    let raw_handle_dup: u32;

    unsafe {
        asm!(
            "svc 0x27",
            in("r1") raw_handle,
            lateout("r0") result_code,
            lateout("r1") raw_handle_dup,
        );

        ResultCode::from(result_code).and_then(|| Handle::new(raw_handle_dup))
    }
}

pub unsafe fn get_system_info(sysinfo_type: u32, param: i32) -> Result<i64> {
    let result_code: u32;
    let mut out: i64;
    let mut out_low: u32;
    let mut out_high: u32;

    asm!(
        "svc 0x2a",
        in("r1") sysinfo_type,
        in("r2") param,
        lateout("r0") result_code,
        lateout("r1") out_low,
        lateout("r2") out_high,
    );

    ResultCode::from(result_code)?;

    Ok(((out_high as i64) << 32) | out_low as i64)
}

pub fn connect_to_port(port_name: &str) -> Result<Handle> {
    let result_code: u32;
    let mut port_handle: u32;
    let port_name = port_name.as_ptr();

    unsafe {
        asm!(
            "svc 0x2d",
            in("r1") port_name,
            lateout("r0") result_code,
            lateout("r1") port_handle,
        );

        ResultCode::from(result_code)?;

        Ok(Handle::new(port_handle))
    }
}

#[inline]
pub unsafe fn send_sync_request(handle: WeakHandle, command_buffer: *mut u32) -> Result<*mut u32> {
    let result_code: u32;
    let raw_handle = handle.into_raw();

    asm!(
        "svc 0x32",
        in("r0") raw_handle,
        lateout("r0") result_code,
    );

    ResultCode::from(result_code).and(command_buffer)
}

pub fn get_resource_limit(process_handle: WeakHandle) -> Result<Handle> {
    let mut result_code: u32;
    let mut out_handle: u32 = 0;

    let process_handle = process_handle.into_raw();
    let out_handle_ptr = &mut out_handle as *mut u32;
    unsafe {
        asm!(
            "svc 0x38",
            in("r0") out_handle_ptr, // TODO: is it really necessary to pass this in?
            in("r1") process_handle,
            lateout("r0") result_code,
            lateout("r1") out_handle,
        )
    }

    ResultCode::from(result_code).and_then(|| unsafe { Handle::new(out_handle) })
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
    unsafe {
        asm!(
            "svc 0x3c",
            in("r0") reason,
        );
    }

    loop {}
}

#[inline(always)]
pub(crate) fn output_debug_bytes(bytes: &[u8]) {
    unsafe {
        asm!(
            "svc 0x3d",
            in("r0") bytes.as_ptr(),
            in("r1") bytes.len(),
        );
    }
}

pub fn output_debug_string(message: &str) {
    output_debug_bytes(message.as_bytes())
}
