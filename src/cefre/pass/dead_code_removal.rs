use tracing::{instrument, trace, trace_span};

use crate::cefre::{
    CheckCtx, VarValue, Visitor,
    ast::{Assignment, Block, Builtin, BuiltinOrDef, Else, Expr, Ident, If, Statement},
};

pub struct DeadCodeRemoval;

impl DeadCodeRemoval {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {}
    }
}

impl Visitor for DeadCodeRemoval {
    fn visit_block<'a>(&mut self, ctx: &CheckCtx, block: &Block<'a>) -> Option<Block<'a>> {
        let mut new_block = vec![];
        let mut removed_dead_code = false;

        for (idx, statement) in block.clone().into_iter().enumerate() {
            match statement {
                Statement::Expr(expr) => {
                    if is_pure(&expr) {
                        // drop this expr
                        removed_dead_code = true;
                    } else {
                        new_block.push(Statement::Expr(expr))
                    }
                }
                Statement::Loop(loop_) => match loop_.block.statements() {
                    [] => removed_dead_code = true,
                    // immediate break out of a loop means that loop never loops, can drop the whole
                    // loop
                    // TODO: Probably we can keep anything *before* the top level break and inline
                    // it outside of the loop, dropping the rest of the loop contents
                    [Statement::Break(break_), ..] if loop_.label == break_.0 => {
                        removed_dead_code = true;
                    }
                    _ => new_block.push(Statement::Loop(loop_)),
                },
                Statement::Break(_) => new_block.push(statement),
                Statement::Continue(_) => new_block.push(statement),
                // TODO: Drop empty if blocks with pure conditions
                Statement::If(if_) => {
                    // TODO: Handle more complex if statements
                    if if_.else_.is_some() {
                        new_block.push(Statement::If(if_));
                    } else {
                        match if_.cond {
                            Expr::Val(val) => {
                                if val.value() == 0 {
                                    // drop completely, unreachable block
                                } else {
                                    // trivially true condition, inline the block
                                    new_block.extend(if_.block);
                                }
                                removed_dead_code = true;
                            }
                            _ => {
                                new_block.push(Statement::If(if_));
                            }
                        }
                    }
                }
                Statement::Assignment(assignment) => {
                    removed_dead_code |=
                        drop_dead_assignment(ctx, block, &mut new_block, idx, assignment);
                }
                Statement::Def(def) => {
                    if def_is_referenced(&def.ident, &block.statements()[(idx + 1)..]) {
                        new_block.push(Statement::Def(def))
                    } else {
                        // drop the def since it's never used
                        removed_dead_code = true;
                    }
                }
            }
        }

        removed_dead_code.then_some(Block::new(new_block, 0..0))
    }
}

#[instrument(skip_all, fields(%assignment, %idx))]
fn drop_dead_assignment<'a>(
    ctx: &CheckCtx,
    block: &Block<'a>,
    new_block: &mut Vec<Statement<'a>>,
    idx: usize,
    assignment: Assignment<'a>,
) -> bool {
    trace_span!("checking assignment {assignment}");

    // only drop pure assignments
    if let [var] = &*assignment.vars
        && is_pure(&assignment.expr)
    {
        if ctx.has_var(var) {
            match (ctx.var_value(var), &assignment.expr) {
                (VarValue::Const(c_val), Expr::Val(e_val)) if c_val == e_val => true,
                _ => {
                    new_block.push(Statement::Assignment(assignment));
                    false
                }
            }
        } else {
            let maybe_next_assignment_idx =
                next_assignment_idx(var, &block.statements()[(idx + 1)..]);

            trace!("next_assignment_idx: {maybe_next_assignment_idx:?}");

            if let Some(statements) = match maybe_next_assignment_idx {
                Some(next_assignment_idx) => {
                    block.statements().get((idx + 1)..=((idx + 1) + next_assignment_idx))
                }
                None => block.statements().get((idx + 1)..),
            } && !var_is_referenced(var, statements)
            {
                trace!("dropping {assignment}");
                true
                // drop
            } else {
                new_block.push(Statement::Assignment(assignment));
                false
            }
        }
    } else {
        new_block.push(Statement::Assignment(assignment));
        false
    }
}

fn next_assignment_idx<'a: 'b, 'b>(
    var: &Ident<'_>,
    block: impl IntoIterator<Item = &'b Statement<'a>>,
) -> Option<usize> {
    block.into_iter().enumerate().find_map(|(idx, s)| match s {
        Statement::Expr(_) => None,
        Statement::Loop(_) => None,
        Statement::Break(_) => None,
        Statement::Continue(_) => None,
        Statement::If(_) => None,
        Statement::Assignment(assignment) => assignment.vars.contains(var).then_some(idx),
        Statement::Def(_) => None,
    })
}

fn var_is_referenced<'a: 'b, 'b>(
    var: &Ident<'a>,
    block: impl IntoIterator<Item = &'b Statement<'a>>,
) -> bool {
    fn go_expr(var: &Ident<'_>, expr: &Expr<'_>) -> bool {
        match expr {
            Expr::Val(_) => false,
            Expr::Var(ident) => ident == var,
            Expr::Call { args, .. } => args.iter().any(|arg| go_expr(var, arg)),
        }
    }

    block.into_iter().any(|s| match &s {
        Statement::Loop(loop_) => var_is_referenced(var, &loop_.block),
        Statement::If(if_) => {
            fn go_if(var: &Ident<'_>, if_: &If<'_>) -> bool {
                go_expr(var, &if_.cond)
                    || var_is_referenced(var, &if_.block)
                    || if_.else_.as_ref().is_some_and(|e| match e {
                        Else::ElseIf { if_ } => go_if(var, if_),
                        Else::Tail { block } => var_is_referenced(var, block),
                    })
            }
            go_if(var, if_)
        }
        Statement::Assignment(assignment) => {
            /* assignment.vars.contains(var) || */
            go_expr(var, &assignment.expr)
        }
        Statement::Expr(expr) => go_expr(var, expr),
        Statement::Break(_) => false,
        Statement::Continue(_) => false,
        Statement::Def(_) => false,
    })
}

fn def_is_referenced<'a: 'b, 'b>(
    def_ident: &Ident<'a>,
    block: impl IntoIterator<Item = &'b Statement<'a>>,
) -> bool {
    fn go_expr(def: &Ident<'_>, expr: &Expr<'_>) -> bool {
        match expr {
            Expr::Val(_) => false,
            Expr::Var(ident) => ident == def,
            Expr::Call { f, args, .. } => {
                matches!(&**f, BuiltinOrDef::Def(f_def) if f_def == def)
                    || args.iter().any(|arg| go_expr(def, arg))
            }
        }
    }

    block.into_iter().any(|s| match &s {
        Statement::Loop(loop_) => def_is_referenced(def_ident, &loop_.block),
        Statement::If(if_) => {
            fn go_if(var: &Ident<'_>, if_: &If<'_>) -> bool {
                go_expr(var, &if_.cond)
                    || def_is_referenced(var, &if_.block)
                    || if_.else_.as_ref().is_some_and(|e| match e {
                        Else::ElseIf { if_ } => go_if(var, if_),
                        Else::Tail { block } => def_is_referenced(var, block),
                    })
            }
            go_if(def_ident, if_)
        }
        Statement::Assignment(assignment) => {
            /* assignment.vars.contains(var) || */
            go_expr(def_ident, &assignment.expr)
        }
        Statement::Expr(expr) => go_expr(def_ident, expr),
        Statement::Break(_) => false,
        Statement::Continue(_) => false,
        Statement::Def(def) => def_is_referenced(def_ident, &def.body),
    })
}

fn is_pure(expr: &Expr<'_>) -> bool {
    match expr {
        Expr::Val(_) => true,
        Expr::Var(_) => true,
        Expr::Call { spread: _, f, args } => match &**f {
            BuiltinOrDef::Builtin(builtin) => match builtin {
                Builtin::Add
                | Builtin::Sub
                | Builtin::Mul
                | Builtin::Div
                | Builtin::Exp
                | Builtin::Mod
                | Builtin::Eq
                | Builtin::Lt
                | Builtin::Gt
                | Builtin::Shl
                | Builtin::Shr
                | Builtin::Or
                | Builtin::Xor
                | Builtin::And
                | Builtin::Not
                | Builtin::Neg
                | Builtin::Dlen => args.iter().all(is_pure),
                _ => false,
            },
            // TODO: Peek into defs
            BuiltinOrDef::Def(_) => false,
        },
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
    fn test() {
        init();

        let _raw = include_str!("../../../tests/sha3-256.cfr");
        let raw = r#"
        def xor64(at, u) {}
        def store64(at, u) {}
        def load64(at) -> u {}
        loop :a {
          current <- 0
          r <- 0
          Y <- 0
          r <- 1
          Y <- 2
          x <- 0
          y <- 2
          temp <- load64(80)
          store64(80, xor(shl(current, 1), shr(current, 63)))
          current <- temp
          r <- 3
          Y <- 1
          x <- 2
          y <- 1
          temp <- load64(56)
          store64(56, xor(shl(current, 3), shr(current, 61)))
          current <- temp
          r <- 6
          Y <- 2
          x <- 1
          y <- 2
          temp <- load64(88)
          store64(88, xor(shl(current, 6), shr(current, 58)))
          current <- temp
        }
        "#;

        let ast = grammar().block.parse(raw).unwrap();

        let mut ctx = CheckCtx::new("");

        let ast = ctx.check_with(&ast, &mut DeadCodeRemoval).unwrap();
        let ast = ctx.check_with(&ast, &mut DeadCodeRemoval).unwrap();

        println!("{}", print_ast(&ast));
    }
}
