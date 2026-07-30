#![no_std]
#![no_main]

mod elf;
mod memory;

use uefi::{
    CStr16, Identify, Result, Status,
    boot::{self, AllocateType, MemoryType, SearchType},
    mem::memory_map::{MemoryMap, MemoryMapMut},
    proto::media::{
        file::{File, FileAttribute, FileInfo, FileMode, FileType},
        fs::SimpleFileSystem,
    },
};
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, OffsetPageTable, PageTable},
};

use crate::memory::UefiLegacyFrameAllocator;

#[uefi::entry]
fn main() -> uefi::Status {
    bootloader_inner().unwrap();

    unreachable!()
}

fn bootloader_inner() -> Result<()> {
    common::logger::init_logger().map_err(|_| Status::INVALID_PARAMETER)?;
    log::info!("Starting bootloader...");

    let kernel_slice = read_file("kernel")?;

    let mut memory_map = unsafe { boot::exit_boot_services(None) };
    memory_map.sort();

    let mut frame_allocator = UefiLegacyFrameAllocator::new(memory_map.entries());

    let (mut kernel_page_table, kernel_page_table_frame) = {
        let frame = frame_allocator
            .allocate_frame()
            .expect("failed to allocate frame for kernel page table");
        let addr = VirtAddr::zero() + frame.start_address().as_u64();
        let ptr = addr.as_mut_ptr();
        unsafe { *ptr = PageTable::new() };
        let level_4_page_table = unsafe { &mut *ptr };

        // Copy the lower 256 entries from the active UEFI PML4 table to preserve identity mapping
        let (active_p4_frame, _) = x86_64::registers::control::Cr3::read();
        let active_p4_ptr = active_p4_frame.start_address().as_u64() as *const PageTable;
        let active_p4 = unsafe { &*active_p4_ptr };
        for i in 0..256 {
            level_4_page_table[i] = active_p4[i].clone();
        }

        (
            unsafe { OffsetPageTable::new(level_4_page_table, VirtAddr::zero()) },
            frame,
        )
    };

    let high_base = VirtAddr::new(0xffffffff80000000);
    

    // Retrieve raw system table pointer
    let system_table = uefi::table::system_table_raw()
        .expect("Failed to get raw system table")
        .as_ptr() as *const core::ffi::c_void;

    // Prepare kernel parameters with system table pointer
    let params = common::KernelParameters {
        system_table,
    };

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
    let pages_needed = (file_info.file_size() as usize + 4095) / 4096;
    let file_ptr = boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        pages_needed,
    )?
    .as_ptr();

    log::info!("file loading into 0x{:x}", file_ptr as usize);

    unsafe { file_ptr.write_bytes(0, file_info.file_size() as usize) }

    let file_slice =
        unsafe { core::slice::from_raw_parts_mut(file_ptr, file_info.file_size() as usize) };

    file.read(file_slice)?;

    Ok(file_slice)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::arch::asm;

    unsafe { common::logger::LOGGER.get().map(|l| l.force_unlock()) };
    log::error!("{}", info);

    loop {
        unsafe { asm!("cli; hlt") };
    }
}
