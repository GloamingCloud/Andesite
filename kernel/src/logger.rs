use core::fmt;

use conquer_once::spin::OnceCell;
use noto_sans_mono_bitmap::{
    FontWeight, RasterHeight, RasterizedChar, get_raster, get_raster_width,
};
use spinning_top::Spinlock;
use uart_16550::Uart16550;

pub static LOGGER: OnceCell<LockedLogger> = OnceCell::uninit();

#[derive(Debug, Clone, Copy)]
pub enum PixelFormat {
    Rgb,
    Bgr,
}

#[derive(Debug, Clone, Copy)]
pub struct FrameBufferInfo {
    pub bytes_len: usize,
    pub width: usize,
    pub height: usize,
    pub pixel_format: PixelFormat,
    pub stride: usize,
}

pub struct LockedLogger {
    framebuffer: Spinlock<FrameBufferWriter>,
    serial: Spinlock<SerialWriter>,
}

impl LockedLogger {
    pub fn new(framebuffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        Self {
            framebuffer: Spinlock::new(FrameBufferWriter::new(framebuffer, info)),
            serial: Spinlock::new(unsafe { SerialWriter::new() }),
        }
    }

    pub unsafe fn force_unlock(&self) {
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
    pub const BACKUP_CHAR: char = '\u{FFFD}';
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
