use std::hint::black_box;

use criterion::{
    BatchSize, BenchmarkGroup, Criterion, criterion_group, criterion_main, measurement::WallTime,
};
use palinka::{Op, Vm};
use rand::RngExt;

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("ops");
    group.sample_size(1000);
    group.nresamples(1_000_000);

    op_bench(&mut group, "add", Op::ADD);
    op_bench(&mut group, "sub", Op::SUB);
    op_bench(&mut group, "mul", Op::MUL);
    op_bench(&mut group, "exp", Op::EXP);
    // op_bench(&mut group, "mod", Op::MOD);
    op_bench(&mut group, "eq", Op::EQ);
    op_bench(&mut group, "neq", Op::NEQ);
    op_bench(&mut group, "lt", Op::LT);
    op_bench(&mut group, "gt", Op::GT);
    op_bench(&mut group, "shl", Op::SHL);
    op_bench(&mut group, "shr", Op::SHR);
    op_bench(&mut group, "and", Op::AND);
    op_bench(&mut group, "or", Op::OR);
    op_bench(&mut group, "xor", Op::XOR);

    group.bench_function("div", |b| {
        b.iter_batched(
            || Vm {
                code: (0..10_000).flat_map(|_| Op::DIV.to_bytes()).collect(),
                data: vec![],
                stack: (0..10_001)
                    // .map(|_| rng.random_range(1..u64::MAX))
                    .map(|_| 1)
                    .collect(),
                memory: vec![],
                hook: (),
                pc: 0,
                max_memory: usize::MAX,
            },
            |vm| black_box(black_box(vm).run_raw()).unwrap(),
            BatchSize::SmallInput,
        );
    });

    group.bench_function("mod", |b| {
        b.iter_batched(
            || Vm {
                code: (0..10_000).flat_map(|_| Op::MOD.to_bytes()).collect(),
                data: vec![],
                stack: (2..10_003)
                    // .rev()
                    // .map(|_| rng.random_range(1..u64::MAX))
                    // .map(|_| 2)
                    // .chain([1, 2])
                    .collect(),
                memory: vec![],
                hook: (),
                pc: 0,
                max_memory: usize::MAX,
            },
            |vm| black_box(black_box(vm).run_raw()).unwrap(),
            BatchSize::SmallInput,
        );
    });

    // group.bench_function("exp", |b| {
    //     let mut rng = rand::rng();

    //     b.iter_batched(
    //         || Vm {
    //             code: (0..10_000).flat_map(|_| Op::EXP.to_bytes()).collect(),
    //             data: vec![],
    //             stack: (0..10_001).map(|_| rng.random::<u32>() as u64).collect(),
    //             memory: vec![],
    //             cycles: 0,
    //         },
    //         |vm| black_box(black_box(vm).run()).unwrap(),
    //         BatchSize::SmallInput,
    //     );
    // });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

fn op_bench(group: &mut BenchmarkGroup<'_, WallTime>, name: &str, op: Op) {
    group.bench_function(name, |b| {
        let mut rng = rand::rng();

        b.iter_batched(
            || Vm {
                code: (0..10_000).flat_map(|_| op.to_bytes()).collect(),
                data: vec![],
                stack: (0..10_001).map(|_| rng.random()).collect(),
                memory: vec![],
                hook: (),
                pc: 0,
                max_memory: usize::MAX,
            },
            |vm| black_box(black_box(vm).run_raw()).unwrap(),
            BatchSize::SmallInput,
        );
    });
}
