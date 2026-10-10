const std = @import("std");

pub const PUSH0: u8 = 0x00;
/// Push a single byte to to the stack.
pub const PUSH1: u8 = 0x01;
/// Push 2 bytes to the stack.
pub const PUSH2: u8 = 0x02;
/// Push 3 bytes to the stack.
pub const PUSH3: u8 = 0x03;
/// Push 4 bytes to the stack.
pub const PUSH4: u8 = 0x04;
/// Push 5 bytes to the stack.
pub const PUSH5: u8 = 0x05;
/// Push 6 bytes to the stack.
pub const PUSH6: u8 = 0x06;
/// Push 7 bytes to the stack.
pub const PUSH7: u8 = 0x07;
/// Push 8 bytes (a full word) to the stack.
pub const PUSH8: u8 = 0x08;
/// Pop the item on the top of the stack as N and duplicate the Nth stack item.
///
/// | Stack Input   | Stack Output  |
/// | ------------- | ------------- |
/// | `[..., a, N]` | `[..., a, a]` |
pub const DUP: u8 = 0x09;
/// Duplicate the item on the top of the stack.
///
/// | Stack Input | Stack Output  |
/// | ------------| ------------- |
/// | `[..., a]`  | `[..., a, a]` |
pub const DUP0: u8 = 0x0a;
/// Pop the item on the top of the stack as N and swap the first and (N-1)th stack items.
///
/// | Stack Input                  | Stack Output                 |
/// | ---------------------------- | ---------------------------- |
/// | `[..., a, (...{n-1}), b, n]` | `[..., b, (...{n-1}), a, n]` |
pub const SWAP: u8 = 0x0b;
/// Swap the top two items on the stack.
///
/// | Stack Input   | Stack Output  |
/// | ------------- | ------------- |
/// | `[..., a, b]` | `[..., b, a]` |
pub const SWAP0: u8 = 0x0c;
/// Pop the top of the stack, returning an error if the stack is empty.
///
/// | Stack Input | Stack Output |
/// | ----------- | ------------ |
/// | `[..., a]`  | `[...]`      |
pub const POP: u8 = 0x0d;
/// Pop the top of the stack anad grow the memory by that number of bytes.
///
/// | Stack Input   | Stack Output |
/// | ------------- | ------------ |
/// | `[..., size]` | `[...]`      |
pub const ALLOC: u8 = 0x20;
pub const WRITE1: u8 = 0x21;
pub const WRITE2: u8 = 0x22;
pub const WRITE3: u8 = 0x23;
pub const WRITE4: u8 = 0x24;
pub const WRITE5: u8 = 0x25;
pub const WRITE6: u8 = 0x26;
pub const WRITE7: u8 = 0x27;
/// Write the full value on the top of the stack at the memory location of the
/// 2nd top item in the stack.
pub const WRITE8: u8 = 0x28;
pub const READ1: u8 = 0x29;
pub const READ2: u8 = 0x2a;
pub const READ3: u8 = 0x2b;
pub const READ4: u8 = 0x2c;
pub const READ5: u8 = 0x2d;
pub const READ6: u8 = 0x2e;
pub const READ7: u8 = 0x2f;
/// Read the full value at the memory location specified by the top value on the
/// stack to the top of the stack.
///
/// Unwritten memory is read as zero.
pub const READ8: u8 = 0x30;
pub const DREAD1: u8 = 0x31;
pub const DREAD2: u8 = 0x32;
pub const DREAD3: u8 = 0x33;
pub const DREAD4: u8 = 0x34;
pub const DREAD5: u8 = 0x35;
pub const DREAD6: u8 = 0x36;
pub const DREAD7: u8 = 0x37;
/// Read the full value at the memory location specified by the top value on the
/// stack to the top of the stack.
///
/// Data read beyond the data length is read as zero.
pub const DREAD8: u8 = 0x38;
/// Copy a portion of data delimited by src..src+len to memory at dst.
///
/// | Stack Input            | Stack Output           |
/// | ---------------------- | ---------------------- |
/// | `[..., src, dst, len]` | `[...]`                |
pub const DCOPY: u8 = 0x39;
/// Push the length of the data to the stack.
///
/// | Stack Input | Stack Output |
/// | ----------- | ------------ |
/// | `[...]`     | `[..., len]` |
pub const DLEN: u8 = 0x3a;
/// Wrapping addition.
///
/// | Stack Input   | Stack Output   |
/// | ------------- | -------------- |
/// | `[..., b, a]` | `[..., b + a]` |
pub const ADD: u8 = 0x40;
/// Wrapping subtraction.
///
/// | Stack Input   | Stack Output   |
/// | ------------- | -------------- |
/// | `[..., b, a]` | `[..., b - a]` |
pub const SUB: u8 = 0x41;
/// Wrapping multiplication.
///
/// | Stack Input   | Stack Output   |
/// | ------------- | -------------- |
/// | `[..., b, a]` | `[..., b * a]` |
pub const MUL: u8 = 0x42;
/// Floor division.
///
/// | Stack Input   | Stack Output    |
/// | ------------- | --------------- |
/// | `[..., b, a]` | `[..., b // a]` |
///
/// This operation will return an error if the divisor is zero.
pub const DIV: u8 = 0x43;
/// Wrapping exponentiation.
///
/// | Stack Input   | Stack Output    |
/// | ------------- | --------------- |
/// | `[..., b, a]` | `[..., b ** a]` |
pub const EXP: u8 = 0x44;
/// Modulus (remainder).
///
/// | Stack Input   | Stack Output   |
/// | ------------- | -------------- |
/// | `[..., b, a]` | `[..., b % a]` |
pub const MOD: u8 = 0x45;
/// Equality.
///
/// | Stack Input   | Stack Output    |
/// | ------------- | --------------- |
/// | `[..., b, a]` | `[..., b == a]` |
pub const EQ: u8 = 0x4a;
/// Inequality.
///
/// | Stack Input   | Stack Output    |
/// | ------------- | --------------- |
/// | `[..., b, a]` | `[..., b != a]` |
pub const NEQ: u8 = 0x4b;
/// Less-than comparison.
///
/// | Stack Input   | Stack Output   |
/// | ------------- | -------------- |
/// | `[..., b, a]` | `[..., b < a]` |
pub const LT: u8 = 0x4c;
/// Greater-than comparison.
///
/// | Stack Input   | Stack Output   |
/// | ------------- | -------------- |
/// | `[..., b, a]` | `[..., b > a]` |
pub const GT: u8 = 0x4d;
/// Logical NOT.
///
/// | Stack Input | Stack Output |
/// | ----------- | ------------ |
/// | `[..., a]`  | `[..., !a]`  |
///
/// Note that this is not bitwise negation (see [`Op::NEG`]). The value is treated as a boolean, and value pushed back to the stack will only ever be 0 or 1.
pub const NOT: u8 = 0x4e;
/// Left shift.
///
/// | Stack Input   | Stack Output    |
/// | ------------- | --------------- |
/// | `[..., b, a]` | `[..., a << b]` |
///
/// The shift is "unbounded", and as such will always return 0 if the shift value is >= 64.
pub const SHL: u8 = 0x4f;
/// Right shift.
///
/// | Stack Input   | Stack Output    |
/// | ------------- | --------------- |
/// | `[..., b, a]` | `[..., a >> b]` |
///
/// The shift is "unbounded", and as such will always return 0 if the shift value is >= 64.
pub const SHR: u8 = 0x50;
/// Bitwise negation.
///
/// | Stack Input | Stack Output |
/// | ----------- | ------------ |
/// | `[..., a]`  | `[..., ~a]`  |
pub const NEG: u8 = 0x51;
/// Bitwise OR.
///
/// | Stack Input   | Stack Output   |
/// | ------------- | -------------- |
/// | `[..., b, a]` | `[..., b | a]` |
pub const OR: u8 = 0x52;
/// Bitwise XOR.
///
/// | Stack Input   | Stack Output   |
/// | ------------- | -------------- |
/// | `[..., b, a]` | `[..., b ^ a]` |
pub const XOR: u8 = 0x53;
/// Bitwise AND.
///
/// | Stack Input   | Stack Output   |
/// | ------------- | -------------- |
/// | `[..., b, a]` | `[..., b & a]` |
pub const AND: u8 = 0x54;
/// Pop an instruction pointer off the stack and jump to the address.
///
/// | Stack Input   | Stack Output |
/// | ------------- | ------------ |
/// | `[..., addr]` | `[...]`      |
pub const JUMP: u8 = 0xa0;
/// Pop the top value off of the stack and jump to the contained address
/// if the value is non-zero.
///
/// | Stack Input         | Stack Output |
/// | ------------------- | ------------ |
/// | `[..., addr, cond]` | `[...]`      |
pub const JNZ: u8 = 0xa1;
/// Pop an instruction pointer off the stack, push the current address to the stack, and then jump to the previously popped address.
///
/// | Stack Input   | Stack Output |
/// | ------------- | ------------ |
/// | `[..., addr]` | `[..., ret]` |
pub const CALL: u8 = 0xa2;
/// Terminate execution with a payload. The top two values of the stack will
/// be read as the pointer to and length of the return data.
///
/// | Stack Input       | Stack Output           |
/// | ----------------- | ---------------------- |
/// | `[..., ptr, len]` | `<program terminates>` |
pub const EXIT: u8 = 0xa4;
/// Terminate execution with an error code. The top value of the stack is
/// used as the error code.
///
/// | Stack Input   | Stack Output           |
/// | ------------- | ---------------------- |
/// | `[..., code]` | `<program terminates>` |
pub const TRAP: u8 = 0xa5;

pub inline fn add(a: u64, b: u64) u64 {
    return a +% b;
}

pub inline fn sub(a: u64, b: u64) u64 {
    return a -% b;
}

pub inline fn mul(a: u64, b: u64) u64 {
    return a *% b;
}

pub inline fn div(a: u64, b: u64) !u64 {
    if (b == 0) {
        @branchHint(.cold);
        return error.DivideByZero;
    } else {
        return a / b;
    }
}

pub inline fn not(a: u64) u64 {
    return @intFromBool(a == 0);
}

pub inline fn gt(a: u64, b: u64) u64 {
    return @intFromBool(a > b);
}

pub inline fn lt(a: u64, b: u64) u64 {
    return @intFromBool(a < b);
}

pub inline fn neq(a: u64, b: u64) u64 {
    return @intFromBool(a != b);
}

pub inline fn eq(a: u64, b: u64) u64 {
    return @intFromBool(a == b);
}

pub inline fn mod(a: u64, b: u64) !u64 {
    if (b == 0) {
        @branchHint(.cold);
        return error.DivideByZero;
    } else {
        return a % b;
    }
}

pub inline fn and_(a: u64, b: u64) u64 {
    return a & b;
}

pub inline fn xor(a: u64, b: u64) u64 {
    return a ^ b;
}

pub inline fn or_(a: u64, b: u64) u64 {
    return a | b;
}

pub inline fn neg(a: u64) u64 {
    return ~a;
}

pub inline fn expmod(a: u64, b: u64) u64 {
    if (b == 0) {
        return 1;
    }

    var acc: u128 = 1;
    var base = @as(u128, a);
    var exp = b;

    while (true) {
        if ((exp & 1) == 1) {
            acc = (acc * base) % std.math.maxInt(u64);
            // since exp!=0, finally the exp must be 1.
            if (exp == 1) {
                return @truncate(acc);
            }
        }
        exp >>= 1;
        base = (base * base) % std.math.maxInt(u64);
    }
}

pub inline fn shr(a: u64, shift: u64) u64 {
    return std.math.shr(u64, a, shift);
}

pub inline fn shl(a: u64, shift: u64) u64 {
    return std.math.shl(u64, a, shift);
}
