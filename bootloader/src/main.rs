#![no_std]
#![no_main]

use common::{KernelParameters, MemMap};
use elf::endian::AnyEndian;
use uefi::{
    CStr16, Error, Identify, Result, Status,
    boot::{self, AllocateType, MemoryType, SearchType},
    proto::media::{
        file::{File, FileAttribute, FileInfo, FileMode, FileType},
        fs::SimpleFileSystem,
    },
};

#[uefi::entry]
fn main() -> uefi::Status {
    bootloader_inner().unwrap();

    unreachable!()
}

fn bootloader_inner() -> Result<()> {
    common::logger::init_logger().map_err(|_| Status::INVALID_PARAMETER)?;
    log::info!("Something isn't it?");

    let kernel_slice = read_file("kernel")?;
    let kernel_entrypoint = relocate_elf(kernel_slice)?;

    let st_ptr = uefi::table::system_table_raw()
        .expect("SystemTable not set by entry point")
        .as_ptr() as *const core::ffi::c_void;
    let memory_map = unsafe { uefi::boot::exit_boot_services(None) };

    kernel_entrypoint(KernelParameters {
        system_table: st_ptr,
        memory_map: MemMap::new(&memory_map),
    });
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

fn relocate_elf(elf_buffer: &[u8]) -> Result<extern "sysv64" fn(KernelParameters) -> !> {
    let parsed_elf = elf::ElfBytes::<AnyEndian>::minimal_parse(elf_buffer)
        .map_err(|_| Error::new(Status::INVALID_PARAMETER, ()))?;

    let mut mem_min = u64::MAX;
    let mut mem_max = u64::MIN;
    if let Some(segments) = parsed_elf.segments() {
        for program_header in segments {
            if program_header.p_type == elf::abi::PT_LOAD {
                let start_addr = program_header.p_vaddr;
                let mut end_addr = start_addr + program_header.p_memsz;
                let mask = program_header.p_align - 1;
                end_addr = (end_addr + mask) & !mask;
                if start_addr < mem_min {
                    mem_min = start_addr;
                }
                if end_addr > mem_max {
                    mem_max = end_addr;
                }
            }
        }
    }

    let pages_needed = (mem_max - mem_min + 4095) / 4096;

    let program_buffer = boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_CODE,
        pages_needed as usize,
    )?
    .as_ptr();

    unsafe {
        core::ptr::write_bytes(program_buffer, 0, pages_needed as usize * 4096);
    }

    if let Some(segments) = parsed_elf.segments() {
        for program_header in segments {
            if program_header.p_type == elf::abi::PT_LOAD {
                let relative_offset = (program_header.p_vaddr) - mem_min;
                let dst = unsafe { program_buffer.add(relative_offset as usize) };
                let src = unsafe { elf_buffer.as_ptr().add(program_header.p_offset as usize) };
                unsafe {
                    core::ptr::copy_nonoverlapping(src, dst, program_header.p_filesz as usize);
                }
            }
        }
    }

    if let Ok(Some(rela_shdr)) = parsed_elf.section_header_by_name(".rela.dyn") {
        if let Ok(relas) = parsed_elf.section_data_as_relas(&rela_shdr) {
            let load_base = program_buffer as u64 - mem_min;
            for rela in relas {
                if rela.r_type == elf::abi::R_X86_64_RELATIVE {
                    let target = unsafe {
                        program_buffer
                            .add(rela.r_offset as usize - mem_min as usize)
                            .cast::<u64>()
                    };
                    let value = load_base.wrapping_add(rela.r_addend as u64);
                    unsafe { core::ptr::write(target, value) };
                }
            }
        }
    }

    let entry_point = unsafe {
        core::mem::transmute(
            program_buffer.add(parsed_elf.ehdr.e_entry as usize - mem_min as usize),
        )
    };

    Ok(entry_point)
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
