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
#define debug(...) printf(__VA_ARGS__)
#else
#define debug(...)
#endif

#ifdef DO_RESTRICT
#define RESTRICT restrict
#else
#define RESTRICT
#endif

#define likely(x) __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
// #define likely(x) x
// #define unlikely(x) x

#define bail(err) return err
// #include <assert.h>
// #define bail(err) assert(err)

#define ensure_stack(n, err)                                                   \
  {                                                                            \
    if (unlikely(vm->stack.len < (n))) {                                       \
      bail(err);                                                               \
    }                                                                          \
  }

#define ensure_memory(ptr, n)                                                  \
  {                                                                            \
    uint64_t idx;                                                              \
    try_add(&idx, (ptr), (n), VM_ERR_INVALID_STACK_VALUE);                     \
    if (unlikely(vm->memory.size < idx)) {                                     \
      bail(VM_ERR_SEGFAULT);                                                   \
    }                                                                          \
  }

#define ensure_data(ptr, n)                                                    \
  {                                                                            \
    uint64_t idx;                                                              \
    try_add(&idx, (ptr), (n), VM_ERR_INVALID_STACK_VALUE);                     \
    if (unlikely(vm->data.len < idx)) {                                        \
      bail(VM_ERR_SEGFAULT);                                                   \
    }                                                                          \
  }

#define ensure_code(n)                                                         \
  {                                                                            \
    if (unlikely(vm->code.len < (vm->pc + (n)))) {                             \
      vm->cycles--;                                                            \
      bail(VM_ERR_EOF);                                                        \
    }                                                                          \
  }

#define binop(op)                                                              \
  {                                                                            \
    size_t len = vm->stack.len;                                                \
                                                                               \
    if (unlikely(len < 2)) {                                                   \
      bail(VM_ERR_STACK_EMPTY);                                                \
    }                                                                          \
                                                                               \
    uint64_t lhs = vm->stack.data[len - 2];                                    \
    uint64_t rhs = vm->stack.data[len - 1];                                    \
                                                                               \
    vm->stack.len -= 1;                                                        \
    vm->stack.data[len - 2] = op(lhs, rhs);                                    \
    DISPATCH();                                                                \
  }

#define try(expr)                                                              \
  {                                                                            \
    VmResult res = expr;                                                       \
    if (unlikely(res != VM_OK)) {                                              \
      bail(res);                                                               \
    }                                                                          \
  }

#define push_n(n)                                                              \
  {                                                                            \
    debug("PUSH%d\n", n);                                                      \
    ensure_code(n);                                                            \
    try(push_stack(&vm->stack, u64_from_bytes((n), vm->code.ptr + vm->pc)));   \
    vm->pc += (n);                                                             \
  }

#define read_n(n)                                                              \
  {                                                                            \
    debug("READ%d\n", (n));                                                    \
    ensure_stack(1, VM_ERR_STACK_EMPTY);                                       \
    size_t *top = &vm->stack.data[vm->stack.len - 1];                          \
    uint64_t ptr = *top;                                                       \
    ensure_memory(ptr, (n));                                                   \
    uint64_t res = u64_from_bytes((n), vm->memory.data + ptr);                 \
    *top = res;                                                                \
  }

#define dread_n(n)                                                             \
  {                                                                            \
    debug("DREAD%d\n", (n));                                                   \
    ensure_stack(1, VM_ERR_STACK_EMPTY);                                       \
    uint64_t *top = &vm->stack.data[vm->stack.len - 1];                        \
    uint64_t ptr = *top;                                                       \
    ensure_data(ptr, (n));                                                     \
    uint64_t res = u64_from_bytes((n), vm->data.ptr + ptr);                    \
    *top = res;                                                                \
  }

#define write_n(n)                                                             \
  {                                                                            \
    debug("WRITE%d\n", (n));                                                   \
    ensure_stack(2, VM_ERR_STACK_EMPTY);                                       \
    uint64_t value = vm->stack.data[vm->stack.len - 1];                        \
    size_t ptr = vm->stack.data[vm->stack.len - 2];                            \
    ensure_memory(ptr, (n));                                                   \
    write_u64((n), value, vm->memory.data + ptr);                              \
    vm->stack.len -= 2;                                                        \
  }

#define try_add(res, val, n, err)                                              \
  {                                                                            \
    if (unlikely(ckd_add((res), (val), (n)))) {                                \
      bail(err);                                                               \
    };                                                                         \
  }

typedef union u64 {
  uint64_t n;
  uint8_t bz[sizeof(uint64_t)];
} u64;

Fat new_fat(uint8_t const *ptr, size_t len) {
  return (Fat){.len = len, .ptr = ptr};
}

static inline uint64_t u64_from_bytes(size_t n, const uint8_t *RESTRICT arr) {
  u64 out = {0};
  memcpy(out.bz, arr, n);
  debug("n: %zu, value: %02lx\n", n, out.n);
#if BYTE_ORDER == __LITTLE_ENDIAN
  out.n = htobe64(out.n) >> (sizeof(uint64_t) * (sizeof(uint64_t) - n));
#endif
  return out.n;
}

static inline void write_u64(size_t n, uint64_t value,
                             uint8_t *RESTRICT buffer) {
  u64 out;
#if BYTE_ORDER == __LITTLE_ENDIAN
  out.n = be64toh(value << (8 * (8 - n)));
#endif
  memcpy(buffer, out.bz, n);
  debug("WRITE_U64: n: %zu, value: %016lx, out: %016lx\n", n, value, out.n);
}

static inline VmResult push_stack(Stack *RESTRICT stack, uint64_t value) {
  if (unlikely(stack->capacity == 0)) {
    uint64_t *ptr = (uint64_t *)malloc(4 * sizeof(uint64_t));
    if (unlikely(ptr == NULL)) {
      return VM_ERR_OUT_OF_MEMORY;
    }
    stack->data = ptr;
    stack->capacity = 4;
  } else if (unlikely(stack->len == stack->capacity)) {
    uint64_t *ptr = (uint64_t *)realloc(stack->data,
                                        stack->capacity * 2 * sizeof(uint64_t));
    if (unlikely(ptr == NULL)) {
      return VM_ERR_OUT_OF_MEMORY;
    }
    stack->data = ptr;
    stack->capacity *= 2;
  }

  stack->data[stack->len] = value;
  stack->len++;

  return VM_OK;
}

static inline VmResult alloc_memory(Memory *RESTRICT memory,
                                    size_t additional) {
  if (additional == 0)
    return VM_OK;
  uint8_t *ptr = (uint8_t *)realloc(memory->data, memory->size + additional);
  if (unlikely(ptr == NULL)) {
    return VM_ERR_OUT_OF_MEMORY;
  }
  memory->data = ptr;

  memset(ptr + memory->size, 0, additional);

  memory->size += additional;

  return VM_OK;
}

static inline VmResult pop_stack(Stack *stack, uint64_t *value) {
  if (unlikely(stack->len == 0)) {
    return VM_ERR_STACK_EMPTY;
  }

  stack->len--;
  *value = stack->data[stack->len];

  return VM_OK;
}

inline VmResult run_vm(Vm *vm) {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Winitializer-overrides"
  static void *ops_table[256] = {
      [0 ... 255] = &&unknown_op,
      [0x00] = &&_PUSH0,
      [0x01] = &&_PUSH1,
      [0x02] = &&_PUSH2,
      [0x03] = &&_PUSH3,
      [0x04] = &&_PUSH4,
      [0x05] = &&_PUSH5,
      [0x06] = &&_PUSH6,
      [0x07] = &&_PUSH7,
      [0x08] = &&_PUSH8,
      [0x09] = &&_DUP,
      [0x0a] = &&_DUP0,
      [0x0b] = &&_SWAP,
      [0x0c] = &&_SWAP0,
      [0x0d] = &&_POP,
      [0x20] = &&_ALLOC,
      [0x21] = &&_WRITE1,
      [0x22] = &&_WRITE2,
      [0x23] = &&_WRITE3,
      [0x24] = &&_WRITE4,
      [0x25] = &&_WRITE5,
      [0x26] = &&_WRITE6,
      [0x27] = &&_WRITE7,
      [0x28] = &&_WRITE8,
      [0x29] = &&_READ1,
      [0x2a] = &&_READ2,
      [0x2b] = &&_READ3,
      [0x2c] = &&_READ4,
      [0x2d] = &&_READ5,
      [0x2e] = &&_READ6,
      [0x2f] = &&_READ7,
      [0x30] = &&_READ8,
      [0x31] = &&_DREAD1,
      [0x32] = &&_DREAD2,
      [0x33] = &&_DREAD3,
      [0x34] = &&_DREAD4,
      [0x35] = &&_DREAD5,
      [0x36] = &&_DREAD6,
      [0x37] = &&_DREAD7,
      [0x38] = &&_DREAD8,
      [0x39] = &&_DCOPY,
      [0x3a] = &&_DLEN,
      [0x40] = &&_ADD,
      [0x41] = &&_SUB,
      [0x42] = &&_MUL,
      [0x43] = &&_DIV,
      [0x44] = &&_EXP,
      [0x45] = &&_MOD,
      [0x4a] = &&_EQ,
      [0x4b] = &&_NEQ,
      [0x4c] = &&_LT,
      [0x4d] = &&_GT,
      [0x4e] = &&_NOT,
      [0x4f] = &&_SHL,
      [0x50] = &&_SHR,
      [0x51] = &&_NEG,
      [0x52] = &&_OR,
      [0x53] = &&_XOR,
      [0x54] = &&_AND,
      [0xa0] = &&_JUMP,
      [0xa1] = &&_JNZ,
      [0xa2] = &&_CALL,
      [0xa4] = &&_EXIT,
      [0xa5] = &&_TRAP,
  };
#pragma clang diagnostic pop

  uint8_t op;

#define DISPATCH()                                                             \
  {                                                                            \
    if (vm->pc >= vm->code.len)                                                \
      return VM_OK;                                                            \
    op = vm->code.ptr[vm->pc];                                                 \
    vm->cycles++;                                                              \
    vm->pc++;                                                                  \
    goto *ops_table[op];                                                       \
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

  DISPATCH();

_PUSH0:
  try(push_stack(&vm->stack, 0));
  DISPATCH();
_PUSH1:
  push_n(1);
  DISPATCH();
_PUSH2:
  push_n(2);
  DISPATCH();
_PUSH3:
  push_n(3);
  DISPATCH();
_PUSH4:
  push_n(4);
  DISPATCH();
_PUSH5:
  push_n(5);
  DISPATCH();
_PUSH6:
  push_n(6);
  DISPATCH();
_PUSH7:
  push_n(7);
  DISPATCH();
_PUSH8:
  push_n(8);
  DISPATCH();
_DUP: {
  debug("DUP\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  uint64_t *idx = &vm->stack.data[vm->stack.len - 1];
  uint64_t stack_idx;
  try_add(&stack_idx, *idx, 2, VM_ERR_INVALID_STACK_IDX);
  debug("idx: %lu\n", *idx);
  ensure_stack(stack_idx, VM_ERR_INVALID_STACK_IDX);
  *idx = vm->stack.data[vm->stack.len - stack_idx];
  DISPATCH();
};
_DUP0: {
  // debug("DUP0\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  try(push_stack(&vm->stack, vm->stack.data[vm->stack.len - 1]));
  DISPATCH();
};
_SWAP: {
  // debug("SWAP\n");
  uint64_t idx = 0;
  try(pop_stack(&vm->stack, &idx));
  try_add(&idx, idx, 2, VM_ERR_INVALID_STACK_VALUE);
  size_t len = vm->stack.len;
  if (unlikely(len < idx)) {
    bail(VM_ERR_INVALID_STACK_IDX);
  }
  size_t a_idx = len - 1;
  size_t b_idx = len - idx;
  uint64_t a = vm->stack.data[a_idx];
  vm->stack.data[a_idx] = vm->stack.data[b_idx];
  vm->stack.data[b_idx] = a;
  DISPATCH();
};
_SWAP0: {
  // debug("SWAP0\n");
  ensure_stack(2, VM_ERR_STACK_EMPTY);
  uint64_t a = vm->stack.data[vm->stack.len - 1];
  vm->stack.data[vm->stack.len - 1] = vm->stack.data[vm->stack.len - 2];
  vm->stack.data[vm->stack.len - 2] = a;
  DISPATCH();
};
_POP: {
  // debug("POP\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  vm->stack.len--;
  DISPATCH();
};
_ALLOC: {
  // debug("ALLOC\n");
  uint64_t value = 0;
  try(pop_stack(&vm->stack, &value));
  size_t new_size;
  try_add(&new_size, value, vm->memory.size, VM_ERR_INVALID_STACK_VALUE);
  if (new_size > vm->max_memory) {
    bail(VM_ERR_OUT_OF_MEMORY);
  }
  // debug("%lu\n", value);
  try(alloc_memory(&vm->memory, value));
  DISPATCH();
};

_WRITE1:
  write_n(1);
  DISPATCH();
_WRITE2:
  write_n(2);
  DISPATCH();
_WRITE3:
  write_n(3);
  DISPATCH();
_WRITE4:
  write_n(4);
  DISPATCH();
_WRITE5:
  write_n(5);
  DISPATCH();
_WRITE6:
  write_n(6);
  DISPATCH();
_WRITE7:
  write_n(7);
  DISPATCH();
_WRITE8:
  write_n(8);
  DISPATCH();

_READ1:
  read_n(1);
  DISPATCH();
_READ2:
  read_n(2);
  DISPATCH();
_READ3:
  read_n(3);
  DISPATCH();
_READ4:
  read_n(4);
  DISPATCH();
_READ5:
  read_n(5);
  DISPATCH();
_READ6:
  read_n(6);
  DISPATCH();
_READ7:
  read_n(7);
  DISPATCH();
_READ8:
  read_n(8);
  DISPATCH();

_DREAD1:
  dread_n(1);
  DISPATCH();
_DREAD2:
  dread_n(2);
  DISPATCH();
_DREAD3:
  dread_n(3);
  DISPATCH();
_DREAD4:
  dread_n(4);
  DISPATCH();
_DREAD5:
  dread_n(5);
  DISPATCH();
_DREAD6:
  dread_n(6);
  DISPATCH();
_DREAD7:
  dread_n(7);
  DISPATCH();
_DREAD8:
  dread_n(8);
  DISPATCH();

_DCOPY: {
  // debug("DCOPY\n");
  if (unlikely(vm->stack.len < 3)) {
    bail(VM_ERR_STACK_EMPTY);
  }

  uint64_t len = vm->stack.data[vm->stack.len - 1];
  uint64_t dst = vm->stack.data[vm->stack.len - 2];
  uint64_t src = vm->stack.data[vm->stack.len - 3];

  debug("len: %lx, dst: %lx, src: %lx\n", len, dst, src);

  {
    uint64_t dst_idx;
    try_add(&dst_idx, dst, len, VM_ERR_INVALID_STACK_VALUE);
    debug("dst len: %lx\n", dst_idx);

    uint64_t src_idx;
    try_add(&src_idx, src, len, VM_ERR_INVALID_STACK_VALUE);
    debug("src len: %lx\n", src_idx);

    if (unlikely(vm->data.len < src_idx)) {
      bail(VM_ERR_SEGFAULT);
    }
    if (unlikely(vm->memory.size < dst_idx)) {
      bail(VM_ERR_SEGFAULT);
    }
  }

  vm->stack.len -= 3;

  uint64_t memory_ptr;
  try_add(&memory_ptr, (size_t)vm->memory.data, dst,
          VM_ERR_INVALID_STACK_VALUE);

  uintptr_t data_ptr;
  try_add(&data_ptr, (uintptr_t)vm->data.ptr, (uintptr_t)src, VM_ERR_INVALID_STACK_VALUE);

  memcpy(vm->memory.data + dst, vm->data.ptr + src, len);
  DISPATCH();
};
_DLEN: {
  try(push_stack(&vm->stack, vm->data.len));
  DISPATCH();
};
_ADD:
  binop(op_add);
_SUB:
  binop(op_sub);
_MUL:
  binop(op_mul);
_DIV: {
  size_t len = vm->stack.len;
  if (unlikely(len < 2)) {
    bail(VM_ERR_STACK_EMPTY);
  }
  uint64_t lhs = vm->stack.data[len - 2];
  uint64_t rhs = vm->stack.data[len - 1];
  vm->stack.len -= 1;
  if (unlikely(rhs == 0)) {
    bail(VM_ERR_DIVIDE_BY_ZERO);
  }
  vm->stack.data[len - 2] = op_div(lhs, rhs);
  DISPATCH();
};
_EXP:
  binop(op_expmod);
_MOD: {
  size_t len = vm->stack.len;
  if (unlikely(len < 2)) {
    bail(VM_ERR_STACK_EMPTY);
  }
  uint64_t lhs = vm->stack.data[len - 2];
  uint64_t rhs = vm->stack.data[len - 1];
  vm->stack.len -= 1;
  if (unlikely(rhs == 0)) {
    bail(VM_ERR_DIVIDE_BY_ZERO);
  }
  vm->stack.data[len - 2] = op_mod(lhs, rhs);
  DISPATCH();
};
_EQ:
  binop(op_eq);
_NEQ:
  binop(op_neq);
_LT:
  binop(op_lt);
_GT:
  debug("GT\n");
  binop(op_gt);
_NOT: {
  // debug("NOT\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  uint64_t *a = &vm->stack.data[vm->stack.len - 1];
  *a = op_not(*a);
  DISPATCH();
};
_SHL:
  binop(op_shl);
_SHR:
  binop(op_shr);
_NEG: {
  // debug("NEG\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  uint64_t *a = &vm->stack.data[vm->stack.len - 1];
  *a = op_neg(*a);
  DISPATCH();
};
_OR:
  binop(op_or);
_XOR:
  binop(op_xor);
_AND:
  binop(op_and);
_JUMP: {
  // debug("JUMP\n");
  try(pop_stack(&vm->stack, &vm->pc));
  DISPATCH();
};
_JNZ: {
  // debug("JNZ\n");

  ensure_stack(2, VM_ERR_STACK_EMPTY);

  uint64_t dst = vm->stack.data[vm->stack.len - 1];
  uint64_t value = vm->stack.data[vm->stack.len - 2];

  vm->stack.len -= 2;

  if (value != 0) {
    vm->pc = dst;
  }
  DISPATCH();
};
_CALL: {
  // debug("CALL\n");

  ensure_stack(1, VM_ERR_STACK_EMPTY);

  uint64_t *top = &vm->stack.data[vm->stack.len - 1];
  uint64_t address = *top;
  *top = vm->pc;
  vm->pc = address;
  DISPATCH();
};
_EXIT: {
  // debug("EXIT\n");
  ensure_stack(2, VM_ERR_STACK_EMPTY);

  // debug("memory: ");
  // for (int i; i < vm->memory.size; i++) {
  //   // debug("%02x", vm->memory.data[i]);
  // }
  // debug("\n");

  uint64_t len = vm->stack.data[vm->stack.len - 1];
  uint64_t ptr = vm->stack.data[vm->stack.len - 2];
  vm->stack.len -= 2;

  size_t out_ptr;
  try_add(&out_ptr, ptr, len, VM_ERR_INVALID_STACK_VALUE);
  if (out_ptr > vm->memory.size)
    return VM_ERR_SEGFAULT;
  vm->out.exit = new_fat(vm->memory.data + ptr, len);
  return VM_STEP_RESULT_EXIT;
};
_TRAP: {
  debug("TRAP\n");
  ensure_stack(1, VM_ERR_STACK_EMPTY);
  vm->stack.len--;
  vm->out.trap = vm->stack.data[vm->stack.len];
  return VM_STEP_RESULT_TRAP;
};
unknown_op:
  // DISPATCH() eagerly increments the cycle count
  vm->cycles--;
  debug("unknown op %02x\n", op);
  bail(VM_ERR_UNKNOWN_OP);

  return VM_OK;
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

void drop_vm(Vm *vm) {
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
