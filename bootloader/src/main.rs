#![no_std]
#![no_main]

use elf::{abi::PT_LOAD, endian::AnyEndian};
use uefi::{
    CStr16, Error, Identify, Result, Status, boot::{self, AllocateType, MemoryDescriptor, MemoryType, SearchType}, mem::memory_map::{MemoryMap, MemoryMapIter}, println, proto::media::{
        file::{File, FileAttribute, FileInfo, FileMode, FileType},
        fs::SimpleFileSystem,
    }
};
use x86_64::structures::paging::{FrameAllocator, Size4KiB};

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
                    let frame = x86_64::structures::paging::PhysFrame::containing_address(x86_64::PhysAddr::new(self.next_page));
                    self.next_page += 4096;
                    return Some(frame);
                }
            }

            self.current_region = self.iter.find(|d| d.ty == MemoryType::CONVENTIONAL);
            if self.current_region.is_none() { return None; }
        }
    }
}

#[uefi::entry]
fn main() -> uefi::Status {
    bootloader_inner().unwrap();

    unreachable!()
}

fn bootloader_inner() -> Result<()> {
    let kernel_slice = read_file("kernel").unwrap();
    let parsed_kernel = elf::ElfBytes::<AnyEndian>::minimal_parse(kernel_slice)
        .map_err(|_| Error::new(Status::INVALID_PARAMETER, ()))?;

    let mmap_iter = boot::memory_map(MemoryType::LOADER_DATA)?;

    let mut frame_allocator = UefiFrameAllocator::new(mmap_iter.entries());

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

    println!("file loading into 0x{:x}", file_ptr as usize);

    unsafe { file_ptr.write_bytes(0, file_info.file_size() as usize) }

    let file_slice =
        unsafe { core::slice::from_raw_parts_mut(file_ptr, file_info.file_size() as usize) };

    file.read(file_slice)?;

    Ok(file_slice)
}
