#include <assert.h>
#include <endian.h>
#include <stdbool.h>
#include <stdckdint.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "./palinka.h"

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

#define ensure_stack(n, err)                                                   \
  {                                                                            \
    if (vm->stack.len < (n)) [[clang::unlikely]] {                             \
      return (err);                                                            \
    }                                                                          \
  }                                                                            \
  static_assert(true, "")

#define ensure_memory(ptr, n)                                                  \
  {                                                                            \
    uint64_t idx;                                                              \
    try_add(&idx, (ptr), (n), VM_ERR_INVALID_STACK_VALUE);                     \
    if (vm->memory.size < idx) [[clang::unlikely]] {                           \
      return (VM_ERR_SEGFAULT);                                                \
    }                                                                          \
  }

#define ensure_data(ptr, n)                                                    \
  {                                                                            \
    uint64_t idx;                                                              \
    try_add(&idx, (ptr), (n), VM_ERR_INVALID_STACK_VALUE);                     \
    if (vm->data.len < idx) [[clang::unlikely]] {                              \
      return (VM_ERR_SEGFAULT);                                                \
    }                                                                          \
  }

#define ensure_code(n)                                                         \
  {                                                                            \
    if (vm->code.len < (vm->pc + (n))) [[clang::unlikely]] {                   \
      vm->cycles--;                                                            \
      return (VM_ERR_EOF);                                                     \
    }                                                                          \
  }

#define binop(op)                                                              \
  {                                                                            \
    register size_t len = vm->stack.len;                                       \
                                                                               \
    if (len < 2) [[clang::unlikely]] {                                         \
      return (VM_ERR_STACK_EMPTY);                                             \
    }                                                                          \
                                                                               \
    register word lhs = vm->stack.data[len - 2];                               \
    register word rhs = vm->stack.data[len - 1];                               \
                                                                               \
    vm->stack.len -= 1;                                                        \
    vm->stack.data[len - 2] = op(lhs, rhs);                                    \
    DISPATCH()                                                                 \
  }

#define nz_binop(op)                                                           \
  register size_t len = vm->stack.len;                                         \
  if (len < 2) [[clang::unlikely]] {                                           \
    return (VM_ERR_STACK_EMPTY);                                               \
  }                                                                            \
  register word lhs = vm->stack.data[len - 2];                                 \
  register word rhs = vm->stack.data[len - 1];                                 \
  vm->stack.len -= 1;                                                          \
  if (rhs == 0) [[clang::unlikely]] {                                          \
    return (VM_ERR_DIVIDE_BY_ZERO);                                            \
  }                                                                            \
  vm->stack.data[len - 2] = op(lhs, rhs);                                      \
  DISPATCH()

#define unop(op)                                                               \
  ensure_stack(1, VM_ERR_STACK_EMPTY);                                         \
  register word *RESTRICT a = &vm->stack.data[vm->stack.len - 1];              \
  *a = op(*a);                                                                 \
  DISPATCH()

#define try(expr)                                                              \
  {                                                                            \
    register VmResult res = expr;                                              \
    if (res != VM_OK) [[clang::unlikely]] {                                    \
      return (res);                                                            \
    }                                                                          \
  }                                                                            \
  static_assert(true, "")

#define push_n(n)                                                              \
  {                                                                            \
    debug("PUSH%d\n", n);                                                      \
    ensure_code(n);                                                            \
    try(push_stack(&vm->stack, word_from_bytes((n), vm->code.ptr + vm->pc)));  \
    vm->pc += (n);                                                             \
    DISPATCH()                                                                 \
  }

#define read_n(n)                                                              \
  {                                                                            \
    debug("READ%d\n", (n));                                                    \
    ensure_stack(1, VM_ERR_STACK_EMPTY);                                       \
    register size_t *RESTRICT top = &vm->stack.data[vm->stack.len - 1];        \
    register word ptr = *top;                                                  \
    ensure_memory(ptr, (n));                                                   \
    register word res = word_from_bytes((n), vm->memory.data + ptr);           \
    *top = res;                                                                \
    DISPATCH()                                                                 \
  }

#define dread_n(n)                                                             \
  {                                                                            \
    debug("DREAD%d\n", (n));                                                   \
    ensure_stack(1, VM_ERR_STACK_EMPTY);                                       \
    register word *RESTRICT top = &vm->stack.data[vm->stack.len - 1];          \
    register word ptr = *top;                                                  \
    ensure_data(ptr, (n));                                                     \
    register word res = word_from_bytes((n), vm->data.ptr + ptr);              \
    *top = res;                                                                \
    DISPATCH()                                                                 \
  }

#define write_n(n)                                                             \
  {                                                                            \
    debug("WRITE%d\n", (n));                                                   \
    ensure_stack(2, VM_ERR_STACK_EMPTY);                                       \
    register word value = vm->stack.data[vm->stack.len - 1];                   \
    register size_t ptr = vm->stack.data[vm->stack.len - 2];                   \
    ensure_memory(ptr, (n));                                                   \
    write_u64((n), value, vm->memory.data + ptr);                              \
    vm->stack.len -= 2;                                                        \
    DISPATCH()                                                                 \
  }

#define try_add(res, val, n, err)                                              \
  {                                                                            \
    if (ckd_add((res), (val), (n))) [[clang::unlikely]] {                      \
      return (err);                                                            \
    };                                                                         \
  }                                                                            \
  static_assert(true, "")

typedef union u64 {
  word n;
  uint8_t bz[sizeof(word)];
} u64;

inline Fat new_fat(uint8_t const *RESTRICT ptr, size_t len) {
  return (Fat){.len = len, .ptr = ptr};
}

inline word word_from_bytes(size_t n, const uint8_t *RESTRICT arr) {
  // static_assert (n <= 8 && n > 0);
  [[clang::assume(n <= 8)]];
  [[clang::assume(n > 0)]];

  register u64 out = {0};
  memcpy(out.bz, arr, n);
  debug("n: %zu, value: %02lx\n", n, out.n);
#if BYTE_ORDER == __LITTLE_ENDIAN
  out.n = htobe64(out.n) >> (sizeof(word) * (sizeof(word) - n));
#endif
  return out.n;
}

inline void write_u64(size_t n, word value, uint8_t *RESTRICT buffer) {
  [[clang::assume(n <= 8)]];
  [[clang::assume(n > 0)]];

  register u64 out = {0};
#if BYTE_ORDER == __LITTLE_ENDIAN
  out.n = be64toh(value << (8 * (8 - n)));
#endif
  memcpy(buffer, out.bz, n);
  debug("WRITE_U64: n: %zu, value: %016lx, out: %016lx\n", n, value, out.n);
}

inline VmResult push_stack(Stack *RESTRICT stack, word value) {
  if (stack->capacity == 0) [[clang::unlikely]] {
    register word *RESTRICT ptr = (word * RESTRICT) malloc(4 * sizeof(word));
    if (ptr == NULL) [[clang::unlikely]] {
      return VM_ERR_OUT_OF_MEMORY;
    }
    stack->data = ptr;
    stack->capacity = 4;
  } else if (stack->len == stack->capacity) [[clang::unlikely]] {
    register word *RESTRICT ptr = (word * RESTRICT)
        realloc(stack->data, stack->capacity * 2 * sizeof(word));
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

inline VmResult pop_stack(Stack *RESTRICT stack, word *RESTRICT value) {
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
#pragma clang diagnostic ignored "-Wgnu-designator"
  static void *ops_table[256] = {
      [0 ... 255] = &&unknown_op,
      [OP_PUSH0] = &&_push0,
      [OP_PUSH1] = &&_push1,
      [OP_PUSH2] = &&_push2,
      [OP_PUSH3] = &&_push3,
      [OP_PUSH4] = &&_push4,
      [OP_PUSH5] = &&_push5,
      [OP_PUSH6] = &&_push6,
      [OP_PUSH7] = &&_push7,
      [OP_PUSH8] = &&_push8,
      [OP_DUP] = &&_dup,
      [OP_DUP0] = &&_dup0,
      [OP_SWAP] = &&_swap,
      [OP_SWAP0] = &&_swap0,
      [OP_POP] = &&_pop,
      [OP_ALLOC] = &&_alloc,
      [OP_WRITE1] = &&_write1,
      [OP_WRITE2] = &&_write2,
      [OP_WRITE3] = &&_write3,
      [OP_WRITE4] = &&_write4,
      [OP_WRITE5] = &&_write5,
      [OP_WRITE6] = &&_write6,
      [OP_WRITE7] = &&_write7,
      [OP_WRITE8] = &&_write8,
      [OP_READ1] = &&_read1,
      [OP_READ2] = &&_read2,
      [OP_READ3] = &&_read3,
      [OP_READ4] = &&_read4,
      [OP_READ5] = &&_read5,
      [OP_READ6] = &&_read6,
      [OP_READ7] = &&_read7,
      [OP_READ8] = &&_read8,
      [OP_DREAD1] = &&_dread1,
      [OP_DREAD2] = &&_dread2,
      [OP_DREAD3] = &&_dread3,
      [OP_DREAD4] = &&_dread4,
      [OP_DREAD5] = &&_dread5,
      [OP_DREAD6] = &&_dread6,
      [OP_DREAD7] = &&_dread7,
      [OP_DREAD8] = &&_dread8,
      [OP_DCOPY] = &&_dcopy,
      [OP_DLEN] = &&_dlen,
      [OP_ADD] = &&_add,
      [OP_SUB] = &&_sub,
      [OP_MUL] = &&_mul,
      [OP_DIV] = &&_div,
      [OP_EXP] = &&_exp,
      [OP_MOD] = &&_mod,
      [OP_EQ] = &&_eq,
      [OP_NEQ] = &&_neq,
      [OP_LT] = &&_lt,
      [OP_GT] = &&_gt,
      [OP_NOT] = &&_not,
      [OP_SHL] = &&_shl,
      [OP_SHR] = &&_shr,
      [OP_NEG] = &&_neg,
      [OP_OR] = &&_or,
      [OP_XOR] = &&_xor,
      [OP_AND] = &&_and,
      [OP_JUMP] = &&_jump,
      [OP_JNZ] = &&_jnz,
      [OP_CALL] = &&_call,
      [OP_EXIT] = &&_exit,
      [OP_TRAP] = &&_trap,
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
  // }

  DISPATCH()

_push0: {
  try(push_stack(&vm->stack, 0));
  DISPATCH()
}
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
  register word *RESTRICT idx = &vm->stack.data[vm->stack.len - 1];
  word stack_idx;
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
  word idx = 0;
  try(pop_stack(&vm->stack, &idx));
  try_add(&idx, idx, 2, VM_ERR_INVALID_STACK_VALUE);
  size_t len = vm->stack.len;
  if (len < idx) [[clang::unlikely]] {
    return (VM_ERR_INVALID_STACK_IDX);
  }
  register size_t a_idx = len - 1;
  register size_t b_idx = len - idx;
  register word a = vm->stack.data[a_idx];
  vm->stack.data[a_idx] = vm->stack.data[b_idx];
  vm->stack.data[b_idx] = a;
  DISPATCH()
}
_swap0: {
  // debug("SWAP0\n");
  ensure_stack(2, VM_ERR_STACK_EMPTY);
  register word a = vm->stack.data[vm->stack.len - 1];
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
  word value = 0;
  try(pop_stack(&vm->stack, &value));
  size_t new_size;
  try_add(&new_size, value, vm->memory.size, VM_ERR_INVALID_STACK_VALUE);
  if (new_size > vm->max_memory) [[clang::unlikely]] {
    return (VM_ERR_OUT_OF_MEMORY);
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
    return (VM_ERR_STACK_EMPTY);
  }

  register word len = vm->stack.data[vm->stack.len - 1];
  register word dst = vm->stack.data[vm->stack.len - 2];
  register word src = vm->stack.data[vm->stack.len - 3];

  debug("len: %lx, dst: %lx, src: %lx\n", len, dst, src);

  word dst_idx;
  try_add(&dst_idx, dst, len, VM_ERR_INVALID_STACK_VALUE);
  debug("dst len: %lx\n", dst_idx);

  word src_idx;
  try_add(&src_idx, src, len, VM_ERR_INVALID_STACK_VALUE);
  debug("src len: %lx\n", src_idx);

  if (vm->data.len < src_idx) [[clang::unlikely]] {
    return (VM_ERR_SEGFAULT);
  }
  if (vm->memory.size < dst_idx) [[clang::unlikely]] {
    return (VM_ERR_SEGFAULT);
  }

  vm->stack.len -= 3;

  uint64_t memory_ptr;
  try_add(&memory_ptr, (size_t)vm->memory.data, dst,
          VM_ERR_INVALID_STACK_VALUE);

  uintptr_t data_ptr;
  try_add(&data_ptr, (uintptr_t)vm->data.ptr, (uintptr_t)src,
          VM_ERR_INVALID_STACK_VALUE);

  // redo the arithmetic because otherwise clang complains
  memcpy(vm->memory.data + dst, vm->data.ptr + src, len);
  DISPATCH()
}
_dlen: {
  try(push_stack(&vm->stack, (word)vm->data.len));
  DISPATCH()
}
_add: { binop(op_add) }
_sub: { binop(op_sub) }
_mul: { binop(op_mul) }
_div: { nz_binop(op_div) }
_exp: { binop(op_expmod) }
_mod: { nz_binop(op_mod) }
_eq: { binop(op_eq) }
_neq: { binop(op_neq) }
_lt: { binop(op_lt) }
_gt: { binop(op_gt) }
_not: { unop(op_not) }
_shl: { binop(op_shl) }
_shr: { binop(op_shr) }
_neg: { unop(op_neg) }
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

  register word dst = vm->stack.data[vm->stack.len - 1];
  register word value = vm->stack.data[vm->stack.len - 2];

  vm->stack.len -= 2;

  if (value != 0) {
    vm->pc = dst;
  }
  DISPATCH()
}
_call: {
  // debug("CALL\n");

  ensure_stack(1, VM_ERR_STACK_EMPTY);

  register word *RESTRICT top = &vm->stack.data[vm->stack.len - 1];
  register word address = *top;
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

    register word len = vm->stack.data[vm->stack.len - 1];
    register word ptr = vm->stack.data[vm->stack.len - 2];
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
    return (VM_ERR_UNKNOWN_OP);
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
