use std::collections::BTreeMap;

use chumsky::span::Spanned;

use crate::{
    cefre::{
        CheckCtx, Visitor,
        ast::{self, Block, Builtin, BuiltinOrDef, Def, Expr, Ident, Statement, Val},
    },
    op,
};

#[non_exhaustive]
pub struct ConstEval {}

impl ConstEval {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {}
    }
}

impl Visitor for ConstEval {
    fn visit_expr<'a>(&mut self, ctx: &CheckCtx, expr: &Expr<'a>) -> Option<Expr<'a>> {
        let res = eval_to_fixed_point(ctx, expr.clone());
        if &res == expr { None } else { Some(res) }
    }
}

fn const_eval<'a>(ctx: &CheckCtx, expr: Expr<'a>) -> Expr<'a> {
    match expr {
        Expr::Val(val) => Expr::Val(val),
        Expr::Var(var) => Expr::Var(var),
        Expr::Call { spread, f, args } => {
            use BuiltinOrDef::Builtin;
            use ast::Builtin::*;

            macro_rules! p_val {
                ($v:pat) => {
                    Expr::Val($v)
                };
            }

            // macro_rules! p_var {
            //     ($v:pat) => {
            //         Expr::Var($v)
            //     };
            // }

            macro_rules! p_call {
                ($pat:pat, $args:pat $(,)?) => {
                    Expr::Call { spread: false, f: $pat, args: $args }
                };
            }

            let len = args.len();

            let binop = |ctor: ast::Builtin,
                         op_f: fn(u64, u64) -> u64,
                         f_: fn(Expr<'a>, Expr<'a>) -> Option<Expr<'a>>|
             -> Expr<'a> {
                match (const_eval(ctx, args[0].clone()), const_eval(ctx, args[1].clone())) {
                    (p_val!(l), p_val!(r)) => Expr::val(op_f(l.value(), r.value()), f.span),
                    (l, r) => f_(l.clone(), r.clone()).unwrap_or_else(|| Expr::Call {
                        spread,
                        f: Spanned { inner: ctor.into(), span: f.span },
                        args: vec![l, r],
                    }),
                }
            };

            // eval the args first (depth first)
            match (&f.inner, len) {
                (Builtin(b), n) => match (b, n) {
                    (Add, 2) => binop(Add, op::add, |l, r| match (l, r) {
                        (p_val!(val), e) | (e, p_val!(val)) if val.value() == 0 => Some(e),
                        (
                            p_val!(l),
                            p_call!(
                                f @ Spanned { inner: Builtin(Add), .. },
                                args,
                            ),
                        )
                        | (
                            p_call!(
                                f @ Spanned { inner: Builtin(Add), .. },
                                args,
                            ),
                            p_val!(l),
                        ) => {
                            if let [p_val!(r), e] | [e, p_val!(r)] = &*args {
                                Some(Expr::Call {
                                    spread: false,
                                    f,
                                    args: vec![
                                        Expr::Val(Val::new(op::add(l.value(), r.value()))),
                                        e.clone(),
                                    ],
                                })
                            } else {
                                None
                            }
                        }
                        _ => None,
                    }),
                    (Sub, 2) => binop(Sub, op::sub, |_, _| None),
                    (Mul, 2) => binop(Mul, op::mul, |_, _| None),
                    (Div, 2) => binop(Div, |a, b| op::div(a, b).unwrap(), |_, _| None),
                    (Exp, 2) => binop(Exp, op::expmod, |_, _| None),
                    (Mod, 2) => binop(Mod, |a, b| op::r#mod(a, b).unwrap(), |_, _| None),
                    (Eq, 2) => binop(Eq, op::eq, |_, _| None),
                    (Lt, 2) => binop(Lt, op::lt, |_, _| None),
                    (Gt, 2) => binop(Gt, op::gt, |_, _| None),
                    (Shl, 2) => binop(Shl, op::shl, |_, _| None),
                    (Shr, 2) => binop(Shr, op::shr, |_, _| None),
                    (Or, 2) => binop(Or, op::or, |_, _| None),
                    (Xor, 2) => binop(Xor, op::xor, |_, _| None),
                    (And, 2) => binop(And, op::and, |_, _| None),
                    (Not, 1) => match const_eval(ctx, args[0].clone()) {
                        p_val!(l) => Expr::val(op::not(l.value()), f.span),
                        e => Expr::Call {
                            spread,
                            f: Spanned { inner: Builtin(Not), span: f.span },
                            args: vec![e],
                        },
                    },
                    (Neg, 1) => match const_eval(ctx, args[0].clone()) {
                        p_val!(l) => Expr::val(op::neg(l.value()), f.span),
                        e => Expr::Call {
                            spread,
                            f: Spanned { inner: Builtin(Neg), span: f.span },
                            args: vec![e],
                        },
                    },
                    // same as default branch below
                    _ => Expr::Call {
                        spread,
                        f,
                        args: args.into_iter().map(|a| const_eval(ctx, a)).collect(),
                    },
                },
                (BuiltinOrDef::Def(def), _) => {
                    let new_args = args
                        .iter()
                        .map(|a| const_eval(ctx, a.clone()).as_val().map(|v| v.value()))
                        .collect::<Option<Vec<_>>>();
                    match new_args {
                        Some(args) => match try_eval_def(ctx, ctx.get_def(def).unwrap(), &args) {
                            Some(val) => Expr::Val(Val::new(val)),
                            None => Expr::Call {
                                spread,
                                f,
                                args: args
                                    .into_iter()
                                    .map(|a| const_eval(ctx, Expr::Val(Val::new(a))))
                                    .collect(),
                            },
                        },
                        None => Expr::Call {
                            spread,
                            f,
                            args: args.into_iter().map(|a| const_eval(ctx, a)).collect(),
                        },
                    }
                }
            }
        }
    }
}

fn is_pure(ctx: &CheckCtx, expr: &Expr<'_>) -> bool {
    use Builtin::*;

    match expr {
        Expr::Val(_) => true,
        Expr::Var(_) => true,
        Expr::Call { spread: _, f, args } => match &**f {
            BuiltinOrDef::Builtin(builtin) => match builtin {
                Add | Sub | Mul | Div | Exp | Mod | Eq | Lt | Gt | Shl | Shr | Or | Xor | And
                | Not | Neg | Dlen => args.iter().all(|arg| is_pure(ctx, arg)),
                _ => false,
            },
            BuiltinOrDef::Def(def) => block_is_pure(ctx, &ctx.get_def(def).unwrap().body),
        },
    }
}

fn block_is_pure(ctx: &CheckCtx<'_>, block: &Block<'_>) -> bool {
    block.iter().all(|s| match s {
        Statement::Expr(expr) => is_pure(ctx, expr),
        Statement::Loop(_) => false,
        Statement::Break(_) => false,
        Statement::Continue(_) => false,
        Statement::If(_) => false,
        Statement::Assignment(assignment) => is_pure(ctx, &assignment.expr),
        Statement::Def(_) => false,
    })
}

fn try_eval_def(ctx: &CheckCtx<'_>, def: &Def<'_>, params: &[u64]) -> Option<u64> {
    let [ret] = &*def.rets else {
        return None;
    };

    if def.args.len() != params.len() {
        return None;
    }

    let mut vars = def.args.iter().zip(params.iter().copied()).collect::<BTreeMap<_, _>>();

    for s in &def.body {
        match s {
            Statement::Expr(expr) => {
                if !is_pure(ctx, expr) {
                    return None;
                }
            }
            Statement::Loop(_) => return None,
            Statement::Break(_) => return None,
            Statement::Continue(_) => return None,
            Statement::If(_) => return None,
            Statement::Assignment(assignment) => {
                let [var] = &*assignment.vars else {
                    return None;
                };

                vars.insert(var, eval(ctx, &vars, &assignment.expr)?);
            }
            Statement::Def(_) => return None,
        }
    }

    Some(vars[ret])
}

fn eval(ctx: &CheckCtx, vars: &BTreeMap<&Ident<'_>, u64>, expr: &Expr<'_>) -> Option<u64> {
    match expr {
        Expr::Val(val) => Some(val.value()),
        Expr::Var(ident) => Some(vars[&ident]),
        Expr::Call { spread, f, args } => {
            if *spread {
                return None;
            }

            let new_args = args
                .iter()
                .map(|a| eval_to_fixed_point(ctx, inline_vars_into_expr(vars, a)))
                .collect();

            // match &f.inner {
            //     BuiltinOrDef::Builtin(_) => eval_to_fixed_point(
            //         ctx,
            //         Expr::Call { spread: *spread, f: f.clone(), args: new_args },
            //     )
            //     .as_val()
            //     .map(|v| v.value()),
            //     BuiltinOrDef::Def(ident) => {
            //         let def = ctx.get_def(ident).unwrap();
            //         try_eval_def(ctx, def, new_args)
            //     },
            // }

            eval_to_fixed_point(ctx, Expr::Call { spread: *spread, f: f.clone(), args: new_args })
                .as_val()
                .map(|v| v.value())
        }
    }
}

fn inline_vars_into_expr<'a>(vars: &BTreeMap<&Ident<'a>, u64>, expr: &Expr<'a>) -> Expr<'a> {
    match expr {
        Expr::Val(val) => Expr::Val(*val),
        Expr::Var(ident) => Expr::Val(Val::new(vars[ident])),
        Expr::Call { spread, f, args } => Expr::Call {
            spread: *spread,
            f: f.clone(),
            args: args.iter().map(|e| inline_vars_into_expr(vars, e)).collect(),
        },
    }
}

fn eval_to_fixed_point<'a>(ctx: &CheckCtx, mut expr: Expr<'a>) -> Expr<'a> {
    loop {
        let res = const_eval(ctx, expr.clone());
        if res == expr {
            break expr;
        } else {
            expr = res;
        }
    }
}
