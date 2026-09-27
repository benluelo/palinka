use chumsky::span::Spanned;

use crate::cefre::{
    CheckCtx,
    ast::{Assignment, Block, Def, Else, Expr, Ident, If, Loop, Statement},
    pass::Pass,
};

pub struct Normalize {
    counter: u32,
}

impl Normalize {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { counter: 0 }
    }

    fn next_var(&mut self) -> Ident<'static> {
        let id = self.counter;
        self.counter += 1;
        Ident::new(format!("t{id}"))
    }

    fn normalize_expr<'a>(&mut self, expr: Expr<'a>) -> (Vec<Statement<'a>>, Expr<'a>) {
        match expr {
            Expr::Val(val) => (vec![], Expr::Val(val)),
            Expr::Var(var) => (vec![], Expr::Var(var)),
            Expr::Call { spread, f, args } => {
                if args.iter().any(|a| matches!(a, Expr::Call { .. })) {
                    let mut statements = vec![];
                    let mut new_args = vec![];
                    for arg in args {
                        match arg {
                            expr @ (Expr::Var(_) | Expr::Val(_)) => new_args.push(expr),
                            arg @ Expr::Call { .. } => {
                                let (s, expr) = self.normalize_expr(arg);
                                statements.extend(s);
                                let next_var = self.next_var();
                                new_args.push(Expr::Var(next_var.clone()));
                                statements.push(Statement::Assignment(Assignment {
                                    const_: false,
                                    vars: vec![next_var],
                                    expr,
                                }));
                            }
                        }
                    }
                    (statements, Expr::Call { spread, f, args: new_args })
                } else {
                    (vec![], Expr::Call { spread, f, args })
                }
            }
        }
    }

    fn normalize_if<'a>(
        &mut self,
        check_ctx: &CheckCtx<'a>,
        If { cond, block, else_ }: If<'a>,
    ) -> (Vec<Statement<'a>>, If<'a>) {
        let mut new_block = vec![];
        let (statements, cond_expr) = self.normalize_expr(cond);
        new_block.extend(statements);
        let if_ = If {
            cond: cond_expr,
            block: self.run(check_ctx, block),
            // TODO: Run on else blocks
            else_: match else_ {
                Some(Else::ElseIf { if_ }) => {
                    let (statements, new_if) = self.normalize_if(check_ctx, if_.inner);
                    new_block.extend(statements);
                    Some(Else::ElseIf { if_: Box::new(Spanned { inner: new_if, span: if_.span }) })
                }
                Some(Else::Tail { block }) => {
                    Some(Else::Tail { block: self.run(check_ctx, block) })
                }
                None => None,
            },
        };

        (new_block, if_)
    }
}

impl Pass for Normalize {
    fn run<'a>(&mut self, check_ctx: &CheckCtx<'a>, block: Block<'a>) -> Block<'a> {
        let mut new_block = vec![];
        let span = block.span();

        for statement in block {
            match statement {
                Statement::Expr(expr) => {
                    let (statements, expr) = self.normalize_expr(expr);
                    new_block.extend(statements);
                    new_block.push(Statement::Expr(expr));
                }
                Statement::Loop(Loop { label, block }) => new_block
                    .push(Statement::Loop(Loop { label, block: self.run(check_ctx, block) })),
                Statement::If(if_) => {
                    let (statements, if_) = self.normalize_if(check_ctx, if_);
                    new_block.extend(statements);
                    new_block.push(Statement::If(if_));
                }
                Statement::Assignment(Assignment { const_, vars, expr }) => {
                    let (statements, expr) = self.normalize_expr(expr);
                    new_block.extend(statements);
                    new_block.push(Statement::Assignment(Assignment { const_, vars, expr }));
                }
                Statement::Def(Def { ident, args, rets, body }) => {
                    new_block.push(Statement::Def(Def {
                        ident,
                        args,
                        rets,
                        body: self.run(check_ctx, body),
                    }))
                }
                _ => new_block.push(statement),
            };
        }

        Block::new(new_block, span)
    }
}
