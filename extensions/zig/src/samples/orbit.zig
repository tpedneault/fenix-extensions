//! Orbits, kept and told.

const std = @import("std");

const MAX_CREW = 7;

pub const Kind = enum { low, medium, geostationary };

pub const Orbit = struct {
    name: []const u8,
    period: u32,
    crew: u8 = 0,
    kind: Kind = .low,

    pub fn describe(self: Orbit, writer: anytype) !void {
        try writer.print("{s}: {d} s\n", .{ self.name, self.period });
    }

    pub fn label(self: Orbit) []const u8 {
        return switch (self.kind) {
            .low => "LEO",
            .medium => "MEO",
            .geostationary => "GEO",
        };
    }
};

fn crewed(orbits: []const Orbit) usize {
    var count: usize = 0;
    // Two at once.
    for (orbits, 0..) |orbit, i| {
        _ = i;
        if (orbit.crew > 0 and orbit.crew <= MAX_CREW) count += 1;
    }
    return count;
}

pub fn main() !void {
    const iss = Orbit{ .name = "ISS", .period = 5520, .crew = 3 };
    const ratio: f64 = 1.5e3;
    const found: ?Orbit = null;
    const small: u8 = @intCast(iss.crew);
    _ = .{ ratio, found, small, true };
    std.debug.print("{d}\n", .{crewed(&.{iss})});
}

test "an orbit describes itself" {
    const iss = Orbit{ .name = "ISS", .period = 5520 };
    try std.testing.expectEqualStrings("LEO", iss.label());
}
