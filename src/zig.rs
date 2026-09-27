use core::slice;
use std::ffi::c_void;

use crate::{CycleCountVm, VmRunResult, VmT};

#[repr(C)]
struct RunResult {
    cycles: u64,
    tag: RunResultTag,
    data: RunResultUnion,
}

#[repr(u8)]
#[expect(unused, reason = "created in zig")]
enum RunResultTag {
    Done = 0,
    Trap = 1,
    Exit = 2,
    Err = 3,
}

#[repr(C)]
union RunResultUnion {
    trap: u64,
    exit: ExitData,
    err: Err,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct ExitData {
    ptr: *const u8,
    len: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum Err {
    OutOfMemory,

    StackEmpty,
    InvalidStackIdx,
    Segfault,
    Eof,
    DivideByZero,
    InvalidStackValue,
    UnknownOp,
    PointerTooBig,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0:?}")]
    Err(Err),
    #[error("trap: {0:x}")]
    Trap(u64),
}

unsafe extern "C" {
    fn zig_allocator() -> *mut c_void;
    fn zig_init(
        gpa: *mut c_void,
        code: *mut u8,
        code_len: usize,
        data: *const u8,
        data_len: usize,
        max_memory: usize,
    ) -> *mut c_void;
    fn zig_drop(vm: *mut c_void);
    fn zig_run(vm: *mut c_void) -> RunResult;
    fn zig_cycles(vm: *mut c_void) -> u64;
}

pub struct Vm {
    ptr: *mut c_void,
    // gpa: *mut c_void,
    code_ptr: *mut u8,
    code_len: usize,
    data_ptr: *mut u8,
    data_len: usize,
}

impl VmT for Vm {
    fn new(mut code: Vec<u8>, mut data: Vec<u8>, max_memory: usize) -> Self {
        code.shrink_to_fit();
        data.shrink_to_fit();
        let (code_ptr, code_len, _) = code.into_raw_parts();
        let (data_ptr, data_len, _) = data.into_raw_parts();
        let gpa = unsafe { zig_allocator() };
        let ptr = unsafe { zig_init(gpa, code_ptr, code_len, data_ptr, data_len, max_memory) };
        Self {
            ptr,
            // gpa,
            code_ptr,
            code_len,
            data_ptr,
            data_len,
        }
    }

    fn run(&mut self) -> VmRunResult {
        let res = unsafe { zig_run(self.ptr) };
        match res.tag {
            RunResultTag::Done => VmRunResult::Done,
            RunResultTag::Trap => VmRunResult::Trap(unsafe { res.data.trap }),
            RunResultTag::Exit => VmRunResult::Exit(
                unsafe { slice::from_raw_parts(res.data.exit.ptr, res.data.exit.len) }.to_vec(),
            ),
            RunResultTag::Err => {
                let err = unsafe { res.data.err };
                match err {
                    Err::OutOfMemory => VmRunResult::OutOfMemory,
                    Err::StackEmpty => VmRunResult::StackEmpty,
                    Err::InvalidStackIdx => VmRunResult::InvalidStackIdx,
                    Err::Segfault => VmRunResult::Segfault,
                    Err::Eof => VmRunResult::Eof,
                    Err::DivideByZero => VmRunResult::DivideByZero,
                    Err::InvalidStackValue => VmRunResult::InvalidStackValue,
                    Err::UnknownOp => VmRunResult::UnknownOp,
                    Err::PointerTooBig => VmRunResult::PointerTooBig,
                }
            }
        }
    }
}

impl Drop for Vm {
    fn drop(&mut self) {
        unsafe {
            Vec::from_raw_parts(self.code_ptr, self.code_len, self.code_len);
            Vec::from_raw_parts(self.data_ptr, self.data_len, self.data_len);
            zig_drop(self.ptr);
        }
    }
}

impl CycleCountVm for Vm {
    fn cycles(&mut self) -> u64 {
        unsafe { zig_cycles(self.ptr) }
    }
}
