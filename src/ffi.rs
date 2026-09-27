use core::slice;

use crate::{CycleCountVm, VmRunResult, VmT};

mod bindings {
    #![expect(dead_code, non_camel_case_types)]

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub struct Vm {
    vm: bindings::Vm,
}

impl VmT for Vm {
    fn new(mut code: Vec<u8>, mut data: Vec<u8>, max_memory: usize) -> Self {
        code.shrink_to_fit();
        data.shrink_to_fit();
        let (code_ptr, code_len, _) = code.into_raw_parts();
        let (data_ptr, data_len, _) = data.into_raw_parts();
        Self {
            vm: unsafe {
                bindings::new_vm(
                    bindings::Fat { ptr: code_ptr, len: code_len },
                    bindings::Fat { ptr: data_ptr, len: data_len },
                    max_memory,
                )
            },
        }
    }

    fn run(&mut self) -> VmRunResult {
        match unsafe { bindings::run_vm(&mut self.vm) } {
            bindings::VmResult::VM_OK => VmRunResult::Done,
            bindings::VmResult::VM_STEP_RESULT_TRAP => {
                VmRunResult::Trap(unsafe { self.vm.out.trap })
            }
            bindings::VmResult::VM_STEP_RESULT_EXIT => VmRunResult::Exit(unsafe {
                if self.vm.out.exit.len == 0 {
                    vec![]
                } else {
                    slice::from_raw_parts(self.vm.out.exit.ptr, self.vm.out.exit.len).to_vec()
                }
            }),
            bindings::VmResult::VM_ERR_OUT_OF_MEMORY => VmRunResult::OutOfMemory,
            bindings::VmResult::VM_ERR_STACK_EMPTY => VmRunResult::StackEmpty,
            bindings::VmResult::VM_ERR_INVALID_STACK_IDX => VmRunResult::InvalidStackIdx,
            bindings::VmResult::VM_ERR_SEGFAULT => VmRunResult::Segfault,
            bindings::VmResult::VM_ERR_EOF => VmRunResult::Eof,
            bindings::VmResult::VM_ERR_DIVIDE_BY_ZERO => VmRunResult::DivideByZero,
            bindings::VmResult::VM_ERR_INVALID_STACK_VALUE => VmRunResult::InvalidStackValue,
            bindings::VmResult::VM_ERR_UNKNOWN_OP => VmRunResult::UnknownOp,
        }
    }
}

impl Drop for Vm {
    fn drop(&mut self) {
        unsafe {
            Vec::from_raw_parts(self.vm.code.ptr.cast_mut(), self.vm.code.len, self.vm.code.len);
            Vec::from_raw_parts(self.vm.data.ptr.cast_mut(), self.vm.data.len, self.vm.data.len);
            bindings::drop_vm(&mut self.vm);
        };
    }
}

impl CycleCountVm for Vm {
    fn cycles(&mut self) -> u64 {
        self.vm.cycles
    }
}
