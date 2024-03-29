// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::ipc::{IpcRequest, StaticBuffer};
use crate::os::mem::MemoryPermission;
use crate::os::{
    sharedmem::{MappedBlock, SharedMemoryMapper},
    AsHandle, OwnedHandle, BorrowedHandle,
};
use crate::ports::srv::Srv;
use crate::result::{ErrorCode, Result};
use crate::svc::Timeout;
use crate::sync::{Event, ResetType};

use log::{debug, trace, warn};

use core::sync::atomic::{AtomicU32, Ordering};

use ctru_rt_macros::EnumCast;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumCast)]
#[enum_cast(value_type = "usize")]
pub enum Screen {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScreenDimensions {
    pub width: u16,
    pub height: u16,
}

impl Screen {
    pub(crate) const fn dimensions(&self) -> ScreenDimensions {
        match self {
            Self::Top => ScreenDimensions {
                width: 240,
                height: 400,
            },
            Self::Bottom => ScreenDimensions {
                width: 240,
                height: 320,
            },
        }
    }

    pub(crate) const fn dimensions_register(&self) -> u32 {
        let dim = self.dimensions();
        (dim.height as u32) << 16 | (dim.width as u32)
    }
}

#[repr(packed)]
struct InterruptHeader {
    current_index: u8,
    events_total: u8,
    error: u8,
    _unused: u8,
}

impl From<u32> for InterruptHeader {
    fn from(header: u32) -> Self {
        let bytes = header.to_le_bytes();
        Self {
            current_index: bytes[0],
            events_total: bytes[1],
            error: bytes[2],
            _unused: bytes[3],
        }
    }
}

impl From<InterruptHeader> for u32 {
    fn from(header: InterruptHeader) -> Self {
        u32::from_le_bytes([
            header.current_index,
            header.events_total,
            header.error,
            header._unused,
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumCast)]
#[enum_cast(value_type = "u8")]
pub enum InterruptEvent {
    PSC0,
    PSC1,
    VBlank0,
    VBlank1,
    PPF,
    P3D,
    DMA,
}

#[derive(Debug)]
struct Sharedmem {
    gpu_events: Event,
    gsp_module_thread_index: u8,
    shared_memory: MappedBlock,
}

pub struct InterruptEventSet(u32);

impl InterruptEventSet {
    const fn empty() -> Self {
        Self(0)
    }

    fn add(&mut self, event: InterruptEvent) {
        self.0 |= 1 << event.to_value();
    }

    pub fn contains(&self, event: InterruptEvent) -> bool {
        self.0 & (1 << event.to_value()) != 0
    }
}

impl core::fmt::Debug for InterruptEventSet {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        use InterruptEvent::*;
        let mut set = f.debug_set();
        for event in &[PSC0, PSC1, VBlank0, VBlank1, PPF, P3D, DMA] {
            if self.contains(*event) {
                set.entry(event);
            }
        }

        set.finish()
    }
}

#[derive(Debug, EnumCast, Clone, Copy, PartialEq, Eq)]
#[enum_cast(value_type = "u8")]
pub enum FramebufferIndex {
    First,
    Second,
}

impl FramebufferIndex {
    fn swap(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::First,
        }
    }
}

impl core::ops::Not for FramebufferIndex {
    type Output = Self;
    fn not(self) -> Self::Output {
        self.swap()
    }
}

#[repr(C)]
struct FramebufferInfoInner {
    active_framebuffer: u32,
    fb0_vaddr: u32,
    fb1_vaddr: u32,
    stride: u32,
    format: u32,
    display_select: u32,
    unknown: u32,
}

struct FramebufferInfo {
    info: *mut u32,
}

struct FramebufferInfoHeader(u32);

impl FramebufferInfoHeader {
    fn update_index(&self, index: FramebufferIndex) -> Self {
        const FB_INDEX: usize = 0;
        const FB_UPDATE: usize = 1;

        let mut updated: [u8; 4] = self.0.to_le_bytes();
        updated[FB_INDEX] = index.to_value();
        updated[FB_UPDATE] = 1;
        Self(u32::from_le_bytes(updated))
    }
}

impl FramebufferInfo {
    fn header(&self) -> &AtomicU32 {
        unsafe { &*(self.info as *const AtomicU32) }
    }

    fn load_index(&self, order: Ordering) -> FramebufferIndex {
        let index = self.header().load(order).to_le_bytes()[0];
        FramebufferIndex::from_value(index).expect("Invalid framebuffer index from GSP")
    }

    fn info_at(&self, index: FramebufferIndex) -> *mut FramebufferInfoInner {
        unsafe {
            (self.info.offset(1) as *mut FramebufferInfoInner).offset(index.to_value() as isize)
        }
    }

    fn trigger_update(&self, active_fb: FramebufferIndex) {
        let header = self.header();
        let mut current = FramebufferInfoHeader(header.load(Ordering::Acquire));
        loop {
            let updated = current.update_index(active_fb);

            match header.compare_exchange_weak(
                current.0,
                updated.0,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(new) => current = FramebufferInfoHeader(new),
            }
        }
    }

    #[inline]
    fn update(
        &mut self,
        active_fb: FramebufferIndex,
        fb0: *const u8,
        fb1: *const u8,
        stride: u32,
        format: u32,
    ) {
        debug!("Updating framebuffer: active = {:?}, fb0 = {:p}, fb1 = {:p}, stride = {}, format = {:b}", active_fb, fb0, fb1, stride, format);
        {
            let active_fb = u32::from(active_fb.to_value());
            let fb_info = FramebufferInfoInner {
                active_framebuffer: active_fb,
                fb0_vaddr: fb0 as u32,
                fb1_vaddr: fb1 as u32,
                stride,
                format,
                display_select: active_fb,
                unknown: 0,
            };

            let next_index = self.load_index(Ordering::Acquire).swap();
            unsafe {
                self.info_at(next_index).write(fb_info);
            }

            core::sync::atomic::fence(Ordering::Release);
        }

        self.trigger_update(active_fb)
    }
}

struct InterruptInfo {
    event_buf: *const u32,
}

impl InterruptInfo {
    fn header(&self) -> &AtomicU32 {
        unsafe { &*(self.event_buf.offset(0) as *const AtomicU32) }
    }

    fn load_header(&self, order: Ordering) -> InterruptHeader {
        let raw_header = self.header().load(order);

        InterruptHeader::from(raw_header)
    }

    fn store_header(
        &self,
        current: InterruptHeader,
        new: InterruptHeader,
        success: Ordering,
        failure: Ordering,
    ) -> core::result::Result<(), InterruptHeader> {
        match self
            .header()
            .compare_exchange(current.into(), new.into(), success, failure)
        {
            Ok(_) => Ok(()),
            Err(updated) => Err(InterruptHeader::from(updated)),
        }
    }

    unsafe fn read_event(&self, header: &InterruptHeader) -> Option<InterruptEvent> {
        let index = header.current_index;
        let block_idx = usize::from(index / 4);
        let part_idx = usize::from(index & 0b11);
        let current_event_ptr = self.event_buf.offset(0x3 + block_idx as isize) as *const AtomicU32;
        let event_packed = (&*current_event_ptr).load(Ordering::SeqCst);

        match InterruptEvent::from_value(event_packed.to_le_bytes()[3 - part_idx]) {
            Err(e) => {
                warn!("GSP wrote an invalid interrupt event: 0x{:02x}", e);
                None
            }
            Ok(event) => Some(event),
        }
    }
}

impl Sharedmem {
    fn wait_event(&mut self) -> Result<InterruptEventSet> {
        self.gpu_events.wait(Timeout::forever())?;

        self.gpu_events.clear()?;

        let mut events = InterruptEventSet::empty();
        while let Some(event) = self.pop_interrupt() {
            events.add(event)
        }

        Ok(events)
    }

    fn interrupt_info(&self) -> InterruptInfo {
        let event_buf = unsafe {
            self.shared_memory
                .as_ptr()
                .offset(self.gsp_module_thread_index as isize * 0x10)
        };
        InterruptInfo { event_buf }
    }

    fn pop_interrupt(&self) -> Option<InterruptEvent> {
        let info = self.interrupt_info();

        let mut header = info.load_header(Ordering::Acquire);
        loop {
            if header.events_total == 0 {
                return None;
            }

            let event = unsafe { info.read_event(&header) }?;

            let acknowledged = InterruptHeader {
                current_index: if header.current_index >= 0x34 {
                    0
                } else {
                    header.current_index + 1
                },
                events_total: header.events_total - 1,
                error: 0,
                _unused: header._unused,
            };

            match info.store_header(header, acknowledged, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Some(event),
                Err(updated) => {
                    header = updated;
                }
            }
        }
    }

    unsafe fn framebuffer_info_for(&mut self, screen: Screen) -> FramebufferInfo {
        const INFO_BASE: isize = 0x80;
        const SIZE: isize = 0x20;
        const SCREEN_OFFSET: usize = 0x10;

        let base = self.shared_memory.as_mut_ptr().offset(INFO_BASE);
        let screen_offset = (screen.to_value() * SCREEN_OFFSET) as isize;
        let info = base
            .offset(self.gsp_module_thread_index as isize * SIZE)
            .offset(screen_offset);

        FramebufferInfo { info }
    }

    pub fn present_buffer(
        &mut self,
        screen: Screen,
        active_fb: FramebufferIndex,
        fb0: *const u8,
        fb1: *const u8,
        stride: u32,
        mode: u32,
    ) {
        let mut fb_info = unsafe { self.framebuffer_info_for(screen) };

        fb_info.update(active_fb, fb0, fb1, stride, mode)
    }
}

#[derive(Debug)]
pub struct Gpu {
    access: AccessRightsToken,
    sharedmem: Sharedmem,
}

impl Gpu {
    pub fn init(srv: &Srv) -> Result<Self> {
        let service_handle = srv.get_service_handle("gsp::Gpu")?;

        const ACCESS_FLAGS: u8 = 0x00;
        let mut access =
            Self::aquire_access(service_handle, BorrowedHandle::active_process(), ACCESS_FLAGS)?;

        const QUEUE_FLAGS: u8 = 0x01;
        let gsp_relay_queue = Self::register_interrupt_relay_queue(&mut access, QUEUE_FLAGS)?;

        Ok(Self {
            access,
            sharedmem: gsp_relay_queue,
        })
    }

    fn aquire_access(
        service_handle: OwnedHandle,
        owner_process: BorrowedHandle,
        flags: u8,
    ) -> Result<AccessRightsToken> {
        let _reply = IpcRequest::command(0x16)
            .parameter(u32::from(flags))
            .translate_parameter(owner_process)
            .dispatch(&service_handle)?;

        Ok(AccessRightsToken { service_handle })
    }

    fn register_interrupt_relay_queue(
        access: &mut AccessRightsToken,
        flags: u8,
    ) -> Result<Sharedmem> {
        use crate::result::{Level, Module, Summary};

        let gpu_events = Event::new(ResetType::OneShot)?;

        let (result_code, gsp_module_thread_index, queue_handle) = {
            let (result_code, mut reply) = IpcRequest::command(0x13)
                .parameter(flags as u32)
                .translate_parameter(gpu_events.as_handle())
                .dispatch_no_fail(access.as_handle())?;

            (result_code, (reply.read_word() & 0xff) as u8, unsafe {
                reply.finish_results().read_handle()
            })
        };

        const RESULT_NEED_HW_INIT: ErrorCode =
            ErrorCode::new(Level::Success, Summary::Success, Module::Gsp, 519);

        match result_code.into_result() {
            Ok(()) => {}
            Err(RESULT_NEED_HW_INIT) => Self::init_hardware(access)?,
            Err(e) => return Err(e),
        };

        let shared_memory = Self::map_shared_memory(queue_handle)?;

        Ok(Sharedmem {
            gpu_events,
            gsp_module_thread_index,
            shared_memory,
        })
    }

    fn map_shared_memory(shared_memory_handle: OwnedHandle) -> Result<MappedBlock> {
        SharedMemoryMapper::global().map(
            shared_memory_handle,
            0x1000,
            MemoryPermission::Rw,
            MemoryPermission::DontCare,
        )
    }

    fn init_hardware(access: &mut AccessRightsToken) -> Result<()> {
        const GPU_REG_BASE: u32 = 0x40_0000;
        const INIT_VALUES: &[(u32, u32, Option<u32>)] = &[
            (0x1000, 0, None),
            (0x1080, 0x12345678, None),
            (0x10C0, 0xFFFFFFF0, None),
            (0x10D0, 1, None),
            // (0x1914, 1, None), // homebrew addition: make sure GPUREG_START_DRAW_FUNC0 starts off in configuration mode
            // Top screen LCD configuration, see https://www.3dbrew.org/wiki/GPU/External_Registers#LCD_Source_Framebuffer_Setup

            // Top screen sync registers:
            (0x0400, 0x1C2, None),
            (0x0404, 0xD1, None),
            (0x0408, 0x1C1, None),
            (0x040C, 0x1C1, None),
            (0x0410, 0, None),
            (0x0414, 0xCF, None),
            (0x0418, 0xD1, None),
            (0x041C, (0x1C5 << 16) | 0x1C1, None),
            (0x0420, 0x10000, None),
            (0x0424, 0x19D, None),
            (0x0428, 2, None),
            (0x042C, 0x192, None),
            (0x0430, 0x192, None),
            (0x0434, 0x192, None),
            (0x0438, 1, None),
            (0x043C, 2, None),
            (0x0440, (0x196 << 16) | 0x192, None),
            (0x0444, 0, None),
            (0x0448, 0, None),
            // Top screen fb geometry
            (0x045C, (400 << 16) | 240, None), // dimensions
            (0x0460, (0x1C1 << 16) | 0xD1, None),
            (0x0464, (0x192 << 16) | 2, None),
            // Top screen framebuffer format (initial)
            (0x0470, 0x80340, None),
            // Top screen unknown reg @ 0x9C
            (0x049C, 0, None),
            // Bottom screen LCD configuration

            // Bottom screen sync registers:
            (0x0500, 0x1C2, None),
            (0x0504, 0xD1, None),
            (0x0508, 0x1C1, None),
            (0x050C, 0x1C1, None),
            (0x0510, 0xCD, None),
            (0x0514, 0xCF, None),
            (0x0518, 0xD1, None),
            (0x051C, (0x1C5 << 16) | 0x1C1, None),
            (0x0520, 0x10000, None),
            (0x0524, 0x19D, None),
            (0x0528, 0x52, None),
            (0x052C, 0x192, None),
            (0x0530, 0x192, None),
            (0x0534, 0x4F, None),
            (0x0538, 0x50, None),
            (0x053C, 0x52, None),
            (0x0540, (0x198 << 16) | 0x194, None),
            (0x0544, 0, None),
            (0x0548, 0x11, None),
            // Bottom screen fb geometry
            (0x055C, Screen::Bottom.dimensions_register(), None), // dimensions
            (0x0560, (0x1C1 << 16) | 0xD1, None),
            (0x0564, (0x192 << 16) | 0x52, None),
            // Bottom screen framebuffer format (initial)
            (0x0570, 0x80300, None),
            // Bottom screen unknown reg @ 0x9C
            (0x059C, 0, None),
            // Initial, blank framebuffer (top left A/B, bottom A/B, top right A/B)
            (0x0468, 0x18300000, None),
            (0x046C, 0x18300000, None),
            (0x0568, 0x18300000, None),
            (0x056C, 0x18300000, None),
            (0x0494, 0x18300000, None),
            (0x0498, 0x18300000, None),
            // Framebuffer select: A
            (0x0478, 1, None),
            (0x0578, 1, None),
            // Clear DMA transfer (PPF) "transfer finished" bit
            (0x0C18, 0, Some(0xFF00)),
            // GX_GPU_CLK |= 0x70000 (value is 0x100 when gsp starts, enough to at least display framebuffers & have memory fill work)
            // This enables the clock to some GPU components
            (0x0004, 0x70100, None),
            // Clear Memory Fill (PSC0 and PSC1) "busy" and "finished" bits
            (0x001C, 0, Some(0xFF)),
            (0x002C, 0, Some(0xFF)),
            // More init registers
            (0x0050, 0x22221200, None),
            (0x0054, 0xFF2, Some(0xFFFF)),
            // Enable some LCD clocks (?) (unsure)
            (0x0474, 0x10501, None),
            (0x0574, 0x10501, None),
        ];

        for (register_offset, value, mask) in INIT_VALUES {
            let register = GPU_REG_BASE + register_offset;
            trace!(
                "Writing GSP register: {:#08x} = {:#08x} (mask: {:08x?})",
                register,
                value,
                mask
            );
            match mask {
                None => {
                    write_graphics_register(access.service_handle.as_handle(), register, value)?
                }
                Some(mask) => write_graphics_register_masked(
                    access.service_handle.as_handle(),
                    register,
                    value,
                    mask,
                )?,
            }
        }

        Ok(())
    }

    pub fn next_event(&mut self) -> Result<InterruptEventSet> {
        self.sharedmem.wait_event()
    }

    pub fn present_buffer(
        &mut self,
        screen: Screen,
        active_fb: FramebufferIndex,
        fb0: *const u8,
        fb1: *const u8,
        stride: u32,
        mode: u32,
    ) {
        self.sharedmem
            .present_buffer(screen, active_fb, fb0, fb1, stride, mode)
    }

    pub fn set_lcd_force_blank(&mut self, flags: u8) -> Result<()> {
        let _ = IpcRequest::command(0x0b)
            .parameter(flags as u32)
            .dispatch(&self.access)?;
        Ok(())
    }
}

#[derive(Debug)]
#[must_use = "GPU access rights must be released properly"]
struct AccessRightsToken {
    service_handle: OwnedHandle,
}

impl AccessRightsToken {
    fn release(&mut self) -> Result<()> {
        debug!("Releasing GPU access rights");
        let _ = IpcRequest::command(0x17).dispatch(&self.service_handle)?;
        Ok(())
    }
}

impl AsHandle for AccessRightsToken {
    fn as_handle(&self) -> BorrowedHandle {
        self.service_handle.as_handle()
    }
}

impl Drop for AccessRightsToken {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

fn write_graphics_register(
    service_handle: BorrowedHandle,
    register_offset: u32,
    value: &u32,
) -> Result<()> {
    let data = core::slice::from_ref(value);
    let _ = IpcRequest::command(0x01)
        .parameters(&[register_offset, 4])
        .translate_parameter(StaticBuffer::new(data, 0))
        .dispatch(service_handle)?;

    Ok(())
}

fn write_graphics_register_masked(
    service_handle: BorrowedHandle,
    register_offset: u32,
    value: &u32,
    mask: &u32,
) -> Result<()> {
    let value = core::slice::from_ref(value);
    let mask = core::slice::from_ref(mask);
    let _ = IpcRequest::command(0x02)
        .parameters(&[register_offset, 4])
        .translate_parameter(StaticBuffer::new(value, 0))
        .translate_parameter(StaticBuffer::new(mask, 1))
        .dispatch(service_handle)?;

    Ok(())
}
