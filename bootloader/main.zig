const std = @import("std");
const uefi = std.os.uefi;
const log = std.log.scoped(.bootloader);
const elf = std.elf;

const blog = @import("logger.zig");

pub const std_options = std.Options{
    .logFn = blog.log,
};

pub fn main() uefi.Status {
    const console_output = uefi.system_table.con_out orelse return .aborted;
    console_output.clearScreen() catch unreachable;

    blog.init(console_output);
    log.debug("into bootloader, logger working", .{});

    const boot_service = uefi.system_table.boot_services orelse {
        log.err("failed to get boot service", .{});
        return .aborted;
    };

    const simple_file_system_protocol = boot_service.locateProtocol(
        uefi.protocol.SimpleFileSystem,
        null,
    ) catch null orelse {
        log.err("failed to get simple file system protocol", .{});
        return .aborted;
    };

    const root_dir = simple_file_system_protocol.openVolume() catch {
        log.err("failed to open volume", .{});
        return .aborted;
    };

    const kernel_file = root_dir.open(
        &toUtf16("kernel"),
        .read,
        .{
            .read_only = true,
            .directory = false,
        },
    ) catch {
        log.err("failed to open kernel file", .{});
        return .aborted;
    };

    const header_size = @sizeOf(elf.Elf64.Ehdr);
    const header_buffer = boot_service.allocatePool(.loader_data, header_size) catch {
        log.err("failed to allocate buffer for elf header", .{});
        return .aborted;
    };

    _ = kernel_file.read(header_buffer) catch {
        log.err("failed to read kernel file", .{});
        return .aborted;
    };

    var fbs = std.Io.Reader.fixed(header_buffer);
    const elf_header = elf.Header.read(&fbs) catch {
        log.err("failed to parse elf header, bad input", .{});
        return .aborted;
    };
    log.info("parsed kernel elf header", .{});
    log.debug(
        \\Kernel ELF information:
        \\  Entry Point         : 0x{X}
        \\  Is 64-bit           : {d}
        \\  # of Program Headers: {d}
        \\  # of Section Headers: {d}
    ,
        .{
            elf_header.entry,
            @intFromBool(elf_header.is_64),
            elf_header.phnum,
            elf_header.shnum,
        },
    );

    while (true) asm volatile ("hlt");
    return .success;
}

inline fn toUtf16(comptime s: [:0]const u8) [s.len * 2:0]u16 {
    var utf16: [s.len * 2:0]u16 = [_:0]u16{0} ** (s.len * 2);
    for (s, 0..) |c, i| {
        utf16[i] = c;
        utf16[i + 1] = 0;
    }
    return utf16;
}
