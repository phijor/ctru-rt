use crate::{
    os::{Handle, WeakHandle},
    result::{Result, ResultCode},
};

use core::{convert::TryInto, ops::Try, time::Duration};

extern "C" {
    fn svcOutputDebugString(message: *const u8, length: usize);
    fn svcExitProcess();
    fn svcControlMemory(
        dest: *mut usize,
        addr0: usize,
        addr1: usize,
        size: usize,
        op: u32,
        permission: u32,
    ) -> ResultCode;
    fn svcCreateMemoryBlock(
        memory_handle: *mut u32,
        address: *const u8,
        size: usize,
        my_permissions: u32,
        other_permissions: u32,
    ) -> ResultCode;
    fn svcMapMemoryBlock(
        handle: u32,
        address: usize,
        size: usize,
        my_permissions: u32,
        other_permissions: u32,
    ) -> ResultCode;
    fn svcGetSystemInfo(out: *mut i64, sysinfo_type: u32, param: i32) -> ResultCode;
    fn svcSleepThread(ns: u64) -> ResultCode;
    fn svcConnectToPort(out_handle: *mut u32, port_name: *const u8) -> ResultCode;
    fn svcSendSyncRequest(handle: u32) -> ResultCode;
    fn svcCloseHandle(handle: u32) -> ResultCode;
    fn svcDuplicateHandle(copy: *mut u32, original: u32) -> ResultCode;
}

pub fn output_debug_string(message: &str) {
    unsafe {
        asm!(
            "svc 0x3d",
            in("r0") message.as_ptr(),
            in("r1") message.len(),
        );
    }
    // unsafe { svcOutputDebugString(message.as_ptr(), message.len()) }
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
    unsafe {
        asm!(
            "svc 0x3c",
            in("r0") (reason as u32),
        );
    }

    loop {}
}

pub fn exit_process() -> ! {
    unsafe { svcExitProcess() }

    loop {}
}

pub mod mem {
    use crate::os::MemoryRegion;

    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    pub enum MemoryOperationTarget {
        Heap = 0x0_0000,
        Linear = 0x1_0000,
    }

    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    pub enum MemoryOperationRegion {
        App = (MemoryRegion::Application as u32) << 16,
        System = (MemoryRegion::System as u32) << 16,
        Base = (MemoryRegion::Base as u32) << 16,
    }

    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    pub enum MemoryOperationAction {
        Free = 1,
        Reserve = 2,
        Allocate = 3,
        Map = 4,
        Unmap = 5,
        ChangeProtection = 6,
    }

    #[derive(Debug, Clone, Copy)]
    pub struct MemoryOperation(pub(crate) u32);

    impl MemoryOperation {
        #[inline]
        pub const fn new(
            action: MemoryOperationAction,
            region: MemoryOperationRegion,
            target: MemoryOperationTarget,
        ) -> Self {
            Self((action as u32) | (region as u32) | (target as u32))
        }

        #[inline]
        pub const fn allocate() -> Self {
            Self(MemoryOperationAction::Allocate as u32)
        }
    }

    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    pub enum MemoryPermission {
        None = 0,
        R = 1,
        W = 2,
        Rw = 3,
        X = 4,
        Rx = 5,
        Wx = 6,
        Rwx = 7,
        DontCare = 0x1000_0000,
    }

    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    pub enum MemoryState {
        Free = 0,
        Reserved = 1,
        Io = 2,
        Static = 3,
        Code = 4,
        Private = 5,
        Shared = 6,
        Continuous = 7,
        Aliased = 8,
        Alias = 9,
        AliasCode = 10,
        Locked = 11,
    }

    #[derive(Debug)]
    pub struct QueryResult {
        pub base_process_virtual_address: usize,
        pub size: usize,
        pub permission: MemoryPermission,
        pub state: MemoryState,
        pub page_flags: u32,
    }
}

pub unsafe fn query_memory(addr: usize) -> Result<mem::QueryResult> {
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

    ResultCode::from(result_code).map(|| mem::QueryResult {
        base_process_virtual_address,
        size,
        permission,
        state: state,
        page_flags,
    })
}

pub unsafe fn control_memory(
    addr0: usize,
    addr1: usize,
    size: usize,
    op: mem::MemoryOperation,
    permission: mem::MemoryPermission,
) -> Result<usize> {
    let mut dest: usize = 0;
    svcControlMemory(
        &mut dest as *mut usize,
        addr0,
        addr1,
        size,
        op.0,
        permission as u32,
    )?;

    Ok(dest)
}

pub unsafe fn create_memory_block(
    address: *const u8,
    size: usize,
    my_permissions: mem::MemoryPermission,
    other_permissions: mem::MemoryPermission,
) -> Result<Handle> {
    let mut memory_handle = 0;
    svcCreateMemoryBlock(
        &mut memory_handle,
        address,
        size,
        my_permissions as u32,
        other_permissions as u32,
    )?;

    Ok(Handle::new(memory_handle))
}

pub unsafe fn map_memory_block<'h>(
    handle: WeakHandle<'h>,
    address: usize,
    my_permissions: mem::MemoryPermission,
    other_permissions: mem::MemoryPermission,
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

pub unsafe fn get_system_info(sysinfo_type: u32, param: i32) -> Result<i64> {
    let mut out: i64 = 0;
    svcGetSystemInfo(&mut out as *mut i64, sysinfo_type, param)?;
    Ok(out)
}

pub fn sleep_thread(duration: Duration) -> ResultCode {
    let ns: u64 = duration.as_nanos().try_into().unwrap_or(u64::max_value());
    unsafe { svcSleepThread(ns) }
}

pub fn connect_to_port(port_name: &str) -> Result<Handle> {
    let mut out_handle: u32 = 0;
    unsafe {
        svcConnectToPort(&mut out_handle, port_name.as_ptr())?;
        Ok(Handle::new(out_handle))
    }
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

    ResultCode::from(result_code).map(|| unsafe { Handle::new(out_handle) })
}

pub unsafe fn send_sync_request(handle: WeakHandle, command_buffer: *mut u32) -> Result<*mut u32> {
    svcSendSyncRequest(handle.as_raw())?;
    Ok(command_buffer)
}

pub fn close_handle(handle: WeakHandle) -> ResultCode {
    unsafe { svcCloseHandle(handle.as_raw()) }
}

pub fn duplicate_handle(handle: WeakHandle) -> Result<Handle> {
    let mut out_handle: u32 = 0;
    unsafe {
        svcDuplicateHandle(&mut out_handle, handle.as_raw())?;
        Ok(Handle::new(out_handle))
    }
}
