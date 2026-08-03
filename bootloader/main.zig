const std = @import("std");
const uefi = std.os.uefi;
const log = std.log.scoped(.bootloader);
const elf = std.elf;

const blog = @import("logger.zig");
const arch = @import("lib.zig").arch;

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

    const kernel_executable_path: [*:0]const u16 = std.unicode.utf8ToUtf16LeStringLiteral("kernel");
    const kernel_file = root_dir.open(
        kernel_executable_path,
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

    arch.page.setLv4Writable(boot_service) catch |err| {
        log.err("Failed to set lv4 page writable: {any}", .{err});
        return .aborted;
    };
    arch.page.map4kTo(0xFFFF_FFFF_DEAD_0000, 0x10_0000, .read_write, boot_service) catch |err| {
        log.err("Failed to map 4kib page: {any}", .{err});
        return .aborted;
    };

    while (true) asm volatile ("hlt");
    return .success;
}
