#![no_std]
#![no_main]

mod logger;

use crate::logger::{FrameBufferInfo, PixelFormat};
use core::panic::PanicInfo;
use uefi::{
    Identify, Result,
    boot::{self, SearchType},
    prelude::*,
    proto::console::gop::GraphicsOutput,
};
use x86_64::PhysAddr;

#[derive(Debug, Clone, Copy)]
struct RawFrameBufferInfo {
    addr: PhysAddr,
    info: FrameBufferInfo,
}

fn init_logger() -> Result<RawFrameBufferInfo> {
    let graphics_output_protocol_handle =
        *boot::locateSize16_handle_buffer(SearchType::ByProtocol(&GraphicsOutput::GUID))?
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

#[uefi::entry]
fn main() -> Status {
    init_logger().unwrap();
    log::info!("Andesite kernel loaded!");
    log::info!("Exiting boot services...");

    let _mmap_iter = unsafe { boot::exit_boot_services(None) };

    log::info!("Boot services exited successfully!");
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
