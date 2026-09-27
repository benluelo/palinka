use std::fmt::Write;

use chumsky::{
    prelude::*,
    text::{
        ascii::{self, keyword},
        digits, newline,
    },
};

use crate::cefre::ast::{
    Assignment, Block, Break, BuiltinOrDef, Continue, Def, Else, Expr, Ident, If, Label, Loop,
    Statement, Val,
};

#[cfg(test)]
mod tests;

macro_rules! Parser {
    ($a:lifetime, $ty:ty $(; [$($trait:ident),*])?) => {
        impl Parser<$a, &$a str, $ty, extra::Err<Rich<$a, char>>> + Clone $($(+ $trait)*)?
    };
}

pub struct Grammar<
    Comment,
    Val,
    Expr,
    Assignment,
    Statement,
    Loop,
    If,
    Label,
    Break,
    Continue,
    Block,
    Def,
> {
    pub comment: Comment,
    pub val: Val,
    pub expr: Expr,
    pub assignment: Assignment,
    pub statement: Statement,
    pub loop_: Loop,
    pub if_: If,
    pub label: Label,
    pub break_: Break,
    pub continue_: Continue,
    pub block: Block,
    pub def: Def,
}

#[allow(clippy::type_complexity)] // bitch
pub fn grammar<'a>() -> Grammar<
    Parser!['a, ()],
    Parser!['a, Val],
    Parser!['a, Expr<'a>],
    Parser!['a, Assignment<'a>],
    Parser!['a, Statement<'a>],
    Parser!['a, Loop<'a>],
    Parser!['a, If<'a>],
    Parser!['a, Label<'a>],
    Parser!['a, Break<'a>],
    Parser!['a, Continue<'a>],
    Parser!['a, Block<'a>],
    Parser!['a, Def<'a>],
> {
    fn ident<'a>() -> Parser!['a, Ident<'a>] {
        ascii::ident().spanned().padded().map(Ident::new_spanned)
    }

    fn comment<'a>() -> Parser!['a, ()] {
        just("#")
            .then(any().and_is(newline().not()).repeated())
            .padded()
            .ignored()
            .labelled("comment")
    }

    fn val<'a>() -> Parser!['a, Val] {
        choice((
            just("0x")
                .ignore_then(digits(16).to_slice().map(|s| u64::from_str_radix(s, 16)).unwrapped()),
            just("0b")
                .ignore_then(digits(2).to_slice().map(|s| u64::from_str_radix(s, 2)).unwrapped()),
            digits(10).to_slice().from_str().unwrapped(),
        ))
        .spanned()
        .padded_by(comment().repeated())
        .padded()
        .map(Val::new_spanned)
    }

    fn expr<'a>() -> Parser!['a, Expr<'a>] {
        recursive(|tree| {
            choice((
                just("...")
                    .or_not()
                    .then(
                        ident().map(BuiltinOrDef::from).spanned().then(
                            tree.padded()
                                .padded_by(comment().repeated())
                                .separated_by(just(','))
                                .collect::<Vec<_>>()
                                .delimited_by(just('('), just(')')),
                        ),
                    )
                    .map(|(spread, (f, args))| Expr::Call { spread: spread.is_some(), f, args }),
                ident().map(Expr::Var),
                val().map(Expr::Val),
            ))
        })
        .padded()
        .padded_by(comment().repeated())
        .labelled("arg expression")
    }

    fn assignment<'a>() -> Parser!['a, Assignment<'a>] {
        just("const")
            .padded()
            .or_not()
            .then(
                ident()
                    .padded()
                    .separated_by(just(','))
                    .collect::<Vec<_>>()
                    .then_ignore(just("<-").padded())
                    .then(expr()),
            )
            .map(|(const_, (vars, expr))| Assignment { const_: const_.is_some(), vars, expr })
            .padded_by(comment().repeated())
            .labelled("assignment")
    }

    fn ident_list<'a>() -> Parser!['a, Vec<Ident<'a>>] {
        ident()
            .padded()
            .padded_by(comment().repeated())
            .separated_by(just(','))
            .allow_trailing()
            .collect()
    }

    fn non_empty_ident_list<'a>() -> Parser!['a, Vec<Ident<'a>>] {
        ident().padded().separated_by(just(',')).at_least(1).allow_trailing().collect()
    }

    let mut statement = Recursive::declare();
    let mut loop_ = Recursive::declare();
    let mut if_ = Recursive::declare();
    let mut block = Recursive::declare();
    let mut def = Recursive::declare();

    loop_.define(
        keyword("loop")
            .padded()
            .ignore_then(label())
            .then(block.clone().delimited_by(just('{').padded(), just('}').padded()))
            .map(|(label, block)| Loop { label, block })
            .labelled("loop"),
    );

    if_.define(
        keyword("if")
            .padded()
            .ignore_then(expr().padded())
            .then(block.clone().delimited_by(just('{').padded(), just('}').padded()))
            .then(
                keyword("else")
                    .ignore_then(choice((
                        block
                            .clone()
                            .delimited_by(just('{').padded(), just('}').padded())
                            .map(|block| Else::Tail { block }),
                        if_.clone().spanned().map(|if_| Else::ElseIf { if_: Box::new(if_) }),
                    )))
                    .or_not(),
            )
            .map(|((cond, block), else_)| If { cond, block, else_ })
            .labelled("if"),
    );

    statement.define(
        choice((
            def.clone().map(Statement::Def),
            loop_.clone().map(Statement::Loop),
            break_().map(Statement::Break),
            continue_().map(Statement::Continue),
            if_.clone().map(Statement::If),
            assignment().map(Statement::Assignment),
            expr().map(Statement::Expr),
        ))
        .padded()
        .padded_by(comment().repeated())
        .labelled("statement"),
    );

    block.define(
        statement
            .clone()
            // .then_ignore(newline())
            .padded_by(comment().repeated())
            .repeated()
            .collect()
            .spanned()
            .map(Block::new_spanned)
            .padded_by(comment().repeated()),
    );

    def.define(
        keyword("def")
            .padded()
            .ignore_then(ident().padded())
            .then(
                ident_list()
                    .delimited_by(
                        just('(').padded().padded_by(comment().repeated()),
                        just(')').padded().padded_by(comment().repeated()),
                    )
                    .padded(),
            )
            .then(
                just("->")
                    .padded()
                    .ignore_then(non_empty_ident_list().padded().padded_by(comment().repeated()))
                    .or_not(),
            )
            .then(block.clone().delimited_by(just('{').padded(), just('}').padded()))
            .map(|(((ident, args), ret), body)| Def {
                ident,
                args,
                rets: ret.unwrap_or_default(),
                body,
            })
            .padded()
            .padded_by(comment().repeated())
            .labelled("def"),
    );

    fn label<'a>() -> Parser!['a, Label<'a>] {
        just(":")
            .ignore_then(ascii::ident().spanned().map(Label::new_spanned))
            .padded()
            .labelled("label")
    }

    fn break_<'a>() -> Parser!['a, Break<'a>] {
        keyword("break").padded().ignore_then(label()).map(Break).labelled("break")
    }

    fn continue_<'a>() -> Parser!['a, Continue<'a>] {
        keyword("continue").padded().ignore_then(label()).map(Continue).labelled("continue")
    }

    Grammar {
        comment: comment(),
        val: val(),
        expr: expr(),
        assignment: assignment(),
        statement,
        loop_,
        if_,
        label: label(),
        break_: break_(),
        continue_: continue_(),
        block,
        def,
    }
}

pub fn print_ast(ast: &Block<'_>) -> String {
    fn go(depth: usize, out: &mut String, ast: &Block<'_>) {
        for s in ast.iter() {
            match s {
                Statement::Expr(expr) => {
                    out.push_str(&"  ".repeat(depth));
                    go_expr(out, expr);
                }
                Statement::Loop(Loop { label, block }) => {
                    out.push_str(&"  ".repeat(depth));
                    out.push_str("loop :");
                    write!(out, "{}", label).unwrap();
                    out.push_str(" {\n");
                    go(depth + 1, out, block);
                    out.write_str(&"  ".repeat(depth)).unwrap();
                    out.push('}');
                }
                Statement::Break(label) => {
                    out.push_str(&"  ".repeat(depth));
                    out.push_str("break :");
                    write!(out, "{}", label.0).unwrap();
                }
                Statement::Continue(label) => {
                    out.push_str(&"  ".repeat(depth));
                    out.push_str("continue :");
                    write!(out, "{}", label.0).unwrap();
                }
                Statement::If(if_) => {
                    fn go_if(depth: usize, out: &mut String, If { cond, block, else_ }: &If) {
                        out.push_str("if ");
                        go_expr(out, cond);
                        out.push_str(" {\n");
                        go(depth + 1, out, block);
                        out.write_str(&"  ".repeat(depth)).unwrap();
                        out.push('}');
                        match else_ {
                            Some(Else::ElseIf { if_ }) => {
                                write!(out, " else ").unwrap();
                                go_if(depth, out, if_);
                            }
                            Some(Else::Tail { block }) => {
                                out.push_str(" else {\n");
                                go(depth + 1, out, block);
                                out.write_str(&"  ".repeat(depth)).unwrap();
                                out.push('}');
                            }
                            None => {}
                        }
                    }

                    out.push_str(&"  ".repeat(depth));

                    go_if(depth, out, if_);
                }
                Statement::Assignment(Assignment { const_, vars, expr }) => {
                    out.push_str(&"  ".repeat(depth));
                    if *const_ {
                        write!(out, "global ").unwrap();
                    }
                    for (i, var) in vars.iter().enumerate() {
                        write!(out, "{var}").unwrap();
                        if i == vars.len() {
                            out.push_str(", ");
                        }
                    }
                    out.push_str(" <- ");
                    go_expr(out, expr);
                }
                Statement::Def(Def { ident, args, rets, body }) => {
                    out.push_str(&"  ".repeat(depth));
                    out.push_str("def ");
                    write!(out, "{ident}").unwrap();
                    out.push('(');
                    for (i, arg) in args.iter().enumerate() {
                        write!(out, "{arg}").unwrap();
                        if (i + 1) < args.len() {
                            out.push_str(", ");
                        }
                    }
                    out.push(')');
                    for (i, ret) in rets.iter().enumerate() {
                        if i == 0 {
                            out.push_str(" -> ");
                        }
                        write!(out, "{ret}").unwrap();
                        if (i + 1) < rets.len() {
                            out.push_str(", ");
                        }
                    }
                    out.push_str(" {\n");
                    go(depth + 1, out, body);
                    out.write_str(&"  ".repeat(depth)).unwrap();
                    out.push('}');
                }
            }
            out.push('\n');
        }
    }

    fn go_expr(out: &mut String, expr: &Expr<'_>) {
        match expr {
            Expr::Val(val) => out.write_fmt(format_args!("{val}")).unwrap(),
            Expr::Var(var) => write!(out, "{var}").unwrap(),
            Expr::Call { spread, f, args: exprs } => {
                if *spread {
                    write!(out, "...").unwrap();
                }
                write!(out, "{}", f.inner).unwrap();
                out.push('(');
                for (i, expr) in exprs.iter().enumerate() {
                    go_expr(out, expr);
                    if i < exprs.len() - 1 {
                        out.push_str(", ");
                    }
                }
                out.push(')');
            }
        }
    }

    let mut out = String::new();

    go(0, &mut out, ast);

    out
}
