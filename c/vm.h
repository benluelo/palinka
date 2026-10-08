#include <stddef.h>
#include <stdint.h>

#include "./op.h"

typedef enum VmResult : int8_t {
  /// Signals no error. The operation completed successfully.
  VM_OK = 0,

  VM_STEP_RESULT_TRAP = 1,
  VM_STEP_RESULT_EXIT = 2,

  /// Out of memory.
  VM_ERR_OUT_OF_MEMORY = -1,
  /// Attempted to pop off of an empty stack.
  VM_ERR_STACK_EMPTY = -2,
  /// Attempted to read a stack index that doesn't exist.
  VM_ERR_INVALID_STACK_IDX = -3,
  /// Attempted to read past the max allocated memory address.
  VM_ERR_SEGFAULT = -4,
  /// Unexpected EOF when executing code.
  VM_ERR_EOF = -5,
  /// Attempted to divide by zero.
  VM_ERR_DIVIDE_BY_ZERO = -6,
  /// Invalid stack value for operation.
  VM_ERR_INVALID_STACK_VALUE = -7,
  /// Unknown opcode.
  VM_ERR_UNKNOWN_OP = -8,
} VmResult;

/** <div rustbindgen mustusetype></div> */
typedef struct Stack {
  size_t len;
  size_t capacity;
  uint64_t *restrict data __attribute__((counted_by(len)));
} Stack;

/** <div rustbindgen mustusetype></div> */
typedef struct Memory {
  size_t size;
  uint8_t *restrict data __attribute__((counted_by(size)));
} Memory;

/** <div rustbindgen mustusetype></div> */
typedef struct Fat {
  const uint8_t *restrict ptr;
  size_t len;
} Fat;

/** <div rustbindgen mustusetype></div> */
typedef struct Vm {
  Fat code;
  Fat data;
  Stack stack;
  Memory memory;
  size_t pc;
  union Out {
    uint64_t trap;
    Fat exit;
  } out;
  uint64_t cycles;
  size_t max_memory;
} Vm;

VmResult run_vm(Vm *restrict vm);

Vm new_vm(Fat code, Fat data, size_t max_memory);

void drop_vm(Vm *restrict vm);
