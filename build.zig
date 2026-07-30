const std = @import("std");

pub fn build(b: *std.Build) void {
    b.install_path = "dist";

    const optimize = b.standardOptimizeOption(.{});

    const bootloader = b.addExecutable(.{
        .name = "bootloader",
        .root_module = b.createModule(.{
            .root_source_file = b.path("bootloader/main.zig"),
            .target = b.resolveTargetQuery(.{
                .cpu_arch = .x86_64,
                .os_tag = .uefi,
            }),
            .optimize = optimize,
        }),
        .linkage = .static,
    });

    const esp_dir = "esp";
    const install_bootloader = b.addInstallFile(
        bootloader.getEmittedBin(),
        b.fmt("{s}/efi/boot/BOOTX64.EFI", .{esp_dir}),
    );
    b.getInstallStep().dependOn(&install_bootloader.step);

    const qemu_args = [_][]const u8{
        "qemu-system-x86_64",
        "-m",
        "512M",
        "-bios",
        "/usr/share/ovmf/x64/OVMF.4m.fd",
        "-drive",
        b.fmt("file=fat:rw:{s}/{s}", .{ b.install_path, esp_dir }),
        "-nographic",
        "-serial",
        "mon:stdio",
        "-no-reboot",
        "-enable-kvm",
        "-cpu",
        "host",
    };
    const qemu_cmd = b.addSystemCommand(&qemu_args);
    qemu_cmd.step.dependOn(b.getInstallStep());

    const run_qemu_cmd = b.step("run", "Run project with qemu");
    run_qemu_cmd.dependOn(&qemu_cmd.step);
}
