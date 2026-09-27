use chumsky::span::Spanned;
use tracing::trace;

use crate::cefre::{
    CheckCtx, Visitor,
    ast::{Block, Builtin, BuiltinOrDef, Expr, Statement, Val},
};

pub struct MergeAlloc;

impl Visitor for MergeAlloc {
    fn visit_block<'a>(&mut self, _ctx: &CheckCtx, block: &Block<'a>) -> Option<Block<'a>> {
        let mut new_block = vec![];
        let mut changed = false;

        for statement in block.clone() {
            match (statement, new_block.last_mut()) {
                (
                    Statement::Expr(Expr::Call {
                        spread: false,
                        f: Spanned { inner: BuiltinOrDef::Builtin(Builtin::Alloc), .. },
                        args,
                    }),
                    Some(Statement::Expr(Expr::Call {
                        spread: false,
                        f: Spanned { inner: BuiltinOrDef::Builtin(Builtin::Alloc), .. },
                        args: prev_args,
                    })),
                ) if matches!(&*args, [Expr::Val(_)]) && matches!(&**prev_args, [Expr::Val(_)]) => {
                    let prev = prev_args[0].as_val().unwrap().value();
                    let curr = args[0].as_val().unwrap().value();
                    trace!("merging alloc of {curr} with previous alloc of {prev}");
                    *prev_args[0].as_val_mut().unwrap() = Val::new(prev + curr);
                    changed = true;
                }
                (s, _) => new_block.push(s),
            }
        }

        changed.then_some(Block::new(new_block, block.span()))
    }
}

#[cfg(test)]
mod tests {
    use chumsky::Parser;

    use super::*;
    use crate::cefre::{
        parse::{grammar, print_ast},
        pass::init,
    };

    #[test]
    fn basic() {
        init();

        let raw = r#"
alloc(10)
alloc(15)
"#;

        let ast = grammar().block.parse(raw).unwrap();

        let mut ctx = CheckCtx::new("");

        let ast = ctx.check_with(&ast, &mut MergeAlloc).unwrap();

        println!("{}", print_ast(&ast));
    }
}
