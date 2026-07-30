use core::fmt;

use conquer_once::spin::OnceCell;
use spinning_top::Spinlock;
use uart_16550::Uart16550;

use crate::{ErrorType, Result};

pub static LOGGER: OnceCell<LockedLogger> = OnceCell::uninit();

pub struct LockedLogger {
    serial: Spinlock<SerialWriter>,
}

impl LockedLogger {
    pub fn new() -> Result<Self> {
        Ok(Self {
            serial: Spinlock::new(unsafe { SerialWriter::new()? }),
        })
    }

    #[allow(dead_code)]
    pub unsafe fn force_unlock(&self) {
        unsafe {
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

struct SerialWriter {
    port: uart_16550::Uart16550<uart_16550::backend::PioBackend>,
}

impl SerialWriter {
    unsafe fn new() -> Result<Self> {
        let mut port =
            unsafe { Uart16550::new_port(0x3f8) }.map_err(|_| ErrorType::InvalidArgument)?;
        port.init(uart_16550::Config::default())
            .map_err(|_| ErrorType::DeviceNotReady)?;
        Ok(Self { port })
    }
}

impl fmt::Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.port.send_bytes_exact(s.as_bytes());

        Ok(())
    }
}

pub fn init_logger() -> crate::Result<()> {
    let logger = LockedLogger::new()?;
    let logger = LOGGER.get_or_init(move || logger);
    log::set_logger(logger).map_err(|_| ErrorType::AlreadyExist)?;
    log::set_max_level(log::LevelFilter::Debug);

    Ok(())
}
