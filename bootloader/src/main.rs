#![no_std]
#![no_main]

mod logger;

use elf::endian::AnyEndian;
use uefi::{
    CStr16, Error, Identify, Result, Status,
    boot::{self, AllocateType, MemoryDescriptor, MemoryType, SearchType},
    mem::memory_map::{MemoryMap, MemoryMapIter, MemoryMapMut},
    proto::{
        console::gop::GraphicsOutput,
        media::{
            file::{File, FileAttribute, FileInfo, FileMode, FileType},
            fs::SimpleFileSystem,
        },
    },
};
use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::Cr3Flags,
    structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB},
};

use crate::logger::LockedLogger;

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
    fn allocate_frame(&mut self) -> Option<x86_64::structures::paging::PhysFrame<Size4KiB>> {
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
                    let frame = x86_64::structures::paging::PhysFrame::containing_address(
                        x86_64::PhysAddr::new(self.next_page),
                    );
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

#[uefi::entry]
fn main() -> uefi::Status {
    bootloader_inner().unwrap();

    unreachable!()
}

fn bootloader_inner() -> Result<()> {
    let framebuffer = init_logger()?;
    log::info!("Something isn't it?");

    let kernel_slice = read_file("kernel").unwrap();
    let parsed_kernel = elf::ElfBytes::<AnyEndian>::minimal_parse(kernel_slice)
        .map_err(|_| Error::new(Status::INVALID_PARAMETER, ()))?;

    let mut mmap_iter = unsafe { boot::exit_boot_services(None) };
    mmap_iter.sort();
    let mut frame_allocator = UefiFrameAllocator::new(mmap_iter.entries());

    let page_tables = create_page_tables(&mut frame_allocator)?;

    Ok(())
}

fn read_file(filename: &str) -> Result<&[u8]> {
    let simple_file_system_protocol_handle =
        *boot::locate_handle_buffer(SearchType::ByProtocol(&SimpleFileSystem::GUID))?
            .first()
            .expect("Failed to get first handle");
    let mut simple_file_system_protocol =
        boot::open_protocol_exclusive::<SimpleFileSystem>(simple_file_system_protocol_handle)?;
    let mut simple_file_system = simple_file_system_protocol.open_volume()?;

    // I don't care cause it's always 'kernel', use cstr16! macro!
    let mut buffer = [0u16; 8];
    let mut file_handle = simple_file_system.open(
        CStr16::from_str_with_buf(filename, &mut buffer).unwrap(),
        FileMode::Read,
        FileAttribute::READ_ONLY,
    )?;

    let mut buffer = [0u8; 1024];
    let file_info = file_handle
        .get_info::<FileInfo>(&mut buffer)
        .expect("failed to get file info");
    let mut file = match file_handle.into_type()? {
        FileType::Regular(file) => file,
        FileType::Dir(_) => unimplemented!("file expected: {}", filename),
    };
    let file_ptr = boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        file_info.file_size() as usize,
    )?
    .as_ptr();

    log::info!("file loading into 0x{:x}", file_ptr as usize);

    unsafe { file_ptr.write_bytes(0, file_info.file_size() as usize) }

    let file_slice =
        unsafe { core::slice::from_raw_parts_mut(file_ptr, file_info.file_size() as usize) };

    file.read(file_slice)?;

    Ok(file_slice)
}

#[derive(Debug, Clone, Copy)]
enum PixelFormat {
    Rgb,
    Bgr,
}

#[derive(Debug, Clone, Copy)]
struct FrameBufferInfo {
    bytes_len: usize,
    width: usize,
    height: usize,
    pixel_format: PixelFormat,
    stride: usize,
}

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

    let logger = logger::LOGGER.get_or_init(move || LockedLogger::new(slice, info));
    log::set_logger(logger).expect("logger already exists");
    log::set_max_level(log::LevelFilter::Debug);

    Ok(RawFrameBufferInfo {
        addr: PhysAddr::new(framebuffer.as_mut_ptr() as u64),
        info,
    })
}

struct PageTables {
    bootloader: OffsetPageTable<'static>,
    kernel: OffsetPageTable<'static>,
    kernel_frame: PhysFrame,
}

fn create_page_tables(frame_allocator: &mut UefiFrameAllocator) -> Result<PageTables> {
    let phys_offset = VirtAddr::zero();

    let mut bootloader_page_table = {
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

        unsafe { x86_64::registers::control::Cr3::write(new_p4_frame, Cr3Flags::empty()) };
        unsafe { OffsetPageTable::new(new_p4, phys_offset) }
    };

    let (kernel_page_table, kernel_page_table_frame) = {
        let frame = frame_allocator
            .allocate_frame()
            .expect("failed to allocate frame");
        let addr = phys_offset + frame.start_address().as_u64();
        let ptr = addr.as_mut_ptr();
        unsafe { *ptr = PageTable::new() };
        let l4_table = unsafe { &mut *ptr };
        (
            unsafe { OffsetPageTable::new(l4_table, phys_offset) },
            frame,
        )
    };

    Ok(PageTables {
        bootloader: bootloader_page_table,
        kernel: kernel_page_table,
        kernel_frame: kernel_page_table_frame,
    })
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::arch::asm;

    unsafe { logger::LOGGER.get().map(|l| l.force_unlock()) };
    log::error!("{}", info);

    loop {
        unsafe { asm!("cli; hlt") };
    }
}
