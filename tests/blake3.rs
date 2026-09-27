use std::fs;

use chumsky::Parser;
use palinka::{
    VmT,
    cefre::{
        CheckCtx, Ctx,
        parse::grammar,
        pass::{ConstEval, ConstProp, DeadCodeRemoval, DefInline, LoopUnroll, MergeAlloc, Pass},
    },
    ffi,
};
use serde::{Deserialize, Serialize};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn make_test_input(len: usize) -> Vec<u8> {
    (0..).map(|i| (i % 251) as u8).take(len).collect()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Cases {
    pub _comment: String,
    pub key: String,
    pub context_string: String,
    pub cases: Vec<Case>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Case {
    pub input_len: usize,
    pub hash: String,
    pub keyed_hash: String,
    pub derive_key: String,
}

fn load_blake3_vectors() -> Cases {
    serde_json::from_str(&fs::read_to_string(".blake3-vectors/test_vectors.json").unwrap()).unwrap()
}

#[test]
fn blake3_vectors() {
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .try_init();

    let raw = fs::read_to_string("tests/blake3.cfr").unwrap();

    let ast = grammar().block.parse(&raw).unwrap();

    let mut ctx = CheckCtx::new("root");
    ctx.check(&ast).unwrap();
    let ast = DefInline::new().run(&ctx, ast);

    let mut ctx = CheckCtx::new("root");
    ctx.check(&ast).unwrap();
    let ast = DefInline::new().run(&ctx, ast);

    let mut ast = ast;

    for _ in 1..=3 {
        let mut ctx = CheckCtx::new("root");
        ast = ctx.check_with(&ast, &mut LoopUnroll).unwrap();

        for i in 1.. {
            let mut ctx = CheckCtx::new("root");
            let new_ast = ctx.check_with(&ast, &mut ConstProp).unwrap();

            let mut ctx = CheckCtx::new("root");
            ctx.check(&new_ast).unwrap();
            let new_ast = DefInline::new().run(&ctx, new_ast);

            let mut ctx = CheckCtx::new("root");
            let new_ast = ctx.check_with(&new_ast, &mut ConstEval::new()).unwrap();

            let mut ctx = CheckCtx::new("root");
            let new_ast = ctx.check_with(&new_ast, &mut DeadCodeRemoval).unwrap();

            let mut ctx = CheckCtx::new("root");
            let new_ast = ctx.check_with(&new_ast, &mut MergeAlloc).unwrap();

            if new_ast == ast {
                info!("ran const prop/eval loop {i} times");
                break;
            } else {
                ast = new_ast;
            }
        }
    }

    let mut ctx = Ctx::new_root();

    ctx.compile(&ast).unwrap();

    let obj = ctx.into_object();

    let asm = obj.assemble();

    for case in load_blake3_vectors().cases {
        let input = make_test_input(case.input_len);
        let expected = const_hex::decode(case.hash).unwrap();

        let mut vm = ffi::Vm::new(asm.clone(), input, usize::MAX);

        let res = vm.run().try_into_exit().unwrap();

        assert_eq!(
            res,
            &expected[..32],
            "[{}] failed: {}",
            case.input_len,
            const_hex::encode(&res)
        );

        println!("[{}] pass", case.input_len);
    }
}
