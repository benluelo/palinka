# Pálinka

A very basic 64-bit, big-endian, stack-based VM, with a custom assembly language and low-level programming language, `cefre`, that compiles to it.

The assembly is largely feature-complete, aside from some missing opcodes that I haven't implemented yet (mainly bitwise operators).

The `cefre` MIR is also nearly feature complete:

- [x] Basic expressions
- [x] Control flow (if/else-if/else)
- [x] Loops with labelled break/continue
- [x] Function definitions
  - [x] Multiple return values and the spread operator to use them inline in one expression
- [x] Access to all VM functionality (currently missing all the read/write/dread widths as I haven't gotten around to it yet)
- [ ] Inline assembly and/or naked functions

The goal with `cefre` is to use it as a target for a higher level programming language in the future, to allow for an easier target than using the assembly directly. A work-in-progress design doc for this language can be found in [./DESIGN.md](./DESIGN.md).

A tree-sitter grammar for `cefre` can be found at [./tree-sitter-cefre](./tree-sitter-cefre).

There are currently 4 implementations (all available in the `palinka` binary via the `-i` flag): two in rust (the canonical implementation and one using tail calls), one in zig, and one in C. Currently, the C implementation is the fastest; the functionality is described well in more detail [here](computed-goto).

## Wtf is Pálinka?

Pálinka is a Hungarian fruit spirit (similar to brandy), and cefre is the fruit mash that is fermented and then distilled into pálinka.

## Performance

A basic mini benchmark can be run with `nix run .#mini-benchmark`.

Current results, on an M5 MacBook Pro:

```
Running rust...
time: 0.896774409
total cycles: 958145849
output: d239b78c b239ce12 85fdfb06 5e78f8d5 e51246c0 23871c2f 76b8e3bb 581a9623

real    0m0.899s
user    0m0.897s
sys     0m0.002s

Running rust-tc...
time: 1.110165928
total cycles: 958145849
output: d239b78c b239ce12 85fdfb06 5e78f8d5 e51246c0 23871c2f 76b8e3bb 581a9623

real    0m1.111s
user    0m1.110s
sys     0m0.001s

Running zig...
time: 1.558818126
total cycles: 958145849
output: d239b78c b239ce12 85fdfb06 5e78f8d5 e51246c0 23871c2f 76b8e3bb 581a9623

real    0m1.560s
user    0m1.559s
sys     0m0.001s

Running c...
time: 0.751701267
total cycles: 958145849
output: d239b78c b239ce12 85fdfb06 5e78f8d5 e51246c0 23871c2f 76b8e3bb 581a9623

real    0m0.752s
user    0m0.752s
sys     0m0.000s
```

## AI Disclosure

Absolutely no AI was used in the making of this project, and it never will. Every line of code, every bug, every fix, every line of documentation (although strikingly lacking), was written by me. AI contributions are not welcome.

[computed-goto]: https://eli.thegreenplace.net/2012/07/12/computed-goto-for-efficient-dispatch-tables
