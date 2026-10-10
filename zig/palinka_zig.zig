const std = @import("std");
const Op = @import("Op.zig");

const Io = std.Io;

const zig = @import("zig");

// pub const panic = std.debug.no_panic;

// pub fn panic(_: []const u8, _: ?*std.builtin.StackTrace, _: ?usize) noreturn {
//     @trap();
// }

pub export fn zig_allocator() *anyopaque {
    return @constCast(&std.heap.brk_allocator);
}

pub const Vm = struct {
    gpa: std.mem.Allocator,
    code: [*]u8,
    code_len: usize,
    data: [*]const u8,
    data_len: usize,
    stack: std.ArrayList(u64),
    memory: std.ArrayList(u8),
    pc: usize,
    cycles: u64,
    max_memory: usize,

    inline fn getMut(self: *Vm, n: usize, comptime err: Error) Error!*u64 {
        @setRuntimeSafety(false);

        if (self.stack.items.len <= n) {
            @branchHint(.cold);
            return err;
        } else {
            return &self.stack.items.ptr[(self.stack.items.len - 1) - n];
        }
    }

    inline fn get(self: *Vm, n: usize) Error!u64 {
        @setRuntimeSafety(false);

        if (self.stack.items.len <= n) {
            @branchHint(.cold);
            return Error.StackEmpty;
        } else {
            return self.stack.items.ptr[(self.stack.items.len - 1) - n];
        }
    }

    inline fn pop(self: *Vm) Error!u64 {
        @setRuntimeSafety(false);

        if (self.stack.items.len == 0) {
            @branchHint(.cold);
            return Error.StackEmpty;
        } else {
            self.stack.items.len -= 1;
            return self.stack.items.ptr[self.stack.items.len];
        }
    }

    inline fn push(self: *Vm, value: u64) !void {
        @setRuntimeSafety(false);

        if (self.stack.items.len == self.stack.capacity) {
            @branchHint(.unlikely);
            try self.stack.ensureUnusedCapacity(self.gpa, 1);
        }
        self.stack.items.len += 1;
        self.stack.items.ptr[self.stack.items.len - 1] = value;
    }

    inline fn write_n(self: *Vm, comptime n: usize) Error!void {
        @setRuntimeSafety(false);

        // if (self.stack.items.len < 2) {
        //     @branchHint(.cold);
        //     return Error.StackEmpty;
        // }
        // const value = self.stack.items.ptr[self.stack.items.len - 1];
        // const ptr = try asPtr(self.stack.items.ptr[self.stack.items.len - 2]);
        // self.stack.items.len -= 2;

        const value = try self.pop();
        const ptr = try asPtr(try self.pop());
        try checkBounds(self.memory.items.len, try tryAdd(ptr, n, Error.InvalidStackValue));
        var bytes: [n]u8 = undefined;
        std.mem.writeInt(@Int(.unsigned, n * 8), &bytes, @truncate(value), .big);
        @memcpy(self.memory.items.ptr[ptr..][0..n], &bytes);
    }

    inline fn read_n(self: *Vm, comptime n: usize) Error!void {
        @setRuntimeSafety(false);

        const top: *u64 = try self.getMut(0, Error.StackEmpty);
        const ptr = try asPtr(top.*);
        try checkBounds(self.memory.items.len, try tryAdd(ptr, n, Error.InvalidStackValue));
        const res = self.memory.items.ptr[ptr..][0..n];
        top.* = u64_from_bytes(n, res.*);
    }

    inline fn dread_n(self: *Vm, comptime n: usize) Error!void {
        @setRuntimeSafety(false);

        const top = try self.getMut(0, Error.StackEmpty);
        const ptr = try asPtr(top.*);
        try checkBounds(self.data_len, try tryAdd(ptr, n, Error.InvalidStackValue));
        const res = self.data[ptr..][0..n];
        top.* = u64_from_bytes(n, res.*);
    }

    inline fn push_n(self: *Vm, comptime n: usize) Error!void {
        @setRuntimeSafety(false);

        if (self.pc + n > self.code_len) {
            @branchHint(.cold);
            // incomplete cycle
            self.cycles -= 1;
            return Error.Eof;
        }
        try self.push(u64_from_bytes(n, self.code[self.pc..][0..n].*));
        self.pc += n;
    }

    inline fn binop(self: *Vm, comptime op: fn (u64, u64) callconv(.@"inline") u64) Error!void {
        @setRuntimeSafety(false);

        const len = self.stack.items.len;

        if (len < 2) {
            @branchHint(.cold);
            return Error.StackEmpty;
        }

        const lhs = self.stack.items.ptr[len - 2];
        const rhs = self.stack.items.ptr[len - 1];

        self.stack.items.len -= 1;
        self.stack.items.ptr[len - 2] = op(lhs, rhs);
    }

    pub fn step(self: *Vm) Error!StepResult {
        @setRuntimeSafety(false);

        if (self.pc >= self.code_len) {
            @branchHint(.cold);
            return .done;
        }

        self.cycles += 1;

        const op = self.code[self.pc];

        self.pc += 1;

        switch (op) {
            Op.PUSH0 => try self.push(0),
            Op.PUSH1 => try self.push_n(1),
            Op.PUSH2 => try self.push_n(2),
            Op.PUSH3 => try self.push_n(3),
            Op.PUSH4 => try self.push_n(4),
            Op.PUSH5 => try self.push_n(5),
            Op.PUSH6 => try self.push_n(6),
            Op.PUSH7 => try self.push_n(7),
            Op.PUSH8 => try self.push_n(8),
            Op.DUP => {
                const idx = try self.getMut(0, Error.StackEmpty);
                const stack_idx = try tryAdd(try asPtr(idx.*), 1, Error.InvalidStackIdx);

                idx.* = (try self.getMut(stack_idx, Error.InvalidStackIdx)).*;
            },
            Op.DUP0 => try self.push(try self.get(0)),
            Op.SWAP => {
                const idx = try tryAdd(try asPtr(try self.pop()), 2, Error.InvalidStackValue);
                const len = self.stack.items.len;
                if (len < idx) {
                    @branchHint(.cold);
                    return Error.InvalidStackIdx;
                }
                const a_idx = len - 1;
                const b_idx = len - idx;
                std.mem.swap(u64, &self.stack.items.ptr[a_idx], &self.stack.items.ptr[b_idx]);
            },
            Op.SWAP0 => {
                if (self.stack.items.len < 2) {
                    @branchHint(.cold);
                    return Error.StackEmpty;
                }
                std.mem.swap(u64, &self.stack.items.ptr[self.stack.items.len - 2], &self.stack.items.ptr[self.stack.items.len - 1]);
            },
            Op.POP => {
                if (self.stack.items.len == 0) {
                    @branchHint(.cold);
                    return Error.StackEmpty;
                } else {
                    self.stack.items.len -= 1;
                }
            },
            Op.ALLOC => {
                const size = try self.pop();
                const new_size = try tryAdd(size, self.memory.items.len, Error.InvalidStackValue);
                if (new_size > self.max_memory) {
                    return Error.OutOfMemory;
                }
                try self.memory.appendNTimes(self.gpa, 0, size);
            },

            Op.WRITE1 => try self.write_n(1),
            Op.WRITE2 => try self.write_n(2),
            Op.WRITE3 => try self.write_n(3),
            Op.WRITE4 => try self.write_n(4),
            Op.WRITE5 => try self.write_n(5),
            Op.WRITE6 => try self.write_n(6),
            Op.WRITE7 => try self.write_n(7),
            Op.WRITE8 => try self.write_n(8),

            Op.READ1 => try self.read_n(1),
            Op.READ2 => try self.read_n(2),
            Op.READ3 => try self.read_n(3),
            Op.READ4 => try self.read_n(4),
            Op.READ5 => try self.read_n(5),
            Op.READ6 => try self.read_n(6),
            Op.READ7 => try self.read_n(7),
            Op.READ8 => try self.read_n(8),

            Op.DREAD1 => try self.dread_n(1),
            Op.DREAD2 => try self.dread_n(2),
            Op.DREAD3 => try self.dread_n(3),
            Op.DREAD4 => try self.dread_n(4),
            Op.DREAD5 => try self.dread_n(5),
            Op.DREAD6 => try self.dread_n(6),
            Op.DREAD7 => try self.dread_n(7),
            Op.DREAD8 => try self.dread_n(8),

            Op.DCOPY => {
                if (self.stack.items.len < 3) {
                    @branchHint(.cold);
                    return Error.StackEmpty;
                }

                const len = try asPtr(self.stack.items.ptr[self.stack.items.len - 1]);
                const dst = try asPtr(self.stack.items.ptr[self.stack.items.len - 2]);
                const src = try asPtr(self.stack.items.ptr[self.stack.items.len - 3]);

                const data_end = try tryAdd(src, len, Error.InvalidStackValue);
                const memory_end = try tryAdd(dst, len, Error.InvalidStackValue);

                try checkBounds(self.data_len, data_end);
                try checkBounds(self.memory.items.len, memory_end);

                self.stack.items.len -= 3;

                @memcpy(self.memory.items.ptr[dst..][0..len], self.data[src..][0..len]);
            },

            Op.DLEN => try self.push(@intCast(self.data_len)),

            Op.ADD => try self.binop(Op.add),
            Op.SUB => try self.binop(Op.sub),
            Op.MUL => try self.binop(Op.mul),
            Op.DIV => {
                const len = self.stack.items.len;

                if (len < 2) {
                    @branchHint(.cold);
                    return Error.StackEmpty;
                }

                const a = self.pop() catch {
                    unreachable;
                };

                self.stack.items.ptr[len - 2] = try Op.div(self.stack.items.ptr[len - 2], a);
            },
            Op.EXP => try self.binop(Op.expmod),
            Op.MOD => {
                const len = self.stack.items.len;

                if (len < 2) {
                    @branchHint(.cold);
                    return Error.StackEmpty;
                }

                const a = self.pop() catch {
                    unreachable;
                };

                self.stack.items.ptr[len - 2] = try Op.mod(self.stack.items.ptr[len - 2], a);
            },
            Op.EQ => try self.binop(Op.eq),
            Op.NEQ => try self.binop(Op.neq),
            Op.LT => try self.binop(Op.lt),
            Op.GT => try self.binop(Op.gt),
            Op.NOT => {
                const a = try self.getMut(0, Error.StackEmpty);
                a.* = Op.not(a.*);
            },
            Op.SHR => try self.binop(Op.shr),
            Op.SHL => try self.binop(Op.shl),
            Op.NEG => {
                const a = try self.getMut(0, Error.StackEmpty);
                a.* = Op.neg(a.*);
            },
            Op.OR => try self.binop(Op.or_),
            Op.XOR => try self.binop(Op.xor),
            Op.AND => try self.binop(Op.and_),

            Op.JUMP => {
                const dst = try self.pop();
                self.pc = try asPtr(dst);
            },
            Op.JNZ => {
                if (self.stack.items.len < 2) {
                    @branchHint(.cold);
                    return Error.StackEmpty;
                }

                const dst = self.stack.items.ptr[self.stack.items.len - 1];
                const value = self.stack.items.ptr[self.stack.items.len - 2];

                self.stack.items.len -= 2;

                if (value != 0) {
                    @branchHint(.unpredictable);
                    self.pc = try asPtr(dst);
                }
            },
            Op.CALL => {
                const top = try self.getMut(0, Error.StackEmpty);
                const address = try asPtr(top.*);
                top.* = @intCast(self.pc);
                self.pc = address;
            },
            Op.EXIT => {
                @branchHint(.unlikely);
                const len = try self.pop();
                const ptr = try self.pop();

                try checkBounds(self.memory.items.len, try tryAdd(ptr, len, Error.InvalidStackValue));

                return StepResult{ .exit = self.memory.items.ptr[ptr..][0..len] };
            },
            Op.TRAP => {
                @branchHint(.unlikely);
                const value = try self.pop();
                return StepResult{ .trap = value };
            },
            else => {
                @branchHint(.cold);
                // incomplete cycle
                self.cycles -= 1;
                return Error.UnknownOp;
            },
        }

        return .stepped;
    }
};

export fn zig_cycles(self: *Vm) u64 {
    return self.cycles;
}

export fn zig_run(self: *Vm) RunResult {
    while (true) {
        @branchHint(.likely);

        const res = self.step() catch |e| {
            return .{
                .cycles = self.cycles,
                .tag = .err,
                .data = .{
                    .err = switch (e) {
                        Error.OutOfMemory => .OutOfMemory,

                        Error.StackEmpty => .StackEmpty,
                        Error.InvalidStackIdx => .InvalidStackIdx,
                        Error.Segfault => .Segfault,
                        Error.Eof => .Eof,
                        Error.DivideByZero => .DivideByZero,
                        Error.InvalidStackValue => .InvalidStackValue,
                        Error.UnknownOp => .UnknownOp,
                        Error.PointerTooBig => .PointerTooBig,
                    },
                },
            };
        };

        switch (res) {
            .stepped => {
                @branchHint(.likely);
            },
            .done => {
                @branchHint(.cold);
                return .{
                    .cycles = self.cycles,
                    .tag = .done,
                    .data = undefined,
                };
            },
            .trap => |code| {
                @branchHint(.cold);
                return .{
                    .cycles = self.cycles,
                    .tag = .trap,
                    .data = .{
                        .trap = code,
                    },
                };
            },
            .exit => |bz| {
                @branchHint(.cold);
                return .{
                    .cycles = self.cycles,
                    .tag = .exit,
                    .data = .{
                        .exit = .{
                            .ptr = bz.ptr,
                            .len = bz.len,
                        },
                    },
                };
            },
        }
    }

    return .{
        .cycles = self.cycles,
        .tag = .done,
        .data = undefined,
    };
}

export fn zig_init(gpa_any: *anyopaque, code: [*]u8, code_len: usize, data: [*]const u8, data_len: usize, max_memory: usize) *allowzero Vm {
    const gpa = @as(*std.mem.Allocator, @ptrCast(@alignCast(@constCast(gpa_any))));
    const vm = gpa.create(Vm) catch {
        return @ptrFromInt(0);
    };

    vm.gpa = gpa.*;
    // .gpa = @constCast(&std.heap.brk_allocator);
    // vm.gpa = std.heap.brk_allocator;
    vm.code = code;
    vm.code_len = code_len;
    vm.data = data;
    vm.data_len = data_len;
    vm.stack = .empty;
    vm.memory = .empty;
    vm.pc = 0;
    vm.cycles = 0;
    vm.max_memory = max_memory;

    return vm;
}

export fn zig_drop(vm: *Vm) void {
    vm.stack.deinit(vm.gpa);
    vm.memory.deinit(vm.gpa);
    vm.gpa.destroy(vm);
}

inline fn u64_from_bytes(comptime n: usize, arr: [n]u8) u64 {
    return @intCast(std.mem.readInt(@Int(.unsigned, n * 8), &arr, .big));
}

const StepResultTag = enum {
    stepped,
    done,
    trap,
    exit,
};
const StepResult = union(StepResultTag) {
    stepped: void,
    done: void,
    trap: u64,
    exit: []const u8,
};

const RunResultTag = enum(u8) {
    done = 0,
    trap = 1,
    exit = 2,
    err = 3,
};
const RunResultUnion = extern union {
    trap: u64,
    exit: ExitData,
    err: Err,
};

const Err = enum(u8) {
    OutOfMemory,

    StackEmpty,
    InvalidStackIdx,
    Segfault,
    Eof,
    DivideByZero,
    InvalidStackValue,
    UnknownOp,
    PointerTooBig,
};

const ExitData = extern struct {
    ptr: [*]const u8,
    len: usize,
};

const RunResult = extern struct {
    cycles: u64,
    tag: RunResultTag,
    data: RunResultUnion,
};

inline fn asPtr(val: u64) !usize {
    @setRuntimeSafety(false);

    if (val > std.math.maxInt(usize)) {
        @branchHint(.cold);
        return Error.InvalidStackValue;
    } else {
        return @intCast(val);
    }
}

inline fn tryAdd(val: usize, n: usize, comptime err: Error) Error!usize {
    @setRuntimeSafety(false);

    const res, const overflow = @addWithOverflow(val, n);

    if (overflow != 0) {
        @branchHint(.cold);
        return err;
    }

    return res;
}

inline fn trySub(val: usize, n: usize) ?usize {
    const res, const overflow = @subWithOverflow(val, n);

    if (overflow != 0) {
        @branchHint(.cold);
        return null;
    }

    return res;
}

inline fn checkBounds(len: usize, idx: usize) !void {
    @setRuntimeSafety(false);

    if (len < idx) {
        @branchHint(.cold);
        return Error.Segfault;
    }
}

const Error = error{
    OutOfMemory,

    StackEmpty,
    InvalidStackIdx,
    Segfault,
    Eof,
    DivideByZero,
    InvalidStackValue,
    UnknownOp,
    PointerTooBig,
};
