use std::{
    cmp::Ordering,
    iter::{self},
};

use tracing::{instrument, trace};

use crate::{
    cefre::{
        CheckCtx, VarValue, Visitor,
        ast::{
            Assignment, Block, Builtin, BuiltinOrDef, Else, Expr, Ident, If, Label, Loop,
            Statement, Val,
        },
    },
    op,
};

pub struct LoopUnroll;

impl Visitor for LoopUnroll {
    // 1. ensure only one exit point
    // 2. check if that exit point is behind a conditional
    // 3. ensure that the conditional is not within any other nested loops
    // 4. ensure that the condition contains only one var
    // 5. ensure that the var is only updated in one place in the loop,
    //    unconditionally (i.e. at the top level of the loop, in code that will
    //    always execute)
    // 6. ensure that only constants (or variables with constant values) are used in
    //    the update expression
    // 7. ensure that the then body only contains a single break statement
    // 8. ensure that the break label is the same as the loop being inlined
    // 7. inline the loop body for every iteration of the loop
    #[instrument(skip_all, level = "trace", fields(%label))]
    fn visit_loop<'a>(
        &mut self,
        ctx: &CheckCtx,
        label: &Label<'a>,
        block: &Block<'a>,
    ) -> Option<Block<'a>> {
        let mut cond = None;
        let mut cond_idx = 0;

        let mut update = None;
        let mut update_idx = 0;

        trace!("attempting to unroll loop {label}");

        // look for:
        //
        // a <- 1
        // loop :a {
        //   a <- add(a, 1) # cont. expr can be before or after the cond
        //   if gt(a, 10) {
        //     break :a
        //   }
        //   a <- add(a, 1) # cont. expr can be before or after the cond
        // }

        // find the exit point
        for (idx, statement) in block.iter().enumerate() {
            match statement {
                Statement::Expr(_) => {}
                Statement::Loop(loop_) => {
                    if has_exit_point(&loop_.block, &loop_.label) {
                        trace!("loop has multiple exit points, can't unroll");
                        return None;
                    }
                }
                Statement::Break(_) => {
                    trace!(
                        "break: multiple exit points, can't unroll; break must be shielded by a condition"
                    );
                    return None;
                }
                Statement::Continue(_) => {
                    trace!("continue: multiple exit points, can't unroll");
                    return None;
                }
                // TODO: Allow for the check to be "inverted", i.e. for the exit point to be in the
                // else block
                Statement::If(If { cond: if_cond, block, else_: _ }) => {
                    if let [Statement::Break(break_label)] = block.statements()
                        && break_label.0 == *label
                    {
                        let var = cond_var(if_cond)?;

                        trace!("found valid loop check with var {var}: {if_cond}");

                        match cond {
                            Some(_) => {
                                trace!("multiple exit points, can't unroll");
                                return None;
                            }
                            None => {
                                cond = Some(if_cond);
                                cond_idx = idx;
                            }
                        }
                    }
                }
                Statement::Assignment(_) => {}
                Statement::Def(_) => todo!(),
            }
        }

        if !(cond_idx == 0 || cond_idx == block.len() - 1) {
            trace!(
                "condition must be the first or last statement in the loop, found {cond_idx}/{}",
                block.len() - 1
            );
            return None;
        }

        let Some(cond) = cond else {
            trace!("no cond found");
            return None;
        };

        let cond_var = cond_var(cond).expect("must be valid");

        // dbg!(lcond_var);

        let initial_value = match ctx.var_value(cond_var) {
            VarValue::Dyn => {
                trace!("condition variable '{cond_var}' is not constant, can't unroll");
                return None;
            }
            VarValue::Const(val) => val.value(),
        };

        // dbg!(&initial_value);

        // find the update assignment
        for (idx, statement) in block.iter().enumerate() {
            match statement {
                Statement::Expr(_) => {}
                Statement::Loop(_) => {}
                Statement::Break(_) => {}
                Statement::Continue(_) => {}
                Statement::If(_) => {}
                Statement::Assignment(assignment) => {
                    if assignment.vars.len() == 1 && &assignment.vars[0] == cond_var {
                        match update {
                            Some(_) => {
                                trace!("multiple update points, can't unroll");
                                return None;
                            }
                            None => {
                                update = Some(&assignment.expr);
                                update_idx = idx;
                            }
                        }
                    }
                }
                Statement::Def(_) => todo!(),
            }
        }

        let Some(update) = update else {
            trace!("no update expression found for var {cond_var}");
            return None;
        };

        // dbg!(&cond, &update);

        let mut val = initial_value;

        let it = iter::from_fn(|| -> _ {
            match cond_idx.cmp(&update_idx) {
                // condition is before the update
                Ordering::Less => {
                    if eval_cond(cond, cond_var, val) {
                        None
                    } else {
                        // inline the body, remove the check, and inline the var with the
                        // current value
                        let block = block.clone();
                        let new_val = eval_update_expr(update, cond_var, val);
                        // block.statements_mut()[update_idx] = Statement::Assignment(Assignment {
                        //     vars: vec![cond_var.clone()],
                        //     expr: Expr::Val(Val::new(val)),
                        // });

                        let block = block
                            .into_iter()
                            .enumerate()
                            .filter_map(|(idx, mut s)| {
                                // filter out both the condition and the update
                                if idx == cond_idx || idx == update_idx {
                                    None
                                } else {
                                    inline_var(
                                        &mut s,
                                        cond_var,
                                        if idx > update_idx { new_val } else { val },
                                    );
                                    Some(s)
                                }
                            })
                            .collect::<Vec<_>>();

                        val = new_val;

                        // dbg!(&block);

                        Some(block)
                    }
                }
                // condition is after the update
                Ordering::Greater => todo!(),
                Ordering::Equal => unreachable!(),
            }
        });

        Some(Block::new(
            it.flatten()
                .collect::<Vec<_>>()
                .into_iter()
                .chain([Statement::Assignment(Assignment {
                    const_: false,
                    vars: vec![cond_var.clone()],
                    expr: Expr::Val(Val::new(val)),
                })])
                .collect::<Vec<_>>(),
            0..0,
        ))
    }
}

fn inline_var<'a>(s: &mut Statement<'a>, var: &Ident<'a>, val: u64) {
    fn inline_var_expr<'a>(expr: &mut Expr<'a>, var: &Ident<'a>, val: u64) {
        match expr {
            Expr::Var(ident) => {
                if *ident == *var {
                    *expr = Expr::Val(Val::new(val))
                }
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    inline_var_expr(arg, var, val);
                }
            }
            _ => {}
        }
    }

    match s {
        Statement::Expr(expr) => inline_var_expr(expr, var, val),
        Statement::Loop(loop_) => {
            for s in loop_.block.statements_mut() {
                inline_var(s, var, val);
            }
        }
        Statement::If(if_) => {
            fn go_if<'a>(if_: &mut If<'a>, var: &Ident<'a>, val: u64) {
                inline_var_expr(&mut if_.cond, var, val);
                for s in if_.block.statements_mut() {
                    inline_var(s, var, val);
                }
                if let Some(else_) = if_.else_.as_mut() {
                    match else_ {
                        Else::ElseIf { if_ } => go_if(if_, var, val),
                        Else::Tail { block } => {
                            for s in block.statements_mut() {
                                inline_var(s, var, val);
                            }
                        }
                    }
                }
            }

            go_if(if_, var, val);
        }
        Statement::Assignment(assignment) => {
            if assignment.vars.contains(var) {
                panic!("???")
            }
            inline_var_expr(&mut assignment.expr, var, val)
        }
        _ => {}
    }
}

fn eval_update_expr(update: &Expr<'_>, _var: &Ident<'_>, val: u64) -> u64 {
    match update {
        Expr::Val(_) => todo!(),
        Expr::Var(_) => todo!(),
        Expr::Call { spread: _, f, args } => match &**f {
            BuiltinOrDef::Builtin(builtin) => match builtin {
                Builtin::Add => match &**args {
                    [Expr::Val(a), Expr::Var(_)] => op::add(a.value(), val),
                    [Expr::Var(_), Expr::Val(b)] => op::add(val, b.value()),
                    _ => panic!(),
                },
                _ => todo!(),
            },
            BuiltinOrDef::Def(_) => todo!(),
        },
    }
}

fn eval_cond(cond: &Expr<'_>, var: &Ident<'_>, val: u64) -> bool {
    trace!("evaluating {cond}, {var}={val}");

    match cond {
        Expr::Val(_) => todo!(),
        Expr::Var(_) => todo!(),
        Expr::Call { spread: _, f, args } => match &**f {
            BuiltinOrDef::Builtin(builtin) => match builtin {
                Builtin::Lt => {
                    (match &**args {
                        [Expr::Val(a), Expr::Var(_)] => op::lt(a.value(), val),
                        [Expr::Var(_), Expr::Val(b)] => op::lt(val, b.value()),
                        _ => panic!(),
                    }) != 0_u64
                }
                Builtin::Gt => {
                    (match &**args {
                        [Expr::Val(a), Expr::Var(_)] => op::gt(a.value(), val),
                        [Expr::Var(_), Expr::Val(b)] => op::gt(val, b.value()),
                        _ => panic!(),
                    }) != 0_u64
                }
                Builtin::Eq => {
                    (match &**args {
                        [Expr::Val(a), Expr::Var(_)] => op::eq(a.value(), val),
                        [Expr::Var(_), Expr::Val(b)] => op::eq(val, b.value()),
                        _ => panic!(),
                    }) != 0_u64
                }
                _ => todo!(),
            },
            BuiltinOrDef::Def(_) => todo!(),
        },
    }
}

fn cond_var<'a, 'b>(expr: &'b Expr<'a>) -> Option<&'b Ident<'a>> {
    match expr {
        Expr::Val(_) => todo!(),
        Expr::Var(ident) => Some(ident),
        Expr::Call { spread: _, f, args } => match **f {
            // TODO: Support more opcodes here
            BuiltinOrDef::Builtin(Builtin::Lt | Builtin::Gt | Builtin::Eq) => match &**args {
                [Expr::Val(_), Expr::Var(ident)] | [Expr::Var(ident), Expr::Val(_)] => Some(ident),
                _ => None,
            },
            _ => None,
        },
    }
}

fn has_exit_point(block: &Block, label: &Label) -> bool {
    block.iter().any(|s| match s {
        Statement::Expr(_) => false,
        Statement::Loop(Loop { label: nested_label, block }) => {
            if label == nested_label {
                // TODO: Handle nested loops with shadowed labels, for now assume it can't be
                // unrolled
                true
            } else {
                has_exit_point(block, label)
            }
        }
        Statement::Break(_) => true,
        Statement::Continue(_) => true,
        Statement::If(If { cond: _, block, else_ }) => {
            has_exit_point(block, label)
                || match else_ {
                    Some(else_) => match else_ {
                        Else::ElseIf { if_: _ } => todo!(),
                        Else::Tail { block } => has_exit_point(block, label),
                    },
                    None => false,
                }
        }
        Statement::Assignment(_) => false,
        // TODO: Handle defs defined in loops when unrolling
        Statement::Def(_) => todo!(),
    })
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
        if 1 {
          def xor64(at, u) {
            i <- 0
            loop :a {
              write1(add(at, i), xor(u, read1(add(at, i))))
              u <- shr(u, 8)
              if eq(i, 7) {
                break :a
              }
              i <- add(i, 1)
            }
          }

          y <- 0
          x <- 0
          D <- 0
          loop :theta_effect_y {
            if eq(y, 5) {
              break :theta_effect_y
            }
            xor64(mul(8, add(x, mul(5, y))), D)
            y <- add(y, 1)
          }
        }
        "#;

        let ast = grammar().block.parse(raw).unwrap();

        let mut ctx = CheckCtx::new("");

        let ast = ctx.check_with(&ast, &mut LoopUnroll).unwrap();

        println!("{}", print_ast(&ast));
    }
}
