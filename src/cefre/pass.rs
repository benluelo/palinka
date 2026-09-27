use crate::cefre::{CheckCtx, ast::Block};

mod const_eval;
mod const_prop;
mod dead_code_removal;
mod def_inline;
mod loop_unroll;
mod merge_alloc;
mod normalize;

pub use const_eval::ConstEval;
pub use const_prop::ConstProp;
pub use dead_code_removal::DeadCodeRemoval;
pub use def_inline::DefInline;
pub use loop_unroll::LoopUnroll;
pub use merge_alloc::MergeAlloc;
pub use normalize::Normalize;

pub trait Pass {
    fn run<'a>(&mut self, check_ctx: &CheckCtx<'a>, block: Block<'a>) -> Block<'a>;
}

#[cfg(test)]
fn init() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .try_init();
}

#[cfg(test)]
mod tests {
    use chumsky::Parser;

    use super::*;
    use crate::cefre::parse::{grammar, print_ast};

    #[test]
    fn normalize() {
        let raw = r"
# Function to load a 64-bit value using the little-endian (LE) convention.
# On a LE platform, this could be greatly simplified using a cast.
def load64(at) -> u {
  # i <- 7
  # loop :a {
  #   u <- shl(u, 8)
  #   u <- or(u, read1(add(at, i)))
  #   if eq(i, 0) {
  #     break :a
  #   }
  #   i <- sub(i, 1)
  # }

  u <- or(u, read1(add(at, 7)))
  u <- shl(u, 8)
  u <- or(u, read1(add(at, 6)))
  u <- shl(u, 8)
  u <- or(u, read1(add(at, 5)))
  u <- shl(u, 8)
  u <- or(u, read1(add(at, 4)))
  u <- shl(u, 8)
  u <- or(u, read1(add(at, 3)))
  u <- shl(u, 8)
  u <- or(u, read1(add(at, 2)))
  u <- shl(u, 8)
  u <- or(u, read1(add(at, 1)))
  u <- shl(u, 8)
  u <- or(u, read1(add(at, 0)))
}

# Function to store a 64-bit value using the little-endian (LE) convention.
# On a LE platform, this could be greatly simplified using a cast.
def store64(at, u) {
  # i <- 0
  # loop :a {
  #   write1(add(at, i), u)
  #   u <- shr(u, 8)
  #   if eq(i, 7) {
  #     break :a
  #   }
  #   i <- add(i, 1)
  # }

  write1(add(at, 0), u)
  u <- shr(u, 8)
  write1(add(at, 1), u)
  u <- shr(u, 8)
  write1(add(at, 2), u)
  u <- shr(u, 8)
  write1(add(at, 3), u)
  u <- shr(u, 8)
  write1(add(at, 4), u)
  u <- shr(u, 8)
  write1(add(at, 5), u)
  u <- shr(u, 8)
  write1(add(at, 6), u)
  u <- shr(u, 8)
  write1(add(at, 7), u)
}

# load/store works correctly
# alloc(8)
# store64(0, 0x0123456789abcdef)
# store64(0, load64(0))
# trap(load64(0))

# # Function to XOR into a 64-bit value using the little-endian (LE) convention.
# # On a LE platform, this could be greatly simplified using a cast.
def xor64(at, u) {
  # i <- 0
  # loop :a {
  #   write1(add(at, i), xor(u, read1(add(at, i))))
  #   u <- shr(u, 8)
  #   if eq(i, 7) {
  #     break :a
  #   }
  #   i <- add(i, 1)
  # }

  write1(add(at, 0), xor(u, read1(add(at, 0))))
  u <- shr(u, 8)
  write1(add(at, 1), xor(u, read1(add(at, 1))))
  u <- shr(u, 8)
  write1(add(at, 2), xor(u, read1(add(at, 2))))
  u <- shr(u, 8)
  write1(add(at, 3), xor(u, read1(add(at, 3))))
  u <- shr(u, 8)
  write1(add(at, 4), xor(u, read1(add(at, 4))))
  u <- shr(u, 8)
  write1(add(at, 5), xor(u, read1(add(at, 5))))
  u <- shr(u, 8)
  write1(add(at, 6), xor(u, read1(add(at, 6))))
  u <- shr(u, 8)
  write1(add(at, 7), xor(u, read1(add(at, 7))))
}

def ROTL64(a, offset) -> n {
  # (a << offset) ^ (a >> (64-offset))
  n <- xor(shl(a, offset), shr(a, sub(64, offset)))
}

def i(x, y) -> n {
  # (x+5y)
  n <- add(x, mul(5, y))
}

def readLane(x, y) -> lane {
  lane <- load64(mul(8, i(x, y)))
}

def writeLane(x, y, lane) {
  store64(mul(8, i(x, y)), lane)
}

def XORLane(x, y, lane) {
  xor64(mul(8, i(x, y)), lane)
}

# alloc state (1600 bits)
alloc(200)

# alloc blocks
block_count <- add(1, div(dlen(), 136))
alloc(mul(136, block_count))

# temp array[5] space (C during theta step, temp during chi step)
temp_arr_ptr <- add(200, mul(136, block_count))
alloc(40)

# iota round constants
# TODO: Make these stack variables once I add arrays
round_constants_ptr <- add(40, temp_arr_ptr)
alloc(mul(24, 8))
write8(add(round_constants_ptr, mul(8, 0)), 0x0000000000000001)
write8(add(round_constants_ptr, mul(8, 1)), 0x0000000000008082)
write8(add(round_constants_ptr, mul(8, 2)), 0x800000000000808A)
write8(add(round_constants_ptr, mul(8, 3)), 0x8000000080008000)
write8(add(round_constants_ptr, mul(8, 4)), 0x000000000000808B)
write8(add(round_constants_ptr, mul(8, 5)), 0x0000000080000001)
write8(add(round_constants_ptr, mul(8, 6)), 0x8000000080008081)
write8(add(round_constants_ptr, mul(8, 7)), 0x8000000000008009)
write8(add(round_constants_ptr, mul(8, 8)), 0x000000000000008A)
write8(add(round_constants_ptr, mul(8, 9)), 0x0000000000000088)
write8(add(round_constants_ptr, mul(8, 10)), 0x0000000080008009)
write8(add(round_constants_ptr, mul(8, 11)), 0x000000008000000A)
write8(add(round_constants_ptr, mul(8, 12)), 0x000000008000808B)
write8(add(round_constants_ptr, mul(8, 13)), 0x800000000000008B)
write8(add(round_constants_ptr, mul(8, 14)), 0x8000000000008089)
write8(add(round_constants_ptr, mul(8, 15)), 0x8000000000008003)
write8(add(round_constants_ptr, mul(8, 16)), 0x8000000000008002)
write8(add(round_constants_ptr, mul(8, 17)), 0x8000000000000080)
write8(add(round_constants_ptr, mul(8, 18)), 0x000000000000800A)
write8(add(round_constants_ptr, mul(8, 19)), 0x800000008000000A)
write8(add(round_constants_ptr, mul(8, 20)), 0x8000000080008081)
write8(add(round_constants_ptr, mul(8, 21)), 0x8000000000008080)
write8(add(round_constants_ptr, mul(8, 22)), 0x0000000080000001)
write8(add(round_constants_ptr, mul(8, 23)), 0x8000000080008008)

# copy data to memory
dcopy(0, 200, dlen())

# bits(input) + bits(0b01) + bits(0b1) + bits(0b1) + bits(n) + bits(0b1)
# write padding byte at the end of the copied input
write1(add(dlen(), 200), 0x06)
# write trailing 1 bit to end of blocks
write1(add(mul(136, block_count), 199), or(read1(add(mul(136, block_count), 199)), 0x80))

block_number <- 0
loop :blocks {
  if lt(block_number, block_count) {
    i <- 0
    loop :a {
      # 17 words per block
      if lt(i, 17) {
        state_word <- read8(mul(i, 8))
        # start of blocks (200) + block offset + word offset
        block_word <- read8(add(200, add(mul(block_number, 136), mul(i, 8))))
        write8(
          mul(i, 8),
          xor(state_word, block_word)
        )
        i <- add(i, 1)
      } else {
        break :a
      }
    }

    round <- 0
    x <- 0
    y <- 0
    j <- 0
    t <- 0
    loop :permute {
      if lt(round, 24) {
        # === θ step (see [Keccak Reference, Section 2.3.2]) ===

        C <- temp_arr_ptr

        # Compute the parity of the columns
        x <- 0
        loop :theta_parity {
          if lt(x, 5) {
            # write to C[x]
            write8(
              add(C, mul(x, 8)),
              xor(xor(xor(xor(readLane(x, 0), readLane(x, 1)), readLane(x, 2)), readLane(x, 3)), readLane(x, 4))
            )
            x <- add(x, 1)
          } else {
            break :theta_parity
          }
        }

        x <- 0
        D <- 0
        loop :theta_effect_x {
          if lt(x, 5) {
            # Compute the θ effect for a given column
            D <- xor(
              # C[(x+4)%5]
              read8(add(C, mul(mod(add(x, 4), 5), 8))),
              # C[(x+1)%5]
              ROTL64(read8(add(C, mul(mod(add(x, 1), 5), 8))), 1)
            )

            # Add the θ effect to the whole column
            y <- 0
            loop :theta_effect_y {
              if lt(y, 5) {
                XORLane(x, y, D)
                y <- add(y, 1)
              } else {
                break :theta_effect_y
              }
              # 1591140754
              # 1591810066
            }

            x <- add(x, 1)
          } else {
            break :theta_effect_x
          }
        }

        # === ρ and π steps (see [Keccak Reference, Sections 2.3.3 and 2.3.4]) ===
        current <- 0
        temp <- 0
        # Start at coordinates (1 0)
        x <- 1
        y <- 0
        current <- readLane(x, y)
        # Iterate over ((0 1)(2 3))^t * (1 0) for 0 ≤ t ≤ 23
        t <- 0
        r <- 0
        Y <- 0
        loop :rho_and_pi {
          if lt(t, 24) {
            # Compute the rotation constant r = (t+1)(t+2)/2
            r <- mod(div(mul(add(t, 1), add(t, 2)), 2), 64)
            # # Compute ((0 1)(2 3)) * (x y)
            Y <- mod(add(mul(2, x), mul(3, y)), 5)
            x <- y
            y <- Y
            # Swap current and state(x,y), and rotate
            temp <- readLane(x, y)
            writeLane(x, y, ROTL64(current, r))
            current <- temp
            t <- add(t, 1)
          } else {
            break :rho_and_pi
          }
        }

        # === χ step (see [Keccak Reference, Section 2.3.1]) ===
        y <- 0
        loop :chi {
          if lt(y, 5) {
            # Take a copy of the plane
            x <- 0
            loop :chi_copy_plane {
              if lt(x, 5) {
                write8(
                  add(temp_arr_ptr, mul(x, 8)),
                  readLane(x, y)
                )
                x <- add(x, 1)
              } else {
                break :chi_copy_plane
              }
            }
            # Compute χ on the plane
            x <- 0
            loop :chi_compute {
              if lt(x, 5) {
                writeLane(
                  x, y,
                  xor(
                    read8(add(temp_arr_ptr, mul(x, 8))),
                    and(
                      neg(read8(add(temp_arr_ptr, mul(mod(add(x, 1), 5), 8)))),
                          read8(add(temp_arr_ptr, mul(mod(add(x, 2), 5), 8)))
                    )
                  )
                )
                x <- add(x, 1)
              } else {
                break :chi_compute
              }
            }
            y <- add(y, 1)
          } else {
            break :chi
          }
        }

        # === ι step (see [Keccak Reference, Section 2.3.5]) ===
        XORLane(0, 0, read8(add(mul(8, round), round_constants_ptr)))
        round <- add(round, 1)
      } else {
        break :permute
      }
    }
  } else {
    break :blocks
  }
  block_number <- add(block_number, 1)
}

exit(0, 32)
        ";

        let ast = grammar().block.parse(raw).unwrap();

        let mut ctx = CheckCtx::new("");
        ctx.check(&ast).unwrap();

        let ast = Normalize::new().run(&ctx, ast);

        println!("{}", print_ast(&ast));
    }
}
