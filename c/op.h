#include <stdint.h>

enum Op : uint8_t {
  OP_PUSH0 = 0x00,
  OP_PUSH1 = 0x01,
  OP_PUSH2 = 0x02,
  OP_PUSH3 = 0x03,
  OP_PUSH4 = 0x04,
  OP_PUSH5 = 0x05,
  OP_PUSH6 = 0x06,
  OP_PUSH7 = 0x07,
  OP_PUSH8 = 0x08,
  OP_DUP = 0x09,
  OP_DUP0 = 0x0a,
  OP_SWAP = 0x0b,
  OP_SWAP0 = 0x0c,
  OP_POP = 0x0d,
  OP_ALLOC = 0x20,
  OP_WRITE1 = 0x21,
  OP_WRITE2 = 0x22,
  OP_WRITE3 = 0x23,
  OP_WRITE4 = 0x24,
  OP_WRITE5 = 0x25,
  OP_WRITE6 = 0x26,
  OP_WRITE7 = 0x27,
  OP_WRITE8 = 0x28,
  OP_READ1 = 0x29,
  OP_READ2 = 0x2a,
  OP_READ3 = 0x2b,
  OP_READ4 = 0x2c,
  OP_READ5 = 0x2d,
  OP_READ6 = 0x2e,
  OP_READ7 = 0x2f,
  OP_READ8 = 0x30,
  OP_DREAD1 = 0x31,
  OP_DREAD2 = 0x32,
  OP_DREAD3 = 0x33,
  OP_DREAD4 = 0x34,
  OP_DREAD5 = 0x35,
  OP_DREAD6 = 0x36,
  OP_DREAD7 = 0x37,
  OP_DREAD8 = 0x38,
  OP_DCOPY = 0x39,
  OP_DLEN = 0x3a,
  OP_ADD = 0x40,
  OP_SUB = 0x41,
  OP_MUL = 0x42,
  OP_DIV = 0x43,
  OP_EXP = 0x44,
  OP_MOD = 0x45,
  OP_EQ = 0x4a,
  OP_NEQ = 0x4b,
  OP_LT = 0x4c,
  OP_GT = 0x4d,
  OP_NOT = 0x4e,
  OP_SHL = 0x4f,
  OP_SHR = 0x50,
  OP_NEG = 0x51,
  OP_OR = 0x52,
  OP_XOR = 0x53,
  OP_AND = 0x54,
  OP_JUMP = 0xa0,
  OP_JNZ = 0xa1,
  OP_CALL = 0xa2,
  OP_EXIT = 0xa4,
  OP_TRAP = 0xa5,
};

// typedef uint64_t __attribute__((overflow_behavior(wrap))) word;

typedef uint64_t word;

inline word op_add(word a, word b) { return a + b; }

inline word op_sub(word a, word b) { return a - b; }

inline word op_mul(word a, word b) { return a * b; }

inline word op_div(word a, word b) {
  [[clang::assume(b != 0)]];
  return a / b;
}

inline word op_not(word a) { return a == 0; }

inline word op_gt(word a, word b) { return a > b; }

inline word op_lt(word a, word b) { return a < b; }

inline word op_neq(word a, word b) { return a != b; }

inline word op_eq(word a, word b) { return a == b; }

inline word op_mod(word a, word b) {
  [[clang::assume(b != 0)]];
  return a % b;
}

inline word op_and(word a, word b) { return a & b; }

inline word op_xor(word a, word b) { return a ^ b; }

inline word op_or(word a, word b) { return a | b; }

inline word op_neg(word a) { return ~a; }

inline word op_expmod(word a, word b) {
  if (b == 0) {
    return 1;
  }

  __uint128_t acc = 1;
  __uint128_t base = (__uint128_t)a;
  uint64_t exp = b;

  for (;;) {
    if ((exp & 1) == 1) {
      acc = (acc * base) % 0xFFFFFFFFFFFFFFFF;
      // since exp!=0, finally the exp must be 1.
      if (exp == 1) {
        return (word)acc;
      }
    }
    exp >>= 1;
    base = (base * base) % 0xFFFFFFFFFFFFFFFF;
  }
}

inline word op_shr(word a, word shift) {
  if (shift >= 64)
    return 0;
  return a >> shift;
}

inline word op_shl(word a, word shift) {
  if (shift >= 64)
    return 0;
  return a << shift;
}
