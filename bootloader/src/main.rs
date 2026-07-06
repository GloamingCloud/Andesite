#![no_std]
#![no_main]

mod logger;

use elf::endian::AnyEndian;
use uefi::{
    CStr16, Error, Identify, Result, Status,
    boot::{self, AllocateType, MemoryType, SearchType},
    proto::media::{
        file::{File, FileAttribute, FileInfo, FileMode, FileType},
        fs::SimpleFileSystem,
    },
};

use crate::logger::LockedLogger;

#[uefi::entry]
fn main() -> uefi::Status {
    bootloader_inner().unwrap();

    unreachable!()
}

fn bootloader_inner() -> Result<()> {
    init_logger()?;
    log::info!("Something isn't it?");

    let kernel_slice = read_file("kernel").unwrap();
    let parsed_kernel = elf::ElfBytes::<AnyEndian>::minimal_parse(kernel_slice)
        .map_err(|_| Error::new(Status::INVALID_PARAMETER, ()))?;

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

fn init_logger() -> Result<()> {
    let logger = logger::LOGGER.get_or_init(move || LockedLogger::new());
    log::set_logger(logger).expect("logger already exists");
    log::set_max_level(log::LevelFilter::Debug);

    Ok(())
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
