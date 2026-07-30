const std = @import("std");
const uefi = std.os.uefi;

var con_out: *uefi.protocol.SimpleTextOutput = undefined;

pub fn init(console_output: *uefi.protocol.SimpleTextOutput) void {
    con_out = console_output;
}

fn writer() std.Io.Writer {
    return .{
        .buffer = &.{},
        .vtable = &.{ .drain = drain },
    };
}

fn drain(_: *std.Io.Writer, data: []const []const u8, splat: usize) std.Io.Writer.Error!usize {
    var written: usize = 0;
    for (data[0 .. data.len - 1]) |bytes| {
        outputBytes(bytes);
        written += bytes.len;
    }
    const pattern = data[data.len - 1];
    for (0..splat) |_| {
        outputBytes(pattern);
        written += pattern.len;
    }
    return written;
}

fn outputBytes(bytes: []const u8) void {
    for (bytes) |b| {
        _ = con_out.outputString(&[_:0]u16{b}) catch unreachable;
    }
}

pub fn log(
    comptime message_level: std.log.Level,
    comptime scope: @TypeOf(.EnumLiteral),
    comptime format: []const u8,
    args: anytype,
) void {
    const level_str = switch (message_level) {
        .debug => "[DEBUG]",
        .err => "[ERROR]",
        .info => "[INFO ]",
        .warn => "[WARN ]",
    };
    const scope_str = if (scope == .default) ": " else "(" ++ @tagName(scope) ++ "): ";

    var w = writer();
    w.print(level_str ++ " " ++ scope_str ++ format ++ "\r\n", args) catch unreachable;
}
