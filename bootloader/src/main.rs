#![no_std]
#![no_main]

use core::fmt;

use conquer_once::spin::OnceCell;
use elf::endian::AnyEndian;
use noto_sans_mono_bitmap::{
    FontWeight, RasterHeight, RasterizedChar, get_raster, get_raster_width,
};
use spinning_top::Spinlock;
use uart_16550::Uart16550;
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

static LOGGER: OnceCell<LockedLogger> = OnceCell::uninit();

struct LockedLogger {
    framebuffer: Spinlock<FrameBufferWriter>,
    serial: Spinlock<SerialWriter>,
}

impl LockedLogger {
    fn new(framebuffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        Self {
            framebuffer: Spinlock::new(FrameBufferWriter::new(framebuffer, info)),
            serial: Spinlock::new(unsafe { SerialWriter::new() }),
        }
    }

    unsafe fn force_unlock(&self) {
        unsafe {
            self.framebuffer.force_unlock();
            self.serial.force_unlock();
        }
    }
}

impl log::Log for LockedLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        use core::fmt::Write;

        let file = record.file().unwrap_or("<unknown>");
        let line = record.line().unwrap_or(0);

        let framebuffer = &mut self.framebuffer.lock();
        writeln!(
            framebuffer,
            "[{:5}] [{}:{}] - {}",
            record.level(),
            file,
            line,
            record.args()
        )
        .unwrap();

        let mut serial = self.serial.lock();
        writeln!(
            serial,
            "[{:5}] [{}:{}] - {}",
            record.level(),
            file,
            line,
            record.args()
        )
        .unwrap();
    }

    fn flush(&self) {}
}

const BORDER_PADDING: usize = 1;
const LINE_SPACING: usize = 2;
const LETTER_SPACING: usize = 0;

mod font_constants {
    use super::*;

    pub const CHAR_RASTER_HEIGHT: RasterHeight = RasterHeight::Size16;
    pub const CHAR_RASTER_WIDTH: usize = get_raster_width(FontWeight::Regular, CHAR_RASTER_HEIGHT);
    pub const BACKUP_CHAR: char = '�';
    pub const FONT_WEIGHT: FontWeight = FontWeight::Regular;
}

struct FrameBufferWriter {
    framebuffer: &'static mut [u8],
    info: FrameBufferInfo,
    x_pos: usize,
    y_pos: usize,
}

impl FrameBufferWriter {
    fn new(framebuffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        let mut logger = Self {
            framebuffer,
            info,
            x_pos: 0,
            y_pos: 0,
        };
        logger.clear();
        logger
    }

    fn width(&self) -> usize {
        self.info.width
    }

    fn height(&self) -> usize {
        self.info.height
    }

    fn clear(&mut self) {
        self.x_pos = BORDER_PADDING;
        self.y_pos = BORDER_PADDING;
        self.framebuffer.fill(0);
    }

    fn newline(&mut self) {
        self.y_pos += font_constants::CHAR_RASTER_HEIGHT.val() + LINE_SPACING;
        self.carriage_return();
    }

    fn carriage_return(&mut self) {
        self.x_pos = BORDER_PADDING;
    }

    fn write_pixel(&mut self, x: usize, y: usize, intensity: u8) {
        let pixel_offset = y * self.info.stride + x;
        let color = match self.info.pixel_format {
            PixelFormat::Rgb => [intensity, intensity, intensity / 2, 0],
            PixelFormat::Bgr => [intensity / 2, intensity, intensity, 0],
        };
        let byte_offset = pixel_offset * 4;
        self.framebuffer[byte_offset..(byte_offset + 4)].copy_from_slice(&color[..4]);
        let _ = unsafe {
            core::ptr::read_volatile(&self.framebuffer[byte_offset]);
        };
    }

    fn write_rendered_char(&mut self, rendered_char: RasterizedChar) {
        for (y, row) in rendered_char.raster().iter().enumerate() {
            for (x, byte) in row.iter().enumerate() {
                self.write_pixel(self.x_pos + x, self.y_pos + y, *byte);
            }
        }

        self.x_pos += rendered_char.width() + LETTER_SPACING;
    }

    fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.carriage_return(),
            c => {
                let new_x_pos = self.x_pos + font_constants::CHAR_RASTER_WIDTH;
                if new_x_pos >= self.width() {
                    self.newline();
                }
                let new_y_pos =
                    self.y_pos + font_constants::CHAR_RASTER_HEIGHT.val() + BORDER_PADDING;
                if new_y_pos >= self.height() {
                    self.clear();
                }
                self.write_rendered_char(
                    get_raster(
                        c,
                        font_constants::FONT_WEIGHT,
                        font_constants::CHAR_RASTER_HEIGHT,
                    )
                    .unwrap_or_else(|| {
                        get_raster(
                            font_constants::BACKUP_CHAR,
                            font_constants::FONT_WEIGHT,
                            font_constants::CHAR_RASTER_HEIGHT,
                        )
                        .expect("should not panic")
                    }),
                );
            }
        }
    }
}

impl fmt::Write for FrameBufferWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c)
        }

        Ok(())
    }
}

struct SerialWriter {
    port: uart_16550::Uart16550<uart_16550::backend::PioBackend>,
}

impl SerialWriter {
    unsafe fn new() -> Self {
        let mut port = unsafe { Uart16550::new_port(0x3f8) }.unwrap();
        port.init(uart_16550::Config::default()).unwrap();
        Self { port }
    }
}

impl fmt::Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.port.send_bytes_exact(s.as_bytes());

        Ok(())
    }
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
    let kernel_slice = read_file("kernel").unwrap();
    let parsed_kernel = elf::ElfBytes::<AnyEndian>::minimal_parse(kernel_slice)
        .map_err(|_| Error::new(Status::INVALID_PARAMETER, ()))?;

    let framebuffer = init_logger()?;
    log::info!("Something isn't it?");

    let mut mmap_iter = unsafe { boot::exit_boot_services(None) };
    mmap_iter.sort();
    let mut frame_allocator = UefiFrameAllocator::new(mmap_iter.entries());

    let page_tables = create_page_tables(&mut frame_allocator, &framebuffer)?;

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

    let logger = LOGGER.get_or_init(move || LockedLogger::new(slice, info));
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

fn create_page_tables(
    frame_allocator: &mut UefiFrameAllocator,
    framebuffer: &RawFrameBufferInfo,
) -> Result<PageTables> {
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

        let start_addr = VirtAddr::new(framebuffer.addr.as_u64());
        let end_addr = start_addr + framebuffer.info.bytes_len as u64;
        for p4 in usize::from(start_addr.p4_index())..=usize::from(end_addr.p4_index()) {
            new_p4[p4] = old_p4[p4].clone();
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

    unsafe {
        LOGGER
            .get()
            .map(|l| l.force_unlock())
    };
    log::error!("{}", info);

    loop {
        unsafe { asm!("cli; hlt") };
    }
}
