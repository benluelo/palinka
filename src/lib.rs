#![feature(split_array)]
// #![warn(clippy::panic, clippy::unwrap_in_result)]
#![feature(generic_const_items, explicit_tail_calls)]
#![allow(incomplete_features)]

use std::{
    error::Error as StdError,
    fmt::{self, Debug},
};

use anyhow::Result;
use const_hex::ToHexExt;
use tracing::trace;

pub mod assembler;
pub mod cefre;

pub mod ffi;
pub mod tail;
pub mod zig;

#[cfg(test)]
mod vm_tests;

pub trait VmT {
    fn new(code: Vec<u8>, data: Vec<u8>, max_memory: usize) -> Self;

    fn run(&mut self) -> VmRunResult;
}

#[derive(Debug, Clone, PartialEq)]
pub enum VmRunResult {
    Done,
    Trap(u64),
    Exit(Vec<u8>),

    StackEmpty,
    InvalidStackIdx,
    Segfault,
    Eof,
    DivideByZero,
    InvalidStackValue,
    UnknownOp,
    OutOfMemory,
    PointerTooBig,
}

impl VmRunResult {
    pub fn try_into_exit(self) -> Result<Vec<u8>, Self> {
        if let Self::Exit(v) = self { Ok(v) } else { Err(self) }
    }
}

pub trait CycleCountVm: VmT {
    fn cycles(&mut self) -> u64;
}

pub struct Vm<H: Hook = ()> {
    pub code: Vec<u8>,
    pub data: Vec<u8>,
    pub stack: Vec<u64>,
    pub memory: Vec<u8>,
    pub hook: H,
    pub pc: usize,
    pub max_memory: usize,
}

impl<H: Hook<Error = !> + Default> VmT for Vm<H> {
    fn new(code: Vec<u8>, data: Vec<u8>, max_memory: usize) -> Self {
        Self::new_with(code, data, max_memory, H::default())
    }

    fn run(&mut self) -> VmRunResult {
        match Vm::run_raw(self) {
            Ok(Some(exit)) => VmRunResult::Exit(exit),
            Ok(None) => VmRunResult::Done,
            Err(Error::OutOfMemory) => VmRunResult::OutOfMemory,
            Err(Error::StackEmpty) => VmRunResult::StackEmpty,
            Err(Error::InvalidStackIdx) => VmRunResult::InvalidStackIdx,
            Err(Error::Segfault) => VmRunResult::Segfault,
            Err(Error::Eof) => VmRunResult::Eof,
            Err(Error::DivideByZero) => VmRunResult::DivideByZero,
            Err(Error::InvalidStackValue) => VmRunResult::InvalidStackValue,
            Err(Error::Trap(code)) => VmRunResult::Trap(code),
            Err(Error::UnknownOp(_)) => VmRunResult::UnknownOp,
        }
    }
}

impl CycleCountVm for Vm<CycleCountHook> {
    fn cycles(&mut self) -> u64 {
        self.hook.cycles()
    }
}

impl<H: Hook + Debug> fmt::Debug for Vm<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vm")
            .field("code", &self.code.encode_hex())
            .field("data", &self.data.encode_hex())
            .field("stack", &self.stack)
            .field("memory", &self.memory.encode_hex())
            .field("hook", &self.hook)
            .field("pc", &self.pc)
            .finish()
    }
}

macro_rules! ok_or {
    ($e:expr, $err:expr) => {
        match $e {
            Some(t) => t,
            None => return Err($err),
        }
    };
}

macro_rules! as_ptr {
    ($e:expr) => {{
        #[cfg(target_pointer_width = "64")]
        { $e as usize }
        #[cfg(not(target_pointer_width = "64"))]
        {
            let n = $e;
            if n > (usize::MAX as u64) {
                return Error::PointerTooBig(n);
            } else {
                $e as u64
            }
        }
    }};
}

impl Vm {
    pub fn new(code: Vec<u8>, data: Vec<u8>, max_memory: usize) -> Self {
        Self::new_with(code, data, max_memory, ())
    }
}

impl<H: Hook> Vm<H> {
    pub fn new_with(code: Vec<u8>, data: Vec<u8>, max_memory: usize, hook: H) -> Self {
        Self { code, data, stack: vec![], memory: vec![], hook, pc: 0, max_memory }
    }

    pub fn run_raw(&mut self) -> Result<Option<Vec<u8>>, Error<H>> {
        trace!("data: {}", self.data.encode_hex());

        loop {
            match self.step() {
                Ok(StepResult::Stepped) => {}
                Ok(StepResult::Eof) => break Ok(None),
                Ok(StepResult::Exit(output)) => break Ok(Some(output)),
                Err(err) => {
                    // info!("pc: {pc}");
                    return Err(err);
                }
            }

            // std::thread::sleep(std::time::Duration::from_millis(1000));
        }
    }

    fn push_stack(&mut self, value: u64) -> bool {
        match self.stack.try_reserve(1) {
            Ok(()) => {
                self.stack.push(value);
                true
            }
            Err(_) => false,
        }
    }

    #[warn(clippy::question_mark_used)]
    // #[inline(always)]
    pub fn step(&mut self) -> Result<StepResult, Error<H>> {
        if let Err(err) = self.hook.pre_cycle() {
            return Err(Error::Hook(err));
        };

        #[inline(always)]
        fn u64_from_bytes(arr: &[u8]) -> u64 {
            let mut v = [0; 8];
            v[8 - arr.len()..].copy_from_slice(arr);
            u64::from_be_bytes(v)
        }

        macro_rules! hook {
            ($($op:tt)+) => {
                match self.hook.cycle(self.pc, Op::$($op)+, &self.stack, &self.memory) {
                    Ok(ok) => ok,
                    Err(err) => return Err(Error::Hook(err)),
                }
            };
        }

        macro_rules! pop {
            () => {{
                let len = self.stack.len();
                if len == 0 {
                    return Err(Error::<H>::StackEmpty);
                } else {
                    unsafe {
                        let x = *self.stack.get_unchecked(len - 1);
                        self.stack.set_len(len - 1);
                        x
                    }
                }
            }};
        }

        macro_rules! try_add {
            ($a:expr, $b:expr) => {
                ok_or!(($a).checked_add($b), Error::<H>::InvalidStackValue)
            };
        }

        macro_rules! last {
            () => {
                ok_or!(self.stack.last_mut(), Error::<H>::StackEmpty)
            };
        }

        macro_rules! push_n {
            ($op:ident, $n:literal) => {{
                self.pc += $n;
                let mut v = [0_u8; 8];
                let res = ok_or!(self.code.get(self.pc - $n..self.pc), Error::<H>::Eof);
                v[8 - $n..].copy_from_slice(res);
                let n = u64::from_be_bytes(v);
                trace!("push{} {n:x}", $n);
                hook!($op(*v.rsplit_array_ref::<$n>().1));
                if !self.push_stack(n) {
                    return Err(Error::<H>::OutOfMemory);
                }
            }};
        }

        macro_rules! write_n {
            ($op:ident, $n:literal) => {{
                trace!("write{}", $n);
                hook!($op);
                let len = self.stack.len();
                if len < 2 {
                    return Err(Error::<H>::StackEmpty);
                };
                unsafe {
                    let value_idx = len.unchecked_sub(1);
                    let ptr_idx = len.unchecked_sub(2);
                    let value = *self.stack.get_unchecked(value_idx);
                    let ptr = as_ptr!((*self.stack.get_unchecked(ptr_idx)));
                    self.stack.set_len(ptr_idx);
                    trace!("{value:x} @ {ptr:x}");
                    let bytes = value.to_be_bytes();
                    ok_or!(self.memory.get_mut(ptr..try_add!(ptr, $n)), Error::<H>::Segfault)
                        .copy_from_slice(&bytes[8 - $n..]);
                }
            }};
        }

        macro_rules! read_n {
            ($op:ident, $n:literal) => {{
                trace!("read{}", $n);
                hook!($op);
                let top = last!();
                let ptr = as_ptr!(*top);
                trace!("ptr: {ptr:x}");
                let res = ok_or!(self.memory.get(ptr..try_add!(ptr, $n)), Error::<H>::Segfault);
                *top = u64_from_bytes(res);
            }};
        }

        macro_rules! dread_n {
            ($op:ident, $n:literal) => {{
                // trace!("dread{}", $n);
                hook!($op);
                let top = last!();
                let ptr = as_ptr!(*top);
                trace!("ptr: {ptr:x}");
                let res = ok_or!(self.data.get(ptr..try_add!(ptr, $n)), Error::<H>::Segfault);
                *top = u64_from_bytes(res);
            }};
        }

        macro_rules! binop {
            ($op:ident, $op_name:literal, $f:ident) => {{
                trace!($op_name);
                hook!($op);
                let len = self.stack.len();

                if len < 2 {
                    return Err(Error::<H>::StackEmpty);
                };

                let b = unsafe { len.unchecked_sub(2) };
                let a = unsafe { len.unchecked_sub(1) };

                unsafe {
                    *self.stack.get_unchecked_mut(b) =
                        op::$f(*self.stack.get_unchecked(b), *self.stack.get_unchecked(a))
                };

                unsafe { self.stack.set_len(a) };
            }};
        }

        if self.pc >= self.code.len() {
            return Ok(StepResult::Eof);
        }
        let op = unsafe { self.code.get_unchecked(self.pc) };

        self.pc += 1;

        trace!("");

        match *op {
            raw::PUSH0 => {
                hook!(PUSH0);
                trace!("push0");
                self.push_stack(0);
            }
            raw::PUSH1 => push_n!(PUSH1, 1),
            raw::PUSH2 => push_n!(PUSH2, 2),
            raw::PUSH3 => push_n!(PUSH3, 3),
            raw::PUSH4 => push_n!(PUSH4, 4),
            raw::PUSH5 => push_n!(PUSH5, 5),
            raw::PUSH6 => push_n!(PUSH6, 6),
            raw::PUSH7 => push_n!(PUSH7, 7),
            raw::PUSH8 => push_n!(PUSH8, 8),
            raw::DUP => {
                trace!("dup");
                hook!(DUP);
                let idx = as_ptr!(*last!());
                trace!("idx = {idx:x}");
                let stack_idx = ok_or!(
                    self.stack.len().checked_sub(idx).and_then(|i| i.checked_sub(2)),
                    Error::<H>::InvalidStackIdx
                );

                unsafe {
                    *self.stack.last_mut().unwrap_unchecked() =
                        *self.stack.get_unchecked(stack_idx);
                };
            }
            raw::DUP0 => {
                trace!("dup0");
                hook!(DUP0);
                let stack_idx = ok_or!(self.stack.len().checked_sub(1), Error::<H>::StackEmpty);

                // SAFETY: Len is at least 1 as per above
                self.push_stack(unsafe { *self.stack.get_unchecked(stack_idx) });
            }
            raw::SWAP => {
                trace!("swap");
                hook!(SWAP);
                let idx = as_ptr!(pop!());
                let idx = ok_or!(idx.checked_add(2), Error::<H>::InvalidStackValue);
                let len = self.stack.len();
                if len < idx {
                    return Err(Error::<H>::InvalidStackIdx);
                }
                // SAFETY: Len is at least 1 as per above
                let a_idx = unsafe { len.unchecked_sub(1) };
                // SAFETY: Len is at least idx as per above
                let b_idx = unsafe { len.unchecked_sub(idx) };
                self.stack.swap(a_idx, b_idx);
            }
            raw::SWAP0 => {
                trace!("swap0");
                hook!(SWAP0);
                let b_idx = ok_or!(self.stack.len().checked_sub(2), Error::<H>::StackEmpty);
                // SAFETY: Len is at least 2 as per above
                let a_idx = unsafe { self.stack.len().unchecked_sub(1) };
                self.stack.swap(a_idx, b_idx);
            }
            raw::POP => {
                trace!("pop");
                hook!(POP);
                pop!();
            }
            raw::ALLOC => {
                trace!("alloc");
                hook!(ALLOC);
                let size = as_ptr!(pop!());
                match size.checked_add(self.memory.len()) {
                    Some(new_size) => {
                        if new_size > self.max_memory {
                            return Err(Error::<H>::OutOfMemory);
                        }
                    }
                    None => {
                        return Err(Error::<H>::InvalidStackValue);
                    }
                }
                if self.memory.try_reserve(size).is_err() {
                    return Err(Error::<H>::OutOfMemory);
                }
                self.memory.extend(vec![0; size]);
            }

            raw::WRITE1 => write_n!(WRITE1, 1),
            raw::WRITE2 => write_n!(WRITE2, 2),
            raw::WRITE3 => write_n!(WRITE3, 3),
            raw::WRITE4 => write_n!(WRITE4, 4),
            raw::WRITE5 => write_n!(WRITE5, 5),
            raw::WRITE6 => write_n!(WRITE6, 6),
            raw::WRITE7 => write_n!(WRITE7, 7),
            raw::WRITE8 => write_n!(WRITE8, 8),

            raw::READ1 => read_n!(READ1, 1),
            raw::READ2 => read_n!(READ2, 2),
            raw::READ3 => read_n!(READ3, 3),
            raw::READ4 => read_n!(READ4, 4),
            raw::READ5 => read_n!(READ5, 5),
            raw::READ6 => read_n!(READ6, 6),
            raw::READ7 => read_n!(READ7, 7),
            raw::READ8 => read_n!(READ8, 8),

            raw::DREAD1 => dread_n!(DREAD1, 1),
            raw::DREAD2 => dread_n!(DREAD2, 2),
            raw::DREAD3 => dread_n!(DREAD3, 3),
            raw::DREAD4 => dread_n!(DREAD4, 4),
            raw::DREAD5 => dread_n!(DREAD5, 5),
            raw::DREAD6 => dread_n!(DREAD6, 6),
            raw::DREAD7 => dread_n!(DREAD7, 7),
            raw::DREAD8 => dread_n!(DREAD8, 8),

            raw::DCOPY => {
                trace!("dcopy");
                hook!(DCOPY);
                if self.stack.len() < 3 {
                    return Err(Error::<H>::StackEmpty);
                };

                let [src, dst, len] = &self.stack[self.stack.len() - 3..] else {
                    unsafe { std::hint::unreachable_unchecked() }
                    // unreachable!();
                };

                let len = as_ptr!(*len);
                let dst = as_ptr!(*dst);
                let src = as_ptr!(*src);

                unsafe { self.stack.set_len(self.stack.len() - 3) };
                // self.stack.truncate(self.stack.len() - 3);

                trace!("len: {len:x}, dst: {dst:x}, src: {src:x}");

                let dst_end = try_add!(dst, len);
                trace!("dst_end: {dst_end:x}");

                let src_end = try_add!(src, len);
                trace!("src_end: {src_end:x}");

                ok_or!(self.memory.get_mut(dst..dst_end), Error::<H>::Segfault)
                    .copy_from_slice(ok_or!(self.data.get(src..src_end), Error::<H>::Segfault));
            }

            raw::DLEN => {
                trace!("dlen");
                hook!(DLEN);
                self.push_stack(self.data.len() as u64);
            }

            raw::ADD => binop!(ADD, "add", add),
            raw::SUB => binop!(SUB, "sub", sub),
            raw::MUL => binop!(MUL, "mul", mul),
            raw::DIV => {
                trace!("div");
                hook!(DIV);
                let len = self.stack.len();
                if len < 2 {
                    return Err(Error::<H>::StackEmpty);
                };
                let b = len - 2;
                let a = len - 1;
                unsafe {
                    *self.stack.get_unchecked_mut(b) =
                        match op::div(*self.stack.get_unchecked(b), *self.stack.get_unchecked(a)) {
                            Ok(ok) => ok,
                            Err(err) => return Err(err.widen()),
                        }
                };
                unsafe { self.stack.set_len(a) };
            }
            raw::EXP => binop!(EXP, "exp", expmod),
            raw::MOD => {
                trace!("mod");
                hook!(MOD);
                let a = pop!();
                let b = last!();
                trace!("{b:x} % {a:x}");
                *b = match op::r#mod(*b, a) {
                    Ok(ok) => ok,
                    Err(err) => return Err(err.widen()),
                };
            }
            raw::EQ => binop!(EQ, "eq", eq),
            raw::NEQ => binop!(NEQ, "neq", neq),
            raw::LT => binop!(LT, "lt", lt),
            raw::GT => binop!(GT, "gt", gt),
            raw::NOT => {
                trace!("not");
                hook!(NOT);
                let a = last!();
                *a = op::not(*a);
            }
            raw::SHR => binop!(SHR, "shr", shr),
            raw::SHL => binop!(SHL, "shl", shl),
            raw::NEG => {
                trace!("neg");
                hook!(NEG);
                let a = last!();
                *a = op::neg(*a);
            }
            raw::OR => binop!(OR, "or", or),
            raw::XOR => binop!(XOR, "xor", xor),
            raw::AND => binop!(AND, "and", and),

            raw::JUMP => {
                trace!("jump");
                hook!(JUMP);
                let dst = pop!();
                trace!("dst = {dst:x}");
                self.pc = as_ptr!(dst);
            }
            raw::JNZ => {
                trace!("jnz");
                hook!(JNZ);
                let dst = pop!();
                trace!("dst = {dst:x}");
                let value = pop!();
                trace!("value = {value:x}");
                if value != 0 {
                    self.pc = as_ptr!(dst);
                }
            }
            raw::CALL => {
                trace!("call");
                hook!(CALL);
                let top = last!();
                let address = *top;
                *top = self.pc as u64;
                self.pc = as_ptr!(address);
            }
            raw::EXIT => {
                trace!("exit");
                hook!(EXIT);
                let len = as_ptr!(pop!());
                let ptr = as_ptr!(pop!());

                return Ok(StepResult::Exit(
                    ok_or!(self.memory.get(ptr..try_add!(ptr, len)), Error::<H>::Segfault).to_vec(),
                ));
            }
            raw::TRAP => {
                trace!("trap");
                hook!(TRAP);
                let value = pop!();
                return Err(Error::<H>::Trap(value));
            }
            op => return Err(Error::<H>::UnknownOp(op)),
        }

        trace!("pc: {:x}", self.pc);
        trace!("stack: {:x?}", self.stack);
        trace!("memory: {}", self.memory.encode_hex());

        if let Err(err) = self.hook.post_cycle() {
            return Err(Error::Hook(err));
        };

        Ok(StepResult::Stepped)
    }
}

pub trait Hook {
    type Error: StdError;

    fn pre_cycle(&mut self) -> Result<(), Self::Error>;

    fn cycle(&mut self, pc: usize, op: Op, stack: &[u64], mem: &[u8]) -> Result<(), Self::Error>;

    fn post_cycle(&mut self) -> Result<(), Self::Error>;
}

impl Hook for () {
    type Error = !;

    fn pre_cycle(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn cycle(&mut self, _: usize, _: Op, _: &[u64], _: &[u8]) -> Result<(), Self::Error> {
        Ok(())
    }

    fn post_cycle(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CycleCountHook {
    cycles: u64,
}

impl CycleCountHook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cycles(&self) -> u64 {
        self.cycles
    }
}

impl Hook for CycleCountHook {
    type Error = !;

    #[inline]
    fn pre_cycle(&mut self) -> Result<(), Self::Error> {
        // trace!("cycles: {}", self.cycles + 1);
        Ok(())
    }

    #[inline]
    fn cycle(&mut self, _: usize, _: Op, _: &[u64], _: &[u8]) -> Result<(), Self::Error> {
        self.cycles += 1;
        Ok(())
    }

    fn post_cycle(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub mod op {
    use crate::Error;

    #[inline(always)]
    pub const fn add(a: u64, b: u64) -> u64 {
        a.wrapping_add(b)
    }

    #[inline(always)]
    pub const fn sub(a: u64, b: u64) -> u64 {
        a.wrapping_sub(b)
    }

    #[inline(always)]
    pub const fn mul(a: u64, b: u64) -> u64 {
        a.wrapping_mul(b)
    }

    #[inline(always)]
    pub const fn div(a: u64, b: u64) -> Result<u64, Error> {
        if b == 0 { Err(Error::DivideByZero) } else { Ok(a.wrapping_div(b)) }
    }

    #[inline(always)]
    pub const fn not(a: u64) -> u64 {
        (a == 0) as u64
    }

    #[inline(always)]
    pub const fn gt(a: u64, b: u64) -> u64 {
        (a > b) as u64
    }

    #[inline(always)]
    pub const fn lt(a: u64, b: u64) -> u64 {
        (a < b) as u64
    }

    #[inline(always)]
    pub const fn neq(a: u64, b: u64) -> u64 {
        (a != b) as u64
    }

    #[inline(always)]
    pub const fn eq(a: u64, b: u64) -> u64 {
        (a == b) as u64
    }

    #[inline(always)]
    pub const fn r#mod(a: u64, b: u64) -> Result<u64, Error> {
        if b == 0 { Err(Error::DivideByZero) } else { Ok(a.wrapping_rem(b)) }
    }

    #[inline(always)]
    pub const fn and(a: u64, b: u64) -> u64 {
        a & b
    }

    #[inline(always)]
    pub const fn xor(a: u64, b: u64) -> u64 {
        a ^ b
    }

    #[inline(always)]
    pub const fn or(a: u64, b: u64) -> u64 {
        a | b
    }

    #[inline(always)]
    pub const fn neg(a: u64) -> u64 {
        !a
    }

    #[inline]
    pub fn expmod(a: u64, b: u64) -> u64 {
        if b == 0 {
            return 1;
        }

        let mut acc: u128 = 1;
        let mut base = a as u128;
        let mut exp = b;

        loop {
            if (exp & 1) == 1 {
                acc = acc.strict_mul(base).rem_euclid(u64::MAX as _);
                // since exp!=0, finally the exp must be 1.
                if exp == 1 {
                    return acc as u64;
                }
            }
            exp >>= 1;
            base = base.strict_mul(base).rem_euclid(u64::MAX as _);
        }
    }

    pub fn shr(a: u64, shift: u64) -> u64 {
        a.unbounded_shr(shift.try_into().unwrap_or(u32::MAX))
    }

    pub fn shl(a: u64, shift: u64) -> u64 {
        a.unbounded_shl(shift.try_into().unwrap_or(u32::MAX))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn exp_ok() {
            assert_eq!(expmod(2, 24), 2_u64.pow(24));

            assert_eq!(expmod(3, 65), 7752514819847767473);

            assert_eq!(expmod(1844674407370955164, 18446744073709551), 8344168819056270514);
        }
    }
}

#[derive(Debug, Clone)]
pub enum StepResult {
    Stepped,
    Eof,
    Exit(Vec<u8>),
}

macro_rules! op {
    (pub enum $Op:ident {
        $($(#[$meta:meta])* $Variant:ident $(($tt:tt))* = $value:literal,)+
    }) => {
        #[derive(Debug, PartialEq, Clone, Copy, arbitrary::Arbitrary)]
        #[repr(u8)]
        pub enum $Op {
            $($(#[$meta])* $Variant $(($tt))* = $value,)+
        }

        pub mod raw {
            #[cfg(doc)]
            use super::Op;

            $($(#[$meta])* pub const $Variant: u8 = $value;)+
        }
    };
}

op! {
    pub enum Op {
        // STACK OPERATIONS (0x00-0x1f)
        PUSH0 = 0x00,

        /// Push a single byte to to the stack.
        PUSH1([u8; 1]) = 0x01,

        /// Push 2 bytes to the stack.
        PUSH2([u8; 2]) = 0x02,

        /// Push 3 bytes to the stack.
        PUSH3([u8; 3]) = 0x03,

        /// Push 4 bytes to the stack.
        PUSH4([u8; 4]) = 0x04,

        /// Push 5 bytes to the stack.
        PUSH5([u8; 5]) = 0x05,

        /// Push 6 bytes to the stack.
        PUSH6([u8; 6]) = 0x06,

        /// Push 7 bytes to the stack.
        PUSH7([u8; 7]) = 0x07,

        /// Push 8 bytes (a full word) to the stack.
        PUSH8([u8; 8]) = 0x08,

        /// Pop the item on the top of the stack as N and duplicate the Nth stack item.
        ///
        /// | Stack Input   | Stack Output  |
        /// | ------------- | ------------- |
        /// | `[..., a, N]` | `[..., a, a]` |
        DUP = 0x09,

        /// Duplicate the item on the top of the stack.
        ///
        /// | Stack Input | Stack Output  |
        /// | ------------| ------------- |
        /// | `[..., a]`  | `[..., a, a]` |
        DUP0 = 0x0a,

        /// Pop the item on the top of the stack as N and swap the first and (N-1)th stack items.
        ///
        /// | Stack Input                  | Stack Output                 |
        /// | ---------------------------- | ---------------------------- |
        /// | `[..., a, (...{n-1}), b, n]` | `[..., b, (...{n-1}), a, n]` |
        SWAP = 0x0b,

        /// Swap the top two items on the stack.
        ///
        /// | Stack Input   | Stack Output  |
        /// | ------------- | ------------- |
        /// | `[..., a, b]` | `[..., b, a]` |
        SWAP0 = 0x0c,

        /// Pop the top of the stack, returning an error if the stack is empty.
        ///
        /// | Stack Input | Stack Output |
        /// | ----------- | ------------ |
        /// | `[..., a]`  | `[...]`      |
        POP = 0x0d,

        // MEMORY OPERATIONS (0x20-0x3f)

        /// Pop the top of the stack anad grow the memory by that number of bytes.
        ///
        /// | Stack Input   | Stack Output |
        /// | ------------- | ------------ |
        /// | `[..., size]` | `[...]`      |
        ALLOC = 0x20,

        WRITE1 = 0x21,

        WRITE2 = 0x22,

        WRITE3 = 0x23,

        WRITE4 = 0x24,

        WRITE5 = 0x25,

        WRITE6 = 0x26,

        WRITE7 = 0x27,

        /// Write the full value on the top of the stack at the memory location of the
        /// 2nd top item in the stack.
        WRITE8 = 0x28,

        READ1 = 0x29,

        READ2 = 0x2a,

        READ3 = 0x2b,

        READ4 = 0x2c,

        READ5 = 0x2d,

        READ6 = 0x2e,

        READ7 = 0x2f,

        /// Read the full value at the memory location specified by the top value on the
        /// stack to the top of the stack.
        ///
        /// Unwritten memory is read as zero.
        READ8 = 0x30,

        DREAD1 = 0x31,

        DREAD2 = 0x32,

        DREAD3 = 0x33,

        DREAD4 = 0x34,

        DREAD5 = 0x35,

        DREAD6 = 0x36,

        DREAD7 = 0x37,

        /// Read the full value at the memory location specified by the top value on the
        /// stack to the top of the stack.
        ///
        /// Data read beyond the data length is read as zero.
        DREAD8 = 0x38,

        /// Copy a portion of data delimited by src..src+len to memory at dst.
        ///
        /// | Stack Input            | Stack Output           |
        /// | ---------------------- | ---------------------- |
        /// | `[..., src, dst, len]` | `[...]`                |
        DCOPY = 0x39,

        /// Push the length of the data to the stack.
        ///
        /// | Stack Input | Stack Output |
        /// | ----------- | ------------ |
        /// | `[...]`     | `[..., len]` |
        DLEN = 0x3a,

        // ARITHMETIC OPERATIONS (0x40-0x4f)

        /// Wrapping addition.
        ///
        /// | Stack Input   | Stack Output   |
        /// | ------------- | -------------- |
        /// | `[..., b, a]` | `[..., b + a]` |
        ADD = 0x40,

        /// Wrapping subtraction.
        ///
        /// | Stack Input   | Stack Output   |
        /// | ------------- | -------------- |
        /// | `[..., b, a]` | `[..., b - a]` |
        SUB = 0x41,

        /// Wrapping multiplication.
        ///
        /// | Stack Input   | Stack Output   |
        /// | ------------- | -------------- |
        /// | `[..., b, a]` | `[..., b * a]` |
        MUL = 0x42,

        /// Floor division.
        ///
        /// | Stack Input   | Stack Output    |
        /// | ------------- | --------------- |
        /// | `[..., b, a]` | `[..., b // a]` |
        ///
        /// This operation will return an error if the divisor is zero.
        DIV = 0x43,

        /// Wrapping exponentiation.
        ///
        /// | Stack Input   | Stack Output    |
        /// | ------------- | --------------- |
        /// | `[..., b, a]` | `[..., b ** a]` |
        EXP = 0x44,

        /// Modulus (remainder).
        ///
        /// | Stack Input   | Stack Output   |
        /// | ------------- | -------------- |
        /// | `[..., b, a]` | `[..., b % a]` |
        MOD = 0x45,

        /// Equality.
        ///
        /// | Stack Input   | Stack Output    |
        /// | ------------- | --------------- |
        /// | `[..., b, a]` | `[..., b == a]` |
        EQ = 0x4a,

        /// Inequality.
        ///
        /// | Stack Input   | Stack Output    |
        /// | ------------- | --------------- |
        /// | `[..., b, a]` | `[..., b != a]` |
        NEQ = 0x4b,

        /// Less-than comparison.
        ///
        /// | Stack Input   | Stack Output   |
        /// | ------------- | -------------- |
        /// | `[..., b, a]` | `[..., b < a]` |
        LT = 0x4c,

        /// Greater-than comparison.
        ///
        /// | Stack Input   | Stack Output   |
        /// | ------------- | -------------- |
        /// | `[..., b, a]` | `[..., b > a]` |
        GT = 0x4d,

        /// Logical NOT.
        ///
        /// | Stack Input | Stack Output |
        /// | ----------- | ------------ |
        /// | `[..., a]`  | `[..., !a]`  |
        ///
        /// Note that this is not bitwise negation (see [`Op::NEG`]). The value is treated as a boolean, and value pushed back to the stack will only ever be 0 or 1.
        NOT = 0x4e,

        /// Left shift.
        ///
        /// | Stack Input   | Stack Output    |
        /// | ------------- | --------------- |
        /// | `[..., b, a]` | `[..., a << b]` |
        ///
        /// The shift is "unbounded", and as such will always return 0 if the shift value is >= 64.
        SHL = 0x4f,

        /// Right shift.
        ///
        /// | Stack Input   | Stack Output    |
        /// | ------------- | --------------- |
        /// | `[..., b, a]` | `[..., a >> b]` |
        ///
        /// The shift is "unbounded", and as such will always return 0 if the shift value is >= 64.
        SHR = 0x50,

        /// Bitwise negation.
        ///
        /// | Stack Input | Stack Output |
        /// | ----------- | ------------ |
        /// | `[..., a]`  | `[..., ~a]`  |
        NEG = 0x51,

        /// Bitwise OR.
        ///
        /// | Stack Input   | Stack Output   |
        /// | ------------- | -------------- |
        /// | `[..., b, a]` | `[..., b | a]` |
        OR = 0x52,

        /// Bitwise XOR.
        ///
        /// | Stack Input   | Stack Output   |
        /// | ------------- | -------------- |
        /// | `[..., b, a]` | `[..., b ^ a]` |
        XOR = 0x53,

        /// Bitwise AND.
        ///
        /// | Stack Input   | Stack Output   |
        /// | ------------- | -------------- |
        /// | `[..., b, a]` | `[..., b & a]` |
        AND = 0x54,

        // CONTROL FLOW OPERATIONS (0xa0-0xaf)

        /// Pop an instruction pointer off the stack and jump to the address.
        ///
        /// | Stack Input   | Stack Output |
        /// | ------------- | ------------ |
        /// | `[..., addr]` | `[...]`      |
        JUMP = 0xa0,

        /// Pop the top value off of the stack and jump to the contained address
        /// if the value is non-zero.
        ///
        /// | Stack Input         | Stack Output |
        /// | ------------------- | ------------ |
        /// | `[..., addr, cond]` | `[...]`      |
        JNZ = 0xa1,

        /// Pop an instruction pointer off the stack, push the current address to the stack, and then jump to the previously popped address.
        ///
        /// | Stack Input   | Stack Output |
        /// | ------------- | ------------ |
        /// | `[..., addr]` | `[..., ret]` |
        CALL = 0xa2,

        /// Terminate execution with a payload. The top two values of the stack will
        /// be read as the pointer to and length of the return data.
        ///
        /// | Stack Input       | Stack Output           |
        /// | ----------------- | ---------------------- |
        /// | `[..., ptr, len]` | `<program terminates>` |
        EXIT = 0xa4,

        /// Terminate execution with an error code. The top value of the stack is
        /// used as the error code.
        ///
        /// | Stack Input   | Stack Output           |
        /// | ------------- | ---------------------- |
        /// | `[..., code]` | `<program terminates>` |
        TRAP = 0xa5,
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[inline(always)]
        fn u64_from_bytes(arr: &[u8]) -> u64 {
            let mut v = [0; 8];
            v[8 - arr.len()..].copy_from_slice(arr);
            u64::from_be_bytes(v)
        }

        match self {
            Op::PUSH0 => f.write_str("PUSH0"),
            Op::PUSH1(arr) => f.write_fmt(format_args!("PUSH1 0x{:x}", u64_from_bytes(arr))),
            Op::PUSH2(arr) => f.write_fmt(format_args!("PUSH2 0x{:x}", u64_from_bytes(arr))),
            Op::PUSH3(arr) => f.write_fmt(format_args!("PUSH3 0x{:x}", u64_from_bytes(arr))),
            Op::PUSH4(arr) => f.write_fmt(format_args!("PUSH4 0x{:x}", u64_from_bytes(arr))),
            Op::PUSH5(arr) => f.write_fmt(format_args!("PUSH5 0x{:x}", u64_from_bytes(arr))),
            Op::PUSH6(arr) => f.write_fmt(format_args!("PUSH6 0x{:x}", u64_from_bytes(arr))),
            Op::PUSH7(arr) => f.write_fmt(format_args!("PUSH7 0x{:x}", u64_from_bytes(arr))),
            Op::PUSH8(arr) => f.write_fmt(format_args!("PUSH8 0x{:x}", u64_from_bytes(arr))),
            Op::DUP => f.write_str("DUP"),
            Op::DUP0 => f.write_str("DUP0"),
            Op::SWAP => f.write_str("SWAP"),
            Op::SWAP0 => f.write_str("SWAP0"),
            Op::POP => f.write_str("POP"),
            Op::ALLOC => f.write_str("ALLOC"),
            Op::WRITE1 => f.write_str("WRITE1"),
            Op::WRITE2 => f.write_str("WRITE2"),
            Op::WRITE3 => f.write_str("WRITE3"),
            Op::WRITE4 => f.write_str("WRITE4"),
            Op::WRITE5 => f.write_str("WRITE5"),
            Op::WRITE6 => f.write_str("WRITE6"),
            Op::WRITE7 => f.write_str("WRITE7"),
            Op::WRITE8 => f.write_str("WRITE8"),
            Op::READ1 => f.write_str("READ1"),
            Op::READ2 => f.write_str("READ2"),
            Op::READ3 => f.write_str("READ3"),
            Op::READ4 => f.write_str("READ4"),
            Op::READ5 => f.write_str("READ5"),
            Op::READ6 => f.write_str("READ6"),
            Op::READ7 => f.write_str("READ7"),
            Op::READ8 => f.write_str("READ8"),
            Op::DREAD1 => f.write_str("DREAD1"),
            Op::DREAD2 => f.write_str("DREAD2"),
            Op::DREAD3 => f.write_str("DREAD3"),
            Op::DREAD4 => f.write_str("DREAD4"),
            Op::DREAD5 => f.write_str("DREAD5"),
            Op::DREAD6 => f.write_str("DREAD6"),
            Op::DREAD7 => f.write_str("DREAD7"),
            Op::DREAD8 => f.write_str("DREAD8"),
            Op::DCOPY => f.write_str("DCOPY"),
            Op::DLEN => f.write_str("DLEN"),
            Op::ADD => f.write_str("ADD"),
            Op::SUB => f.write_str("SUB"),
            Op::MUL => f.write_str("MUL"),
            Op::DIV => f.write_str("DIV"),
            Op::EXP => f.write_str("EXP"),
            Op::MOD => f.write_str("MOD"),
            Op::EQ => f.write_str("EQ"),
            Op::NEQ => f.write_str("NEQ"),
            Op::LT => f.write_str("LT"),
            Op::GT => f.write_str("GT"),
            Op::NOT => f.write_str("NOT"),
            Op::SHL => f.write_str("SHL"),
            Op::SHR => f.write_str("SHR"),
            Op::NEG => f.write_str("NEG"),
            Op::OR => f.write_str("OR"),
            Op::XOR => f.write_str("XOR"),
            Op::AND => f.write_str("AND"),
            Op::JUMP => f.write_str("JUMP"),
            Op::JNZ => f.write_str("JNZ"),
            Op::CALL => f.write_str("CALL"),
            Op::EXIT => f.write_str("EXIT"),
            Op::TRAP => f.write_str("TRAP"),
        }
    }
}

impl Op {
    pub fn to_bytes(self) -> Vec<u8> {
        match self {
            Op::PUSH0 => vec![raw::PUSH0],
            Op::PUSH1(v) => [raw::PUSH1].into_iter().chain(v).collect(),
            Op::PUSH2(v) => [raw::PUSH2].into_iter().chain(v).collect(),
            Op::PUSH3(v) => [raw::PUSH3].into_iter().chain(v).collect(),
            Op::PUSH4(v) => [raw::PUSH4].into_iter().chain(v).collect(),
            Op::PUSH5(v) => [raw::PUSH5].into_iter().chain(v).collect(),
            Op::PUSH6(v) => [raw::PUSH6].into_iter().chain(v).collect(),
            Op::PUSH7(v) => [raw::PUSH7].into_iter().chain(v).collect(),
            Op::PUSH8(v) => [raw::PUSH8].into_iter().chain(v).collect(),
            Op::DUP => vec![raw::DUP],
            Op::DUP0 => vec![raw::DUP0],
            Op::SWAP => vec![raw::SWAP],
            Op::SWAP0 => vec![raw::SWAP0],
            Op::POP => vec![raw::POP],
            Op::ALLOC => vec![raw::ALLOC],
            Op::WRITE1 => vec![raw::WRITE1],
            Op::WRITE2 => vec![raw::WRITE2],
            Op::WRITE3 => vec![raw::WRITE3],
            Op::WRITE4 => vec![raw::WRITE4],
            Op::WRITE5 => vec![raw::WRITE5],
            Op::WRITE6 => vec![raw::WRITE6],
            Op::WRITE7 => vec![raw::WRITE7],
            Op::WRITE8 => vec![raw::WRITE8],
            Op::READ1 => vec![raw::READ1],
            Op::READ2 => vec![raw::READ2],
            Op::READ3 => vec![raw::READ3],
            Op::READ4 => vec![raw::READ4],
            Op::READ5 => vec![raw::READ5],
            Op::READ6 => vec![raw::READ6],
            Op::READ7 => vec![raw::READ7],
            Op::READ8 => vec![raw::READ8],
            Op::DREAD1 => vec![raw::DREAD1],
            Op::DREAD2 => vec![raw::DREAD2],
            Op::DREAD3 => vec![raw::DREAD3],
            Op::DREAD4 => vec![raw::DREAD4],
            Op::DREAD5 => vec![raw::DREAD5],
            Op::DREAD6 => vec![raw::DREAD6],
            Op::DREAD7 => vec![raw::DREAD7],
            Op::DREAD8 => vec![raw::DREAD8],
            Op::DCOPY => vec![raw::DCOPY],
            Op::DLEN => vec![raw::DLEN],
            Op::ADD => vec![raw::ADD],
            Op::SUB => vec![raw::SUB],
            Op::MUL => vec![raw::MUL],
            Op::DIV => vec![raw::DIV],
            Op::EXP => vec![raw::EXP],
            Op::MOD => vec![raw::MOD],
            Op::EQ => vec![raw::EQ],
            Op::NEQ => vec![raw::NEQ],
            Op::LT => vec![raw::LT],
            Op::GT => vec![raw::GT],
            Op::NOT => vec![raw::NOT],
            Op::SHL => vec![raw::SHL],
            Op::SHR => vec![raw::SHR],
            Op::NEG => vec![raw::NEG],
            Op::OR => vec![raw::OR],
            Op::XOR => vec![raw::XOR],
            Op::AND => vec![raw::AND],
            Op::JUMP => vec![raw::JUMP],
            Op::JNZ => vec![raw::JNZ],
            Op::CALL => vec![raw::CALL],
            Op::EXIT => vec![raw::EXIT],
            Op::TRAP => vec![raw::TRAP],
        }
    }
}

#[derive(PartialEq, thiserror::Error)]
pub enum Error<H: Hook = ()> {
    /// Memory allocation failed.
    #[error("out of memory")]
    OutOfMemory,

    /// Attempted to pop off of an empty stack.
    #[error("stack empty")]
    StackEmpty,
    /// Attempted to read a stack index that doesn't exist.
    #[error("invalid stack idx")]
    InvalidStackIdx,
    /// Attempted to read past the max allocated memory address.
    #[error("segfault")]
    Segfault,
    /// Unexpected EOF when executing code.
    #[error("eof")]
    Eof,
    /// Attempted to divide by zero.
    #[error("divide by zero")]
    DivideByZero,
    /// Invalid stack value for operation.
    #[error("invalid stack value")]
    InvalidStackValue,

    /// Trap opcode was executed.
    #[error("trap: {0:#x}")]
    Trap(u64),
    /// Unknown operand.
    #[error("unknown op: {0:#x}")]
    UnknownOp(u8),

    #[error(transparent)]
    Hook(H::Error),

    #[cfg(not(target_pointer_width = "64"))]
    /// Attempted to use more memory than is addressable by the host system the
    /// vm was compiled for.
    #[error(
        "pointer {0} too large for host system (pointer width: {pw})",
        pw = usize::BITS,
    )]
    PointerTooBig(u64),
}

impl<H: Hook<Error: Debug>> Debug for Error<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfMemory => write!(f, "OutOfMemory"),
            Self::StackEmpty => write!(f, "StackEmpty"),
            Self::InvalidStackIdx => write!(f, "InvalidStackIdx"),
            Self::Segfault => write!(f, "Segfault"),
            Self::Eof => write!(f, "Eof"),
            Self::DivideByZero => write!(f, "DivideByZero"),
            Self::InvalidStackValue => write!(f, "InvalidStackValue"),
            Self::Trap(code) => f.debug_tuple("Trap").field(code).finish(),
            Self::UnknownOp(op) => f.debug_tuple("UnknownOp").field(op).finish(),
            Self::Hook(err) => f.debug_tuple("Hook").field(err).finish(),
            #[cfg(not(target_pointer_width = "64"))]
            Self::PointerTooBig(ptr) => f.debug_tuple("PointerTooBig").field(ptr).finish(),
        }
    }
}

impl<H: Hook<Error: Clone>> Clone for Error<H> {
    fn clone(&self) -> Self {
        match self {
            Self::OutOfMemory => Self::OutOfMemory,
            Self::StackEmpty => Self::StackEmpty,
            Self::InvalidStackIdx => Self::InvalidStackIdx,
            Self::Segfault => Self::Segfault,
            Self::Eof => Self::Eof,
            Self::DivideByZero => Self::DivideByZero,
            Self::InvalidStackValue => Self::InvalidStackValue,
            Self::Trap(code) => Self::Trap(*code),
            Self::UnknownOp(op) => Self::UnknownOp(*op),
            Self::Hook(err) => Self::Hook(err.clone()),
            #[cfg(not(target_pointer_width = "64"))]
            Self::PointerTooBig(ptr) => Self::PointerTooBig(ptr.clone()),
        }
    }
}

impl Error<()> {
    pub fn widen<H: Hook>(self) -> Error<H> {
        match self {
            Error::OutOfMemory => Error::OutOfMemory,
            Error::StackEmpty => Error::StackEmpty,
            Error::InvalidStackIdx => Error::InvalidStackIdx,
            Error::Segfault => Error::Segfault,
            Error::Eof => Error::Eof,
            Error::DivideByZero => Error::DivideByZero,
            Error::InvalidStackValue => Error::InvalidStackValue,
            Error::Trap(code) => Error::Trap(code),
            Error::UnknownOp(op) => Error::UnknownOp(op),
        }
    }
}
