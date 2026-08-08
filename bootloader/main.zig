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
    log.info("into bootloader, logger working", .{});

    const boot_service = uefi.system_table.boot_services orelse {
        log.err("failed to get boot service", .{});
        return .aborted;
    };

    arch.page.setLv4Writable(boot_service) catch |err| {
        log.err("Failed to set lv4 page writable: {any}", .{err});
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

    const kernel_file_info_size = kernel_file.getInfoSize(.file) catch {
        log.err("failed to get size required for file info", .{});
        return .aborted;
    };
    const kernel_file_info_buffer = boot_service.allocatePool(.loader_data, kernel_file_info_size) catch {
        log.err("failed to allocate memory for file info", .{});
        return .aborted;
    };
    const kernel_file_info = kernel_file.getInfo(.file, kernel_file_info_buffer) catch {
        log.err("failed to read kernel file info", .{});
        return .aborted;
    };

    log.debug("kernel file size: {}", .{kernel_file_info.file_size});

    const kernel_buffer = boot_service.allocatePool(.loader_data, kernel_file_info.file_size) catch {
        log.err("failed to allocate memory for kernel buffer", .{});
        return .aborted;
    };
    _ = kernel_file.read(kernel_buffer) catch {
        log.err("failed to read kernel", .{});
        return .aborted;
    };

    var fbs = std.Io.Reader.fixed(kernel_buffer);
    const elf_header = elf.Header.read(&fbs) catch {
        log.err("failed to take elf header out of kernel", .{});
        return .aborted;
    };

    const Addr = elf.Elf64.Addr;
    var kernel_start_virt: Addr = std.math.maxInt(Addr);
    var kernel_start_phys: Addr align(4096) = std.math.maxInt(Addr);
    var kernel_end_phys: Addr = 0;

    var iter = elf_header.iterateProgramHeadersBuffer(kernel_buffer);

    while (true) {
        const phdr = iter.next() catch |err| {
            log.err("failed to read program header: {}", .{err});
            return .aborted;
        } orelse break;
        if (phdr.p_type != @intFromEnum(elf.PT.LOAD)) continue;
        if (phdr.p_paddr < kernel_start_phys) kernel_start_phys = phdr.p_paddr;
        if (phdr.p_vaddr < kernel_start_virt) kernel_start_virt = phdr.p_vaddr;
        if (phdr.p_paddr + phdr.p_memsz > kernel_end_phys) kernel_end_phys = phdr.p_paddr + phdr.p_memsz;
    }

    const pages_4kib = (kernel_end_phys - kernel_start_phys + 4095) / 4096;
    log.info("Kernel image: 0x{X:0>16} - 0x{X:0>16} (0x{X} pages)", .{ kernel_start_phys, kernel_end_phys, pages_4kib });

    _ = boot_service.allocatePages(
        .{
            .address = @ptrFromInt(kernel_start_phys),
        },
        .loader_data,
        pages_4kib,
    ) catch |err| {
        log.err("failed to allocate memory for kernel: {}", .{err});
        return .aborted;
    };

    for (0..pages_4kib) |i| {
        arch.page.map4kTo(
            kernel_start_virt + 4096 * i,
            kernel_start_phys + 4096 * i,
            .read_write,
            boot_service,
        ) catch {
            log.err("failed to map page for kernel", .{});
            return .aborted;
        };
    }
    log.info("mapped memory for kernel image.", .{});

    log.info("loading kernel", .{});

    iter = elf_header.iterateProgramHeadersBuffer(kernel_buffer);
    while (true) {
        const phdr = iter.next() catch |err| {
            log.err("failed to read program header: {}", .{err});
            return .aborted;
        } orelse break;
        if (phdr.p_type != @intFromEnum(elf.PT.LOAD)) continue;

        const segment: [*]u8 = @ptrFromInt(phdr.p_vaddr);
        @memcpy(segment, kernel_buffer[phdr.p_offset .. phdr.p_offset + phdr.p_memsz]);

        log.info("  Segment @ 0x{X:0>16} - 0x{X:0>16}", .{ phdr.p_vaddr, phdr.p_vaddr + phdr.p_memsz });

        const zero_count = phdr.p_memsz - phdr.p_filesz;
        if (zero_count > 0) {
            boot_service._setMem(@ptrFromInt(phdr.p_vaddr + phdr.p_filesz), zero_count, 0);
        }
    }

    while (true) asm volatile ("hlt");
    return .success;
}
