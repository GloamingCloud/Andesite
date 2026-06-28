#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod logger;

use core::panic::PanicInfo;
use uefi::{
    prelude::*,
    boot::{self, SearchType, MemoryDescriptor, MemoryType},
    proto::console::gop::GraphicsOutput,
    mem::memory_map::{MemoryMap, MemoryMapIter, MemoryMapMut},
    Result, Identify,
};
use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::Cr3Flags,
    structures::paging::{
        FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PhysFrame, Size4KiB, PageTableFlags
    },
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
};
use conquer_once::spin::OnceCell;
use crate::logger::{FrameBufferInfo, PixelFormat};

#[derive(Debug, Clone, Copy)]
struct RawFrameBufferInfo {
    addr: PhysAddr,
    info: FrameBufferInfo,
}

fn init_logger() -> Result<RawFrameBufferInfo> {
    let graphics_output_protocol_handle =
        *boot::locate_handle_buffer(SearchType::ByProtocol(&GraphicsOutput::GUID))?
            .first()
            .expect("failed to locate graphics output handle");
    let mut graphics_output_protocol =
        boot::open_protocol_exclusive::<GraphicsOutput>(graphics_output_protocol_handle)?;

    let modes = graphics_output_protocol.modes();

    let mode = modes
        .filter(|m| m.info().resolution().0 == 1920 && m.info().resolution().1 == 1080)
        .last()
        .expect("failed to find 1920x1080 mode");

    graphics_output_protocol.set_mode(&mode)?;

    let mode_info = graphics_output_protocol.current_mode_info();
    let mut framebuffer = graphics_output_protocol.frame_buffer();
    let slice =
        unsafe { core::slice::from_raw_parts_mut(framebuffer.as_mut_ptr(), framebuffer.size()) };

    let info = FrameBufferInfo {
        bytes_len: slice.len(),
        width: mode_info.resolution().0,
        height: mode_info.resolution().1,
        pixel_format: match mode_info.pixel_format() {
            uefi::proto::console::gop::PixelFormat::Rgb => PixelFormat::Rgb,
            uefi::proto::console::gop::PixelFormat::Bgr => PixelFormat::Bgr,
            _ => unimplemented!("unsupported pixel format"),
        },
        stride: mode_info.stride(),
    };

    let logger = logger::LOGGER.get_or_init(move || logger::LockedLogger::new(slice, info));
    log::set_logger(logger).expect("logger already exists");
    log::set_max_level(log::LevelFilter::Debug);

    Ok(RawFrameBufferInfo {
        addr: PhysAddr::new(framebuffer.as_mut_ptr() as u64),
        info,
    })
}

struct UefiFrameAllocator<'a> {
    iter: MemoryMapIter<'a>,
    current_region: Option<&'a MemoryDescriptor>,
    next_page: u64,
}

impl<'a> UefiFrameAllocator<'a> {
    fn new(iter: MemoryMapIter<'a>) -> Self {
        Self {
            iter,
            current_region: None,
            next_page: 0,
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for UefiFrameAllocator<'_> {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        loop {
            if let Some(desc) = self.current_region {
                let start = desc.phys_start;
                let end = start + (desc.page_count * 4096);

                if self.next_page < start {
                    self.next_page = start;
                }

                if self.next_page == 0 {
                    self.next_page = 4096;
                }

                if self.next_page < end {
                    let frame = PhysFrame::containing_address(PhysAddr::new(self.next_page));
                    self.next_page += 4096;
                    return Some(frame);
                }
            }

            self.current_region = self.iter.find(|d| d.ty == MemoryType::CONVENTIONAL);
            if self.current_region.is_none() {
                return None;
            }
        }
    }
}

fn configure_virtual_memory(
    memory_map: &impl MemoryMap,
) -> OffsetPageTable<'static> {
    let mut frame_allocator = UefiFrameAllocator::new(memory_map.entries());
    let phys_offset = VirtAddr::zero();

    // 1. Clone the current UEFI page table
    let old_p4_frame = x86_64::registers::control::Cr3::read().0;
    let old_p4 = unsafe { &*(old_p4_frame.start_address().as_u64() as *const PageTable) };

    let new_p4_frame = frame_allocator.allocate_frame().expect("out of memory");
    let new_p4 = unsafe { &mut *(new_p4_frame.start_address().as_u64() as *mut PageTable) };
    new_p4.zero();

    for i in 0..512 {
        if !old_p4[i].is_unused() {
            new_p4[i] = old_p4[i].clone();
        }
    }

    // Write new P4 to CR3 to switch to our new cloned page table
    unsafe { x86_64::registers::control::Cr3::write(new_p4_frame, Cr3Flags::empty()) };

    // Create an OffsetPageTable mapper pointing to the new table
    let mut page_table = unsafe { OffsetPageTable::new(new_p4, phys_offset) };

    // 2. Map all physical memory regions to the physical offset 0xffff_8000_0000_0000
    let physical_memory_offset = VirtAddr::new(0xffff_8000_0000_0000);

    for desc in memory_map.entries() {
        let start_addr = PhysAddr::new(desc.phys_start);
        let num_pages = desc.page_count;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

        for i in 0..num_pages {
            let offset = i * 4096;
            let phys_frame = PhysFrame::<Size4KiB>::containing_address(start_addr + offset);
            let virt_page = Page::<Size4KiB>::containing_address(
                physical_memory_offset + (start_addr.as_u64() + offset),
            );

            unsafe {
                match page_table.map_to(virt_page, phys_frame, flags, &mut frame_allocator) {
                    Ok(tlb_flush) => tlb_flush.flush(),
                    Err(x86_64::structures::paging::mapper::MapToError::PageAlreadyMapped(_)) => {
                        // Page is already mapped, which is fine
                    }
                    Err(e) => {
                        log::error!("Failed to map page: {:?}", e);
                    }
                }
            }
        }
    }

    page_table
}

static IDT: OnceCell<InterruptDescriptorTable> = OnceCell::uninit();

pub fn init_idt() {
    let idt = IDT.get_or_init(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt
    });
    idt.load();
}

extern "x86-interrupt" fn breakpoint_handler(
    stack_frame: InterruptStackFrame
) {
    log::info!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    log::error!("page fault");
}

#[uefi::entry]
fn main() -> Status {
    init_logger().unwrap();
    log::info!("Andesite kernel loaded");
    log::info!("Exiting boot services...");

    let mut memory_map = unsafe { boot::exit_boot_services(None) };
    memory_map.sort();

    log::info!("Boot services exited successfully!");

    let _page_table = configure_virtual_memory(&memory_map);
    log::info!("Virtual memory configured successfully with physical memory offset!");

    init_idt();
    log::info!("IDT initialized successfully!");

    x86_64::instructions::interrupts::int3();
    log::info!("Breakpoint exception handled successfully!");

    let ptr = 0xdead_be00_0000u64 as *mut u64;
    
    unsafe {
        // 尝试向该地址写入数据，CPU 会瞬间断流并抛出 #PF
        core::ptr::write_volatile(ptr, 0x42);
    }

    log::info!("Kernel running in long mode!");

    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::arch::asm;

    unsafe { logger::LOGGER.get().map(|l| l.force_unlock()) };
    log::error!("{}", info);

    loop {
        unsafe { asm!("cli; hlt") };
    }
}
