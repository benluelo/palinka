use tracing::info;

use crate::cefre::{CheckCtx, VarValue, Visitor, ast::Expr};

pub struct ConstProp;

impl Visitor for ConstProp {
    fn visit_expr<'a>(&mut self, ctx: &CheckCtx, expr: &Expr<'a>) -> Option<Expr<'a>> {
        let mut expr = expr.clone();
        let changed = visit_expr(ctx, &mut expr);
        if changed { Some(expr) } else { None }
    }
}

fn visit_expr(ctx: &CheckCtx<'_>, expr: &mut Expr<'_>) -> bool {
    match expr {
        Expr::Val(_) => false,
        Expr::Var(ident) => {
            if let VarValue::Const(val) = ctx.var_value(ident) {
                info!("'{ident}' has const value {val}, inlining");
                *expr = Expr::Val(*val);
                true
            } else {
                false
            }
        }
        // Expr::Call {
        //     // TODO: Handle spread
        //     spread: false,
        //     f:
        //         Spanned {
        //             inner: BuiltinOrDef::Def(def),
        //             span,
        //         },
        //     args,
        // } if args.iter().all(|a| a.is_val()) => {
        //     let def = ctx.get_def(def).unwrap();
        //     if def.rets.len() <= 1 {
        //         false
        //     } else {
        //         // try to const eval the fn in a loop to make it inlinable
        //     }
        // }
        Expr::Call { args, .. } => {
            let mut changed = false;
            for arg in args {
                changed |= visit_expr(ctx, arg);
            }
            changed
        }
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
a <- 1
b <- a
"#;

        let ast = grammar().block.parse(raw).unwrap();

        let mut ctx = CheckCtx::new("");

        let ast = ctx.check_with(&ast, &mut ConstProp).unwrap();

        println!("{}", print_ast(&ast));
    }

    #[test]
    fn broken() {
        init();

        let raw = r#"
n <- 0

if 0 {
  n <- 1
}

a <- n
        "#;

        let ast = grammar().block.parse(raw).unwrap();

        let mut ctx = CheckCtx::new("");

        let ast = ctx.check_with(&ast, &mut ConstProp).unwrap();

        println!("{}", print_ast(&ast));
    }
}
