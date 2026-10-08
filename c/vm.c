#include <assert.h>
#include <endian.h>
#include <stdbool.h>
#include <stdckdint.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "./vm.h"

#ifdef DEBUG
#include <stdio.h>
#define debug(...) printf(__VA_ARGS__)
#else
#define debug(...)
#endif

#ifdef DO_RESTRICT
#define RESTRICT restrict _Nonnull
#else
#define RESTRICT
#endif

// #define likely(x) __builtin_expect(!!(x), 1)
// #define unlikely(x) __builtin_expect(!!(x), 0)
// #define likely(x) x
// #define unlikely(x) x

#define bail(err) return err
// #include <assert.h>
// #define bail(err) assert(err)

#define ensure_stack(n, err)                                                   \
  {                                                                            \
    if (vm->stack.len < (n)) [[clang::unlikely]] {                             \
      bail(err);                                                               \
    }                                                                          \
  }

#define ensure_memory(ptr, n)                                                  \
  {                                                                            \
    uint64_t idx;                                                              \
    try_add(&idx, (ptr), (n), VM_ERR_INVALID_STACK_VALUE);                     \
    if (vm->memory.size < idx) [[clang::unlikely]] {                           \
      bail(VM_ERR_SEGFAULT);                                                   \
    }                                                                          \
  }

#define ensure_data(ptr, n)                                                    \
  {                                                                            \
    uint64_t idx;                                                              \
    try_add(&idx, (ptr), (n), VM_ERR_INVALID_STACK_VALUE);                     \
    if (vm->data.len < idx) [[clang::unlikely]] {                              \
      bail(VM_ERR_SEGFAULT);                                                   \
    }                                                                          \
  }

#define ensure_code(n)                                                         \
  {                                                                            \
    if (vm->code.len < (vm->pc + (n))) [[clang::unlikely]] {                   \
      vm->cycles--;                                                            \
      bail(VM_ERR_EOF);                                                        \
    }                                                                          \
  }

#define binop(op)                                                              \
  {                                                                            \
    register size_t len = vm->stack.len;                                       \
                                                                               \
    if (len < 2) [[clang::unlikely]] {                                         \
      bail(VM_ERR_STACK_EMPTY);                                                \
    }                                                                          \
                                                                               \
    register uint64_t lhs = vm->stack.data[len - 2];                           \
    register uint64_t rhs = vm->stack.data[len - 1];                           \
                                                                               \
    vm->stack.len -= 1;                                                        \
    vm->stack.data[len - 2] = op(lhs, rhs);                                    \
    DISPATCH()                                                                 \
  }

#define try(expr)                                                              \
  {                                                                            \
    register VmResult res = expr;                                              \
    if (res != VM_OK) [[clang::unlikely]] {                                    \
      bail(res);                                                               \
    }                                                                          \
  }

#define push_n(n)                                                              \
  {                                                                            \
    debug("PUSH%d\n", n);                                                      \
    ensure_code(n);                                                            \
    try(push_stack(&vm->stack, u64_from_bytes((n), vm->code.ptr + vm->pc)));   \
    vm->pc += (n);                                                             \
    DISPATCH()                                                                 \
  }

#define read_n(n)                                                              \
  {                                                                            \
    debug("READ%d\n", (n));                                                    \
    ensure_stack(1, VM_ERR_STACK_EMPTY);                                       \
    register size_t *RESTRICT top = &vm->stack.data[vm->stack.len - 1];        \
    register uint64_t ptr = *top;                                              \
    ensure_memory(ptr, (n));                                                   \
    register uint64_t res = u64_from_bytes((n), vm->memory.data + ptr);        \
    *top = res;                                                                \
    DISPATCH()                                                                 \
  }

#define dread_n(n)                                                             \
  {                                                                            \
    debug("DREAD%d\n", (n));                                                   \
    ensure_stack(1, VM_ERR_STACK_EMPTY);                                       \
    register uint64_t *RESTRICT top = &vm->stack.data[vm->stack.len - 1];      \
    register uint64_t ptr = *top;                                              \
    ensure_data(ptr, (n));                                                     \
    register uint64_t res = u64_from_bytes((n), vm->data.ptr + ptr);           \
    *top = res;                                                                \
    DISPATCH()                                                                 \
  }

#define write_n(n)                                                             \
  {                                                                            \
    debug("WRITE%d\n", (n));                                                   \
    ensure_stack(2, VM_ERR_STACK_EMPTY);                                       \
    register uint64_t value = vm->stack.data[vm->stack.len - 1];               \
    register size_t ptr = vm->stack.data[vm->stack.len - 2];                   \
    ensure_memory(ptr, (n));                                                   \
    write_u64((n), value, vm->memory.data + ptr);                              \
    vm->stack.len -= 2;                                                        \
    DISPATCH()                                                                 \
  }

#define try_add(res, val, n, err)                                              \
  {                                                                            \
    if (ckd_add((res), (val), (n))) [[clang::unlikely]] {                      \
      bail(err);                                                               \
    };                                                                         \
  }

typedef union u64 {
  uint64_t n;
  uint8_t bz[sizeof(uint64_t)];
} u64;

inline Fat new_fat(uint8_t const *RESTRICT ptr, size_t len) {
  return (Fat){.len = len, .ptr = ptr};
}

inline uint64_t u64_from_bytes(size_t n, const uint8_t *RESTRICT arr) {
  // static_assert (n <= 8 && n > 0);
  [[clang::assume(n <= 8)]];
  [[clang::assume(n > 0)]];

  register u64 out = {0};
  memcpy(out.bz, arr, n);
  debug("n: %zu, value: %02lx\n", n, out.n);
#if BYTE_ORDER == __LITTLE_ENDIAN
  out.n = htobe64(out.n) >> (sizeof(uint64_t) * (sizeof(uint64_t) - n));
#endif
  return out.n;
}

inline void write_u64(size_t n, uint64_t value, uint8_t *RESTRICT buffer) {
  [[clang::assume(n <= 8)]];
  [[clang::assume(n > 0)]];

  register u64 out = {0};
#if BYTE_ORDER == __LITTLE_ENDIAN
  out.n = be64toh(value << (8 * (8 - n)));
#endif
  memcpy(buffer, out.bz, n);
  debug("WRITE_U64: n: %zu, value: %016lx, out: %016lx\n", n, value, out.n);
}

inline VmResult push_stack(Stack *RESTRICT stack, uint64_t value) {
  if (stack->capacity == 0) [[clang::unlikely]] {
    register uint64_t *RESTRICT ptr =
        (uint64_t *RESTRICT)malloc(4 * sizeof(uint64_t));
    if (ptr == NULL) [[clang::unlikely]] {
      return VM_ERR_OUT_OF_MEMORY;
    }
    stack->data = ptr;
    stack->capacity = 4;
  } else if (stack->len == stack->capacity) [[clang::unlikely]] {
    register uint64_t *RESTRICT ptr = (uint64_t *RESTRICT)realloc(
        stack->data, stack->capacity * 2 * sizeof(uint64_t));
    if (ptr == NULL) [[clang::unlikely]] {
      return VM_ERR_OUT_OF_MEMORY;
    }
    stack->data = ptr;
    stack->capacity *= 2;
  }

  stack->data[stack->len] = value;
  stack->len++;

  return VM_OK;
}

inline VmResult alloc_memory(Memory *RESTRICT memory, size_t additional) {
  if (additional == 0) [[clang::unlikely]] {
    return VM_OK;
  }
  register uint8_t *RESTRICT ptr =
      (uint8_t *RESTRICT)realloc(memory->data, memory->size + additional);
  if (ptr == NULL) [[clang::unlikely]] {
    return VM_ERR_OUT_OF_MEMORY;
  }
  memory->data = ptr;

  memset(ptr + memory->size, 0, additional);

  memory->size += additional;

  return VM_OK;
}

inline VmResult pop_stack(Stack *RESTRICT stack, uint64_t *RESTRICT value) {
  if (stack->len == 0) [[clang::unlikely]] {
    return VM_ERR_STACK_EMPTY;
  }

  stack->len--;
  *value = stack->data[stack->len];

  return VM_OK;
}

inline VmResult run_vm(Vm *RESTRICT vm) {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Winitializer-overrides"
  static void *ops_table[256] = {
      [0 ... 255] = &&unknown_op,
      [0x00] = &&_push0,
      [0x01] = &&_push1,
      [0x02] = &&_push2,
      [0x03] = &&_push3,
      [0x04] = &&_push4,
      [0x05] = &&_push5,
      [0x06] = &&_push6,
      [0x07] = &&_push7,
      [0x08] = &&_push8,
      [0x09] = &&_dup,
      [0x0a] = &&_dup0,
      [0x0b] = &&_swap,
      [0x0c] = &&_swap0,
      [0x0d] = &&_pop,
      [0x20] = &&_alloc,
      [0x21] = &&_write1,
      [0x22] = &&_write2,
      [0x23] = &&_write3,
      [0x24] = &&_write4,
      [0x25] = &&_write5,
      [0x26] = &&_write6,
      [0x27] = &&_write7,
      [0x28] = &&_write8,
      [0x29] = &&_read1,
      [0x2a] = &&_read2,
      [0x2b] = &&_read3,
      [0x2c] = &&_read4,
      [0x2d] = &&_read5,
      [0x2e] = &&_read6,
      [0x2f] = &&_read7,
      [0x30] = &&_read8,
      [0x31] = &&_dread1,
      [0x32] = &&_dread2,
      [0x33] = &&_dread3,
      [0x34] = &&_dread4,
      [0x35] = &&_dread5,
      [0x36] = &&_dread6,
      [0x37] = &&_dread7,
      [0x38] = &&_dread8,
      [0x39] = &&_dcopy,
      [0x3a] = &&_dlen,
      [0x40] = &&_add,
      [0x41] = &&_sub,
      [0x42] = &&_mul,
      [0x43] = &&_div,
      [0x44] = &&_exp,
      [0x45] = &&_mod,
      [0x4a] = &&_eq,
      [0x4b] = &&_neq,
      [0x4c] = &&_lt,
      [0x4d] = &&_gt,
      [0x4e] = &&_not,
      [0x4f] = &&_shl,
      [0x50] = &&_shr,
      [0x51] = &&_neg,
      [0x52] = &&_or,
      [0x53] = &&_xor,
      [0x54] = &&_and,
      [0xa0] = &&_jump,
      [0xa1] = &&_jnz,
      [0xa2] = &&_call,
      [0xa4] = &&_exit,
      [0xa5] = &&_trap,
  };
#pragma clang diagnostic pop

  register uint8_t op;

#define DISPATCH()                                                             \
  {                                                                            \
    if (vm->pc >= vm->code.len) [[clang::unlikely]] {                          \
      return VM_OK;                                                            \
    } else [[clang::likely]] {                                                 \
      op = vm->code.ptr[vm->pc];                                               \
      vm->cycles++;                                                            \
      vm->pc++;                                                                \
      goto *ops_table[op];                                                     \
    }                                                                          \
  }
  // debug("pc: %zu, op: %02x\n", vm->pc, op); \
  // debug("stack (%ld): [ ", vm->stack.len); \
  // for (int i = 0; i < vm->stack.len; i++) { \
  //   debug("%016lx ", vm->stack.data[i]); \
  // } \
  // debug("]\n"); \
  // debug("memory (%ld)\n", vm->memory.size); \
  // debug("\n"); \
  // debug("memory (%ld): ", vm->memory.size); \
  // for (int i = 0; i < vm->memory.size; i++) { \
  //   debug("%02x", vm->memory.data[i]); \
  // } \

  DISPATCH()

_push0: { try(push_stack(&vm->stack, 0)) DISPATCH() }
_push1: { push_n(1) }
_push2: { push_n(2) }
_push3: { push_n(3) }
_push4: { push_n(4) }
_push5: { push_n(5) }
_push6: { push_n(6) }
_push7: { push_n(7) }
_push8: { push_n(8) }
_dup: {
  debug("DUP\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  register uint64_t *RESTRICT idx = &vm->stack.data[vm->stack.len - 1];
  uint64_t stack_idx;
  try_add(&stack_idx, *idx, 2, VM_ERR_INVALID_STACK_IDX);
  debug("idx: %lu\n", *idx);
  ensure_stack(stack_idx, VM_ERR_INVALID_STACK_IDX);
  *idx = vm->stack.data[vm->stack.len - stack_idx];
  DISPATCH()
}
_dup0: {
  // debug("DUP0\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  try(push_stack(&vm->stack, vm->stack.data[vm->stack.len - 1]));
  DISPATCH()
}
_swap: {
  // debug("SWAP\n");
  uint64_t idx = 0;
  try(pop_stack(&vm->stack, &idx))
      try_add(&idx, idx, 2, VM_ERR_INVALID_STACK_VALUE) size_t len =
          vm->stack.len;
  if (len < idx) [[clang::unlikely]] {
    bail(VM_ERR_INVALID_STACK_IDX);
  }
  register size_t a_idx = len - 1;
  register size_t b_idx = len - idx;
  register uint64_t a = vm->stack.data[a_idx];
  vm->stack.data[a_idx] = vm->stack.data[b_idx];
  vm->stack.data[b_idx] = a;
  DISPATCH()
}
_swap0: {
  // debug("SWAP0\n");
  ensure_stack(2, VM_ERR_STACK_EMPTY);
  register uint64_t a = vm->stack.data[vm->stack.len - 1];
  vm->stack.data[vm->stack.len - 1] = vm->stack.data[vm->stack.len - 2];
  vm->stack.data[vm->stack.len - 2] = a;
  DISPATCH()
}
_pop: {
  // debug("POP\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  vm->stack.len--;
  DISPATCH()
}
_alloc: {
  // debug("ALLOC\n");
  uint64_t value = 0;
  try(pop_stack(&vm->stack, &value));
  size_t new_size;
  try_add(&new_size, value, vm->memory.size, VM_ERR_INVALID_STACK_VALUE);
  if (new_size > vm->max_memory) [[clang::unlikely]] {
    bail(VM_ERR_OUT_OF_MEMORY);
  }
  // debug("%lu\n", value);
  try(alloc_memory(&vm->memory, value));
  DISPATCH()
}

_write1: { write_n(1) }
_write2: { write_n(2) }
_write3: { write_n(3) }
_write4: { write_n(4) }
_write5: { write_n(5) }
_write6: { write_n(6) }
_write7: { write_n(7) }
_write8: { write_n(8) }
_read1: { read_n(1) }
_read2: { read_n(2) }
_read3: { read_n(3) }
_read4: { read_n(4) }
_read5: { read_n(5) }
_read6: { read_n(6) }
_read7: { read_n(7) }
_read8: { read_n(8) }
_dread1: { dread_n(1) }
_dread2: { dread_n(2) }
_dread3: { dread_n(3) }
_dread4: { dread_n(4) }
_dread5: { dread_n(5) }
_dread6: { dread_n(6) }
_dread7: { dread_n(7) }
_dread8: { dread_n(8) }
_dcopy: {
  // debug("DCOPY\n");
  if (vm->stack.len < 3) [[clang::unlikely]] {
    bail(VM_ERR_STACK_EMPTY);
  }

  register uint64_t len = vm->stack.data[vm->stack.len - 1];
  register uint64_t dst = vm->stack.data[vm->stack.len - 2];
  register uint64_t src = vm->stack.data[vm->stack.len - 3];

  debug("len: %lx, dst: %lx, src: %lx\n", len, dst, src);

  {
    uint64_t dst_idx;
    try_add(&dst_idx, dst, len, VM_ERR_INVALID_STACK_VALUE);
    debug("dst len: %lx\n", dst_idx);

    uint64_t src_idx;
    try_add(&src_idx, src, len, VM_ERR_INVALID_STACK_VALUE);
    debug("src len: %lx\n", src_idx);

    if (vm->data.len < src_idx) [[clang::unlikely]] {
      bail(VM_ERR_SEGFAULT);
    }
    if (vm->memory.size < dst_idx) [[clang::unlikely]] {
      bail(VM_ERR_SEGFAULT);
    }
  }

  vm->stack.len -= 3;

  uint64_t memory_ptr;
  try_add(&memory_ptr, (size_t)vm->memory.data, dst,
          VM_ERR_INVALID_STACK_VALUE);

  uintptr_t data_ptr;
  try_add(&data_ptr, (uintptr_t)vm->data.ptr, (uintptr_t)src,
          VM_ERR_INVALID_STACK_VALUE);

  memcpy(vm->memory.data + dst, vm->data.ptr + src, len);
  DISPATCH()
}
_dlen: {
  try(push_stack(&vm->stack, vm->data.len));
  DISPATCH()
}
_add: { binop(op_add) }
_sub: { binop(op_sub) }
_mul: { binop(op_mul) }
_div: {
  register size_t len = vm->stack.len;
  if (len < 2) [[clang::unlikely]] {
    bail(VM_ERR_STACK_EMPTY);
  }
  register uint64_t lhs = vm->stack.data[len - 2];
  register uint64_t rhs = vm->stack.data[len - 1];
  vm->stack.len -= 1;
  if (rhs == 0) [[clang::unlikely]] {
    bail(VM_ERR_DIVIDE_BY_ZERO);
  }
  vm->stack.data[len - 2] = op_div(lhs, rhs);
  DISPATCH()
};
_exp:
  binop(op_expmod);
_mod: {
  register size_t len = vm->stack.len;
  if (len < 2) [[clang::unlikely]] {
    bail(VM_ERR_STACK_EMPTY);
  }
  register uint64_t lhs = vm->stack.data[len - 2];
  register uint64_t rhs = vm->stack.data[len - 1];
  vm->stack.len -= 1;
  if (rhs == 0) [[clang::unlikely]] {
    bail(VM_ERR_DIVIDE_BY_ZERO);
  }
  vm->stack.data[len - 2] = op_mod(lhs, rhs);
  DISPATCH()
};
_eq: { binop(op_eq) }
_neq: { binop(op_neq) }
_lt: { binop(op_lt) }
_gt: { binop(op_gt) }
_not: {
  // debug("NOT\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  register uint64_t *RESTRICT a = &vm->stack.data[vm->stack.len - 1];
  *a = op_not(*a);
  DISPATCH()
}
_shl: { binop(op_shl) }
_shr: { binop(op_shr) }
_neg: {
  // debug("NEG\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  register uint64_t *RESTRICT a = &vm->stack.data[vm->stack.len - 1];
  *a = op_neg(*a);
  DISPATCH()
};
_or: { binop(op_or) }
_xor: { binop(op_xor) }
_and: { binop(op_and) }
_jump: {
  // debug("JUMP\n");
  try(pop_stack(&vm->stack, &vm->pc));
  DISPATCH()
}
_jnz: {
  // debug("JNZ\n");

  ensure_stack(2, VM_ERR_STACK_EMPTY);

  register uint64_t dst = vm->stack.data[vm->stack.len - 1];
  register uint64_t value = vm->stack.data[vm->stack.len - 2];

  vm->stack.len -= 2;

  if (value != 0) {
    vm->pc = dst;
  }
  DISPATCH()
}
_call: {
  // debug("CALL\n");

  ensure_stack(1, VM_ERR_STACK_EMPTY);

  register uint64_t *RESTRICT top = &vm->stack.data[vm->stack.len - 1];
  register uint64_t address = *top;
  *top = vm->pc;
  vm->pc = address;
  DISPATCH()
}
_exit:
  [[clang::unlikely]] {
    // debug("EXIT\n");
    ensure_stack(2, VM_ERR_STACK_EMPTY);

    // debug("memory: ");
    // for (int i; i < vm->memory.size; i++) {
    //   // debug("%02x", vm->memory.data[i]);
    // }
    // debug("\n");

    register uint64_t len = vm->stack.data[vm->stack.len - 1];
    register uint64_t ptr = vm->stack.data[vm->stack.len - 2];
    vm->stack.len -= 2;

    size_t out_ptr;
    try_add(&out_ptr, ptr, len, VM_ERR_INVALID_STACK_VALUE);
    if (out_ptr > vm->memory.size) [[clang::unlikely]] {
      return VM_ERR_SEGFAULT;
    }
    vm->out.exit = new_fat(vm->memory.data + ptr, len);
    return VM_STEP_RESULT_EXIT;
  }
_trap:
  [[clang::unlikely]] {
    debug("TRAP\n");
    ensure_stack(1, VM_ERR_STACK_EMPTY);
    vm->stack.len--;
    vm->out.trap = vm->stack.data[vm->stack.len];
    return VM_STEP_RESULT_TRAP;
  }
unknown_op:
  [[clang::unlikely]] {
    // DISPATCH() eagerly increments the cycle count
    vm->cycles--;
    debug("unknown op %02x\n", op);
    bail(VM_ERR_UNKNOWN_OP);
  }
}

Vm new_vm(Fat code, Fat data, size_t max_memory) {
  return (Vm){
      .code = code,
      .data = data,
      .stack =
          {
              .data = 0,
              .capacity = 0,
              .len = 0,
          },
      .memory =
          {
              .data = 0,
              .size = 0,
          },
      .pc = 0,
      .out =
          {
              .trap = 0,
          },
      .cycles = 0,
      .max_memory = max_memory,
  };
}

void drop_vm(Vm *RESTRICT vm) {
  free(vm->stack.data);
  free(vm->memory.data);
}

// ENTRYPOINT

// int readFile(char *path, uint8_t **out, size_t *size) {
//   FILE *infile;

//   infile = fopen(path, "r");

//   if (infile == NULL) {
//     return 1;
//   }

//   fseek(infile, 0L, SEEK_END);
//   *size = ftell(infile);
//   // debug("file size: %lu\n", *size);

//   fseek(infile, 0L, SEEK_SET);

//   *out = (uint8_t *)calloc(*size, sizeof(uint8_t));

//   if (out == NULL) {
//     fclose(infile);
//     return 1;
//   }

//   unsigned long res = fread(*out, sizeof(char), *size, infile);
//   if (res != *size) {
//     free(*out);
//     fclose(infile);
//     // debug("only read %lu/%zu bytes\n", res, *size);
//     return 2;
//   }
//   fclose(infile);

//   return 0;
// }

// int main(int argc, char *argv[]) {
//   if (argc < 2) {
//     fprintf(stderr, "missing argument\n");
//     return 1;
//   }

//   uint8_t *code;
//   size_t code_len;
//   int code_res = readFile(argv[1], &code, &code_len);
//   if (code_res != 0) {
//     // debug("unable to read code: %d\n", code_res);
//     return code_res;
//   }

//   // debug("code size: %zu\n", code_len);

//   uint8_t *data;
//   size_t data_len;
//   int data_res = readFile(argv[2], &data, &data_len);
//   if (data_res != 0) {
//     // debug("unable to read data: %d\n", data_res);
//     free(code);
//     return data_res;
//   }

//   // debug("data size: %zu\n", data_len);

//   Vm vm = new_vm(new_fat(code, code_len), new_fat(data, data_len));

//   VmResult res = run_vm(&vm);

//   switch (__builtin_expect(res, VM_OK)) {
//   case VM_OK: {
//     fprintf(stdout, "done\n");
//     break;
//   }
//   case VM_STEP_RESULT_EOF: {
//     fprintf(stdout, "eof\n");
//     break;
//   }
//   case VM_STEP_RESULT_TRAP: {
//     fprintf(stdout, "trap %zu\n", vm.out.trap);
//     break;
//   }
//   case VM_STEP_RESULT_EXIT: {
//     fprintf(stdout, "exit ");
//     for (size_t i = 0; i < vm.out.exit.len; i++) {
//       fprintf(stdout, "%02x", vm.out.exit.ptr[i]);
//     }
//     fprintf(stdout, "\n");
//     break;
//   }
//   default:
//     fprintf(stdout, "error: %d", res);
//     break;
//   }

//   free((void *)vm.code.ptr);
//   free((void *)vm.data.ptr);
//   free(vm.memory.data);
//   free(vm.stack.data);
// }
