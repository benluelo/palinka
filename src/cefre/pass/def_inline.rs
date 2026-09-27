use chumsky::span::Spanned;
use tracing::trace;

use crate::cefre::{
    CheckCtx,
    ast::{Assignment, Block, BuiltinOrDef, Def, Else, Expr, Ident, If, Loop, Statement},
    pass::Pass,
};

pub struct DefInline {}

impl DefInline {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {}
    }

    fn run_on_if_statement<'a>(&mut self, check_ctx: &CheckCtx<'a>, if_: If<'a>) -> If<'a> {
        If {
            cond: def_inline(check_ctx, if_.cond),
            block: self.run(check_ctx, if_.block),
            else_: match if_.else_ {
                Some(else_) => match else_ {
                    Else::ElseIf { if_ } => Some(Else::ElseIf {
                        if_: Box::new(Spanned {
                            inner: self.run_on_if_statement(check_ctx, if_.inner),
                            span: if_.span,
                        }),
                    }),
                    Else::Tail { block } => Some(Else::Tail { block: self.run(check_ctx, block) }),
                },
                None => None,
            },
        }
    }
}

impl Pass for DefInline {
    fn run<'a>(&mut self, check_ctx: &CheckCtx<'a>, block: Block<'a>) -> Block<'a> {
        let mut new_block = vec![];
        let span = block.span();

        for statement in block {
            let new_statement = match statement {
                Statement::Expr(expr) => Statement::Expr(def_inline(check_ctx, expr)),
                Statement::Loop(Loop { label, block }) => {
                    Statement::Loop(Loop { label, block: self.run(check_ctx, block) })
                }
                Statement::If(if_) => Statement::If(self.run_on_if_statement(check_ctx, if_)),
                Statement::Assignment(Assignment { const_, vars, expr }) => {
                    Statement::Assignment(Assignment {
                        const_,
                        vars,
                        expr: def_inline(check_ctx, expr),
                    })
                }
                Statement::Def(Def { ident, args, rets, body }) => {
                    Statement::Def(Def { ident, args, rets, body: self.run(check_ctx, body) })
                }
                _ => statement,
            };

            new_block.push(new_statement);
        }

        Block::new(new_block, span)
    }
}

fn def_inline<'a>(check_ctx: &CheckCtx<'a>, expr: Expr<'a>) -> Expr<'a> {
    match expr {
        Expr::Val(val) => Expr::Val(val),
        Expr::Var(var) => Expr::Var(var),
        Expr::Call { spread, f, args } => {
            let inline_args = |f, args: Vec<_>| Expr::Call {
                spread,
                f,
                args: args.into_iter().map(|arg| def_inline(check_ctx, arg)).collect(),
            };

            if let BuiltinOrDef::Def(f_) = &*f {
                let def = check_ctx.get_def(f_).unwrap();
                if def.rets.len() == 1
                    && let [Statement::Assignment(assignment)] = def.body.statements()
                    && assignment.vars.len() == 1
                    && assignment.vars[0] == def.rets[0]
                {
                    trace!("inlining");
                    let mut a = assignment.expr.clone();

                    inline_def_args(&mut a, &args, &def.args);

                    a
                } else if def.rets.is_empty()
                    && let [Statement::Expr(expr)] = def.body.statements()
                {
                    trace!("inlining");
                    let mut expr = expr.clone();

                    inline_def_args(&mut expr, &args, &def.args);

                    expr
                // } else if def.rets.is_empty() && args.iter().all(|a|
                // a.is_val()) {     trace!("inlining");
                //     let mut expr = expr.clone();

                //     inline_def_args(&mut expr, &args, &def.args);

                //     expr
                } else {
                    inline_args(f, args)
                }
            } else {
                inline_args(f, args)
            }
        }
    }
}

fn inline_def_args<'a>(expr: &mut Expr<'a>, params: &[Expr<'a>], args: &[Ident<'a>]) {
    match expr {
        Expr::Val(_) => {}
        Expr::Var(ident) => {
            if let Some(idx) = args.iter().position(|n| n == ident) {
                *expr = params[idx].clone();
            }
        }
        Expr::Call { spread: _, f: _, args: call_args } => {
            for ca in call_args {
                inline_def_args(ca, params, args)
            }
        }
    }
}
