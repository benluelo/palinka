#![expect(unused_variables, dead_code)]

use std::collections::BTreeMap;

use indexmap::IndexMap;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    assembler::AsmOp,
    cefre::{
        IdentifiedLabel, LabelId,
        ast::{Block, Builtin, BuiltinOrDef, Def, Else, Expr, Ident, Label, Loop, Statement},
        ssa::builder::{
            BlockId, Builder, BuiltinCall, DefId, Expr as SsaExpr, Goto, Operand, Sealed,
            Statement as SsaStatement, Terminal, VarId,
        },
    },
};

pub mod builder;
pub mod id_map;

// COPIED FROM HERE ON

#[derive(Debug)]
pub struct Ctx<'a> {
    next_scope_label_id: LabelId,
    prefix: String,
    sections: IndexMap<String, Vec<AsmOp<'a>>>,
    builder: Builder,
    fns: IndexMap<String, IndexMap<String, Vec<AsmOp<'a>>>>,
    stack_depth: usize,
    scopes: Vec<Scope<'a>>,
    current_block_id: BlockId,
}

#[derive(Debug)]
pub struct Scope<'a> {
    id: LabelId,
    label: Option<(Label<'a>, BlockId)>,
    vars: BTreeMap<Ident<'a>, VarId>,
    defs: BTreeMap<Ident<'a>, (DefId, Def<'a>)>,
}

impl<'a> Ctx<'a> {
    #[allow(clippy::new_without_default)]
    pub fn new_root() -> Self {
        Ctx::new("MAIN")
    }

    pub fn new(prefix: &str) -> Self {
        let first_scope_label_id = LabelId::new();
        let identified_label = IdentifiedLabel::new(
            Label::new(
                // TODO: Figure out a better way to do this
                format!("$ROOT/{prefix}").leak(),
            ),
            first_scope_label_id,
        );
        let first_section_id = identified_label.to_string();
        let scope = Scope {
            id: first_scope_label_id,
            label: None,
            vars: Default::default(),
            defs: Default::default(),
        };

        let mut builder = Builder::new();

        let current_block_id = builder.push_block();

        Self {
            next_scope_label_id: first_scope_label_id.increment(),
            prefix: prefix.to_owned(),
            sections: [(first_section_id.clone(), vec![])].into_iter().collect(),
            builder,
            stack_depth: 0,
            fns: Default::default(),
            scopes: [scope].into_iter().collect(),
            current_block_id,
        }
    }

    fn compile(&mut self, ast: Block<'a>) {
        for stmt in ast {
            match stmt {
                Statement::Expr(expr) => {
                    let (statements, expr) = self.normalize_expr(expr);

                    self.push_statements(statements);

                    match expr {
                        SsaExpr::Const(val) => todo!(),
                        SsaExpr::Var(var_id) => todo!(),
                        SsaExpr::Builtin(builtin_call) => {
                            self.push_statements([SsaStatement::Builtin(builtin_call)])
                        }
                    }
                }
                Statement::Loop(Loop { label, block }) => {
                    // self.push_scope(ScopeLabel::Label(IdentifiedLabel::new(label)));
                    todo!();
                }
                Statement::Break(_) => todo!(),
                Statement::Continue(_) => todo!(),
                Statement::If(if_) => {
                    // dbg!(&if_);

                    let mut current_if = Some(if_);

                    // end of the entire if statement, all blocks will eventually end up here unless
                    // they exit to another scope (break/continue)
                    let tail_block_id = self.push_block();

                    while let Some(if_) = current_if.take() {
                        let (statements, expr) = self.normalize_expr(if_.cond);

                        self.push_statements(statements);

                        let then_block_id = self.push_block();
                        let else_block_id = self.push_block();

                        self.terminate_block(
                            self.current_block_id,
                            Terminal::Jump {
                                cond: expr,
                                then: Goto { label: then_block_id, args: vec![] },
                                else_: Goto { label: else_block_id, args: vec![] },
                            },
                        );

                        self.seal_block(self.current_block_id);

                        self.current_block_id = then_block_id;
                        self.compile(if_.block);
                        self.terminate_block(
                            self.current_block_id,
                            Terminal::Goto(Goto { label: tail_block_id, args: vec![] }),
                        );
                        self.seal_block(then_block_id);

                        match if_.else_ {
                            Some(else_) => match else_ {
                                Else::ElseIf { if_: else_if } => {
                                    self.current_block_id = else_block_id;
                                    current_if = Some(else_if.inner);
                                }
                                Else::Tail { block } => {
                                    self.current_block_id = else_block_id;
                                    self.compile(block);
                                    self.terminate_block(
                                        self.current_block_id,
                                        Terminal::Goto(Goto { label: tail_block_id, args: vec![] }),
                                    );
                                    self.seal_block(self.current_block_id);
                                }
                            },
                            None => {
                                self.terminate_block(
                                    else_block_id,
                                    Terminal::Goto(Goto { label: tail_block_id, args: vec![] }),
                                );
                                self.seal_block(else_block_id);
                            }
                        }
                    }

                    self.current_block_id = tail_block_id;
                }
                Statement::Assignment(assignment) => {
                    let (statements, expr) = self.normalize_expr(assignment.expr);

                    self.push_statements(statements);

                    let (var_id, block_id) = self.get_or_init_named_var(&assignment.vars[0]);

                    self.push_statements([SsaStatement::Assignment { var_id, expr }]);
                }
                Statement::Def(def) => todo!(),
            }
        }

        // self.builder.block_mut(self.current_block_id).terminal =
        // Terminal::End;
    }

    fn push_statements(&mut self, statements: impl IntoIterator<Item = SsaStatement>) {
        self.builder.push_statements(self.current_block_id, statements)
    }

    fn push_block(&mut self) -> BlockId {
        self.builder.push_block()
    }

    fn add_predecessor(&mut self, block_id: BlockId, predecessor: BlockId) {
        let block_mut = self.builder.block_mut(block_id);

        assert!(!block_mut.is_sealed());

        block_mut.predecessors.push(predecessor);
    }

    fn terminate_block(&mut self, block_id: BlockId, terminal: Terminal) {
        match &terminal {
            Terminal::Jump { cond: _, then, else_ } => {
                self.add_predecessor(then.label, block_id);
                self.add_predecessor(else_.label, block_id);
            }
            Terminal::Goto(goto) => {
                self.add_predecessor(goto.label, block_id);
            }
            Terminal::End => todo!(),
            Terminal::None => todo!(),
        }

        self.builder.block_mut(block_id).terminal = terminal;
    }

    fn seal_block(&mut self, block_id: BlockId) {
        let Sealed::No { vars } =
            std::mem::replace(&mut self.builder.block_mut(block_id).sealed, Sealed::Yes)
        else {
            unreachable!("sealed twice?")
        };

        // need to clone to appease borrowck
        for pre_block_id in self.builder.block(block_id).predecessors.clone() {
            // TODO: Figure out how not to clone here
            let pre_block = self.builder.block(pre_block_id).clone();

            assert!(pre_block.is_sealed());

            self.builder.block_mut(block_id).args.extend(pre_block.def_vars.intersection(&vars));

            match &mut self.builder.block_mut(pre_block_id).terminal {
                Terminal::Jump { cond, then, else_ } => {
                    if then.label == block_id {
                        then.args.extend(
                            pre_block.def_vars.intersection(&vars).copied().map(Operand::Var),
                        )
                    } else {
                        // ???
                    }
                    if else_.label == block_id {
                        else_.args.extend(
                            pre_block.def_vars.intersection(&vars).copied().map(Operand::Var),
                        )
                    }
                }
                Terminal::Goto(goto) => {
                    if goto.label == block_id {
                        goto.args.extend(
                            pre_block.def_vars.intersection(&vars).copied().map(Operand::Var),
                        )
                    }
                }
                Terminal::End => todo!(),
                Terminal::None => todo!(),
            }
        }
    }

    /// Get a variable by name from the AST. If there is no variable visible
    /// from the current scope with the provided identifier, this function will
    /// return None.
    fn use_named_var(&mut self, var: &Ident<'a>) -> Option<(VarId, BlockId)> {
        self.scopes
            .iter()
            .find_map(|s| {
                s.vars.iter().find_map(|(ident, var_id)| ident.eq(var).then_some(*var_id))
            })
            .map(|var_id| {
                let var_def_block_id = self.builder.get_var_def_block(var_id);

                if var_def_block_id != self.current_block_id {
                    println!(
                        "current block: {}, var {var_id} is from block {var_def_block_id}: {:#?}",
                        self.current_block_id,
                        self.builder.block(var_def_block_id)
                    );

                    // // loop {
                    // for pre in &self.builder.block(var_def_block_id).predecessors {
                    //     let pre = self.builder.block(*pre);
                    //     dbg!(pre);
                    // }
                    // // }
                    match &mut self.builder.block_mut(self.current_block_id).sealed {
                        Sealed::Yes => todo!(),
                        Sealed::No { vars } => vars.insert(var_id),
                    };
                }

                (var_id, var_def_block_id)
            })
    }

    /// Get a variable by name from the AST, or initialize it if it is not
    /// found.
    fn get_or_init_named_var(&mut self, var: &Ident<'a>) -> (VarId, BlockId) {
        match self.use_named_var(var) {
            Some(var_id) => var_id,
            None => {
                let var_id = self.def_var();

                self.scopes.last_mut().unwrap().vars.insert(var.clone(), var_id);

                (var_id, self.current_block_id)
            }
        }
    }

    /// Get a def by name from the AST. If there is no def visible
    /// from the current scope with the provided identifier, this function will
    /// return None.
    fn get_def(&self, def: &Ident<'a>) -> Option<&(DefId, Def<'a>)> {
        self.scopes.iter().find_map(|s| {
            s.defs.iter().find_map(|(ident, def_and_id)| ident.eq(def).then_some(def_and_id))
        })
    }

    // fn push_var(&mut self, var: Ident<'a>) -> VarId {
    //     match self.get_var(&var) {
    //         Some(id) => id,
    //         None => {}
    //     }
    //     self.builder.push_var()
    // }

    fn normalize_expr(&mut self, expr: Expr<'a>) -> (Vec<SsaStatement>, SsaExpr) {
        match expr {
            Expr::Val(val) => (vec![], SsaExpr::Const(val.value())),
            Expr::Var(var) => {
                let (var_id, block_id) = self.use_named_var(&var).expect("var not found");
                (vec![], SsaExpr::Var(var_id))
            }
            Expr::Call { spread, f, args } => {
                assert!(!spread, "spread arguments not yet supported");

                let mut statements = vec![];
                let mut new_args = vec![];

                // flatten nested calls (if any) with local temporaries
                for (idx, arg) in args.into_iter().enumerate() {
                    match arg {
                        Expr::Val(val) => new_args.push(Operand::Const(val.value())),
                        Expr::Var(var) => {
                            let (var_id, block_id) =
                                self.use_named_var(&var).expect("var not found");
                            new_args.push(Operand::Var(var_id))
                        }
                        arg @ Expr::Call { .. } => {
                            let (s, expr) = self.normalize_expr(arg);
                            statements.extend(s);
                            let var_id = self.def_var();
                            new_args.push(Operand::Var(var_id));
                            statements.push(SsaStatement::Assignment { var_id, expr });
                        }
                    }
                }

                macro_rules! builtin_args {
                    ($Op:ident($($var:ident),*)) => {{
                        let [$($var),*] = new_args.try_into().unwrap();

                        // $(
                        //     let $var = match $var {
                        //         Expr::Val(val) => Operand::Const(val.value()),
                        //         Expr::Var(var) => {
                        //             let var = self.get_var(&var).expect("var not found");
                        //             Operand::Var(var)
                        //         }
                        //         Expr::Call { spread, f, args } => panic!("bug: not normalized"),
                        //     };
                        // )*

                        SsaExpr::Builtin(BuiltinCall::$Op($($var),*))
                    }};
                }

                let expr = match &*f {
                    BuiltinOrDef::Builtin(builtin) => match builtin {
                        Builtin::Add => builtin_args!(Add(a, b)),
                        Builtin::Sub => builtin_args!(Sub(a, b)),
                        Builtin::Mul => builtin_args!(Mul(a, b)),
                        Builtin::Div => builtin_args!(Div(a, b)),
                        Builtin::Exp => builtin_args!(Exp(a, b)),
                        Builtin::Mod => builtin_args!(Mod(a, b)),
                        Builtin::Eq => builtin_args!(Eq(a, b)),
                        Builtin::Lt => builtin_args!(Lt(a, b)),
                        Builtin::Gt => builtin_args!(Gt(a, b)),
                        Builtin::Shl => builtin_args!(Shl(a, b)),
                        Builtin::Shr => builtin_args!(Shr(a, b)),
                        Builtin::Or => builtin_args!(Or(a, b)),
                        Builtin::Xor => builtin_args!(Xor(a, b)),
                        Builtin::And => builtin_args!(And(a, b)),
                        Builtin::Not => builtin_args!(Not(a)),
                        Builtin::Neg => builtin_args!(Neg(a)),
                        Builtin::Dread1 => builtin_args!(Dread1(a)),
                        Builtin::Dread2 => builtin_args!(Dread2(a)),
                        Builtin::Dread3 => builtin_args!(Dread3(a)),
                        Builtin::Dread4 => builtin_args!(Dread4(a)),
                        Builtin::Dread5 => builtin_args!(Dread5(a)),
                        Builtin::Dread6 => builtin_args!(Dread6(a)),
                        Builtin::Dread7 => builtin_args!(Dread7(a)),
                        Builtin::Dread8 => builtin_args!(Dread8(a)),
                        Builtin::Dlen => builtin_args!(Dlen()),
                        Builtin::Read1 => builtin_args!(Read1(a)),
                        Builtin::Read2 => builtin_args!(Read2(a)),
                        Builtin::Read3 => builtin_args!(Read3(a)),
                        Builtin::Read4 => builtin_args!(Read4(a)),
                        Builtin::Read5 => builtin_args!(Read5(a)),
                        Builtin::Read6 => builtin_args!(Read6(a)),
                        Builtin::Read7 => builtin_args!(Read7(a)),
                        Builtin::Read8 => builtin_args!(Read8(a)),
                        Builtin::Alloc => builtin_args!(Alloc(a)),
                        Builtin::Write1 => builtin_args!(Write1(a, b)),
                        Builtin::Write2 => builtin_args!(Write2(a, b)),
                        Builtin::Write3 => builtin_args!(Write3(a, b)),
                        Builtin::Write4 => builtin_args!(Write4(a, b)),
                        Builtin::Write5 => builtin_args!(Write5(a, b)),
                        Builtin::Write6 => builtin_args!(Write6(a, b)),
                        Builtin::Write7 => builtin_args!(Write7(a, b)),
                        Builtin::Write8 => builtin_args!(Write8(a, b)),
                        Builtin::Dcopy => builtin_args!(Dcopy(a, b, c)),
                        Builtin::Exit => builtin_args!(Exit(a, b)),
                        Builtin::Trap => builtin_args!(Trap(a)),
                    },
                    // can't use a call as an expr directly, need to assign the call result to a
                    // variable and return that result as the expression
                    // TODO: how to handle multiple return values?
                    BuiltinOrDef::Def(ident) => {
                        let statement = SsaStatement::Call {
                            def_id: self.get_def(ident).unwrap().0,
                            args: new_args,
                            rets: vec![],
                        };
                        statements.push(statement);
                        todo!();
                    }
                };

                (statements, expr)
            }
        }
    }

    fn current_scope_id(&self) -> LabelId {
        self.scopes.last().unwrap().id
    }

    fn push_scope(&mut self, label: Option<(Label<'a>, BlockId)>) {
        self.scopes.push(Scope {
            label,
            vars: Default::default(),
            defs: Default::default(),
            id: self.next_scope_label_id,
        });
        self.next_scope_label_id = self.next_scope_label_id.increment();
    }

    /// Define a var in the current block.
    fn def_var(&mut self) -> VarId {
        self.builder.push_var(self.current_block_id)
    }
}

// #[derive(Debug)]
// pub struct ScopeId(u32);

// #[derive(Debug)]
// pub struct ScopeId(Vec<u32>);

// impl fmt::Display for ScopeId {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         f.write_str(
//             &self
//                 .0
//                 .iter()
//                 .map(|id| id.to_string())
//                 .collect::<Vec<_>>()
//                 .join("/"),
//         )
//     }
// }

fn init() {
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .try_init();
}

#[cfg(test)]
mod tests {
    use chumsky::Parser;

    use super::*;
    use crate::cefre::parse::{grammar, print_ast};

    #[test]
    fn compile_expr() {
        init();

        let raw = "
            var <- 2
            var2 <- 10
            if 1 {
                var <- add(var, var2)
            } else if eq(add(var2, var), 1) {
                trap(1)
            }
            ";

        let raw = "
            var <- 2
            var2 <- 10
            var <- add(mul(var, 3), sub(var2, 1))
            ";

        let raw = "
            var <- 1
            if 1 {
                if 2 {
                    var <- 2
                } else if 0 {
                    trap(2)
                }
            } else {
                trap(1)
            }
            ";

        let raw = "
            a <- 1
            if 2 {
                b <- 2
                a <- add(a, b)
            } else if 0 {
                c <- 3
                a <- add(a, c)
                trap(2)
            } else {
                d <- 3
                a <- add(a, d)
                write8(1, 1)
            }
            ";

        let ast = grammar().block.parse(raw).unwrap();

        let mut ctx = Ctx::new_root();

        println!("{}", print_ast(&ast));

        ctx.compile(ast);

        println!("{}", ctx.builder);
        println!("{:#?}", ctx.builder);
    }

    #[test]
    fn test() {
        init();

        let raw = "
            # def add_mul(a, b) -> o {
            #     o <- mul(a, add(a, b))
            # }

            three <- 3
            v <- mul(2, add(1, three))

            alloc(8)
            write8(0, v)
            exit(0, 8)
            ";

        let ast = grammar().block.parse(raw).unwrap();
        // let ast = Normalize::new().run(&CheckCtx::new(""), ast);

        let mut ctx = Ctx::new_root();

        ctx.compile(ast);

        println!("{}", ctx.builder);
    }

    // #[test]
    // fn compile_if_else_if_branch() {
    //     init();

    //     let raw = "
    //             var <- 2
    //             var2 <- 10
    //             if 1 {
    //                 var <- add(var, var2)
    //             } else {
    //                 trap(1)
    //             }
    //             ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     // dbg!(&obj);

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, None);

    //     assert_eq!(
    //         vm.stack,
    //         [
    //             12, // var
    //             10  // var2
    //         ]
    //     );
    // }

    // #[test]
    // fn compile_if_else_else_branch() {
    //     init();

    //     let raw = "
    //             var <- 2
    //             var2 <- 10
    //             if eq(2, sub(var, 1)) {
    //                 trap(1)
    //             } else {
    //                 var <- add(var, var2)
    //             }
    //             ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     // dbg!(&obj);

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, None);

    //     assert_eq!(
    //         vm.stack,
    //         [
    //             12, // var
    //             10  // var2
    //         ]
    //     );
    // }

    // #[test]
    // fn compile_if_else_if() {
    //     init();

    //     let raw = "
    //             var <- 2
    //             var2 <- 10
    //             if 0 {
    //                 trap(1)
    //             } else if 0 {
    //                 trap(2)
    //             } else {
    //                 var <- add(var, var2)
    //             }
    //             ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     // dbg!(&obj);

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, None);

    //     assert_eq!(
    //         vm.stack,
    //         [
    //             12, // var
    //             10  // var2
    //         ]
    //     );
    // }

    // #[test]
    // fn compile_def_single_arg() {
    //     init();

    //     let raw = "
    //             def square(i) -> o {
    //                 o <- mul(i, i)
    //             }

    //             five <- add(1, 4)
    //             v <- square(five)

    //             u <- add(1, v)

    //             alloc(16)
    //             write8(0, v)
    //             write8(8, u)
    //             exit(0, 16)
    //             ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(
    //         res,
    //         Some(
    //             [25_u64.to_be_bytes(), 26_u64.to_be_bytes()]
    //                 .as_flattened()
    //                 .to_vec()
    //         )
    //     );
    // }

    // #[test]
    // fn compile_def_multiple_args() {
    //     init();

    //     let raw = "
    //             def add_mul(a, b) -> o {
    //                 o <- mul(a, add(a, b))
    //             }

    //             three <- 3
    //             v <- add_mul(three, 5)

    //             alloc(8)
    //             write8(0, v)
    //             exit(0, 8)
    //             ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, Some(24_u64.to_be_bytes().to_vec()));
    // }

    // #[test]
    // fn fib_recursive() {
    //     init();

    //     let raw = "
    //             def fib(n) -> m {
    //                 if eq(n, 0) {
    //                     m <- 0
    //                 }

    //                 if eq(n, 1) {
    //                     m <- 1
    //                 }

    //                 if gt(n, 1) {
    //                     m <- add(fib(sub(n, 1)), fib(sub(n, 2)))
    //                 }
    //             }

    //             res <- fib(10)

    //             alloc(8)
    //             write8(0, res)
    //             exit(0, 8)
    //             ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, Some(55_u64.to_be_bytes().to_vec()));
    // }

    // #[test]
    // fn compile_def_shadowing() {
    //     init();

    //     let raw = "
    //             def digit_to_place(digit, idx) -> n {
    //               n <- mul(digit, exp(10, sub(dlen(), add(idx, 1))))
    //             }

    //             if eq(dlen(), 0) {
    //               trap(1)
    //             }

    //             n <- 0
    //             idx <- 0

    //             loop :a {
    //               if eq(dlen(), idx) {
    //                 break :a
    //               }

    //               ascii_digit <- dread1(idx)

    //               if lt(ascii_digit, 0x30) {
    //                 trap(2)
    //               }

    //               if gt(ascii_digit, 0x39) {
    //                 trap(3)
    //               }

    //               digit <- sub(ascii_digit, 0x30)
    //               n <- add(n, digit_to_place(digit, idx))
    //               idx <- add(idx, 1)
    //             }

    //             alloc(8)
    //             write8(0, n)
    //             exit(0, 8)
    //         ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, b"123".to_vec());

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, Some(123_u64.to_be_bytes().to_vec()));
    // }

    // #[test]
    // fn multiple_return_values() {
    //     init();

    //     let raw = "
    //             def many(a) -> b, c, d, e, f {
    //                 b <- add(a, 1)
    //                 c <- add(a, 2)
    //                 d <- add(a, 3)
    //                 e <- add(a, 4)
    //                 f <- add(a, 5)
    //             }

    //             a <- 100

    //             b, c, d, e, f <- many(a)

    //             alloc(6)
    //             write1(0, a)
    //             write1(1, b)
    //             write1(2, c)
    //             write1(3, d)
    //             write1(4, e)
    //             write1(5, f)
    //             exit(0, 6)
    //         ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     // dbg!(&obj);

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, Some(vec![100, 101, 102, 103, 104, 105]));

    //     assert_eq!(vm.stack, [100, 101, 102, 103, 104, 105]);
    // }

    // #[test]
    // fn multiple_return_values_update_and_init() {
    //     init();

    //     let raw = "
    //             def foo(a, b) -> c, d, e {
    //                 c <- b
    //                 d <- a
    //                 e <- 0x22
    //             }

    //             a <- 0x11
    //             c <- 0x33

    //             a, b, c <- foo(a, c)

    //             alloc(3)
    //             write1(0, a)
    //             write1(1, b)
    //             write1(2, c)
    //             exit(0, 3)
    //         ";

    //     // # 0x33, 0x11, 0x22

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     // dbg!(&obj);

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(
    //         res,
    //         Some(vec![
    //             0x33, // a
    //             0x11, // b
    //             0x22, // c
    //         ])
    //     );

    //     // a and c are pushed to the stack, then b when it is first set in
    // the multi     // assignment along with a and c being updated
    //     assert_eq!(
    //         vm.stack,
    //         [
    //             0x33, // a
    //             0x22, // c
    //             0x11, // b
    //         ]
    //     );
    // }

    // #[test]
    // fn multiple_return_values_as_args() {
    //     init();

    //     let raw = "
    //             def foo(a) -> c, d {
    //                 c <- mul(10, a)
    //                 d <- mul(2, a)
    //             }

    //             a <- sub(...foo(4))

    //             alloc(1)
    //             write1(0, a)
    //             exit(0, 1)
    //         ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     // dbg!(&obj);

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, Some(vec![(10 * 4) - (2 * 4)]));

    //     assert_eq!(vm.stack, [(10 * 4) - (2 * 4)]);
    // }

    // #[test]
    // fn multiple_return_values_as_args_complex() {
    //     init();

    //     let raw = "
    //             def foo(a) -> c, d {
    //                 c <- mul(10, a)
    //                 d <- mul(2, a)
    //             }

    //             def bar(a, b, c) -> d {
    //                 d <- mul(a, add(b, c))
    //             }

    //             def baz(a) -> d {
    //                 d <- add(a, 1)
    //             }

    //             a <- 1
    //             res <- bar(...foo(4), a)

    //             alloc(3)
    //             write1(0, a)
    //             write2(1, res)
    //             exit(0, 3)
    //         ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     // dbg!(&obj);

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(
    //         res,
    //         Some(
    //             [[1_u8].as_slice(), 360_u16.to_be_bytes().as_slice()]
    //                 .into_iter()
    //                 .flatten()
    //                 .copied()
    //                 .collect::<Vec<_>>()
    //         )
    //     );
    // }

    // #[test]
    // fn multiple_return_swap_params() {
    //     init();

    //     let raw = "
    //             def swap(a_, b_) -> c, d {
    //                 d <- a_
    //                 c <- b_
    //             }

    //             a <- 0xaa
    //             b <- 0xbb

    //             a, b <- swap(a, b)

    //             alloc(2)
    //             write1(0, a)
    //             write1(1, b)
    //             exit(0, 2)
    //         ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     // dbg!(&obj);

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, Some(vec![0xbb, 0xaa]));

    //     assert_eq!(vm.stack, [0xbb, 0xaa]);
    // }

    // #[test]
    // fn compile_loop() {
    //     init();

    //     let raw = "
    //             counter <- 0x00

    //             loop :a {
    //               counter <- add(counter, 1)
    //               if eq(counter, 10) {
    //                 break :a
    //               }
    //             }

    //             alloc(1)
    //             write1(0, counter)
    //             exit(0, 1)
    //             ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     // dbg!(&obj);

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, Some(vec![10]));
    // }

    // #[test]
    // fn compile_loop_shadow_label() {
    //     init();

    //     let raw = "
    //             counter <- 0x00

    //             loop :a {
    //               loop :a {
    //                 counter <- add(counter, 1)
    //                 if eq(counter, 10) {
    //                   break :a
    //                 }
    //               }
    //               break :a
    //             }

    //             alloc(1)
    //             write1(0, counter)
    //             exit(0, 1)
    //             ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, vec![]);

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, Some(vec![10]));
    // }

    // #[test]
    // fn compile_atoi() {
    //     // # n += digit * (10 ** (dlen() - (idx + 1)))
    //     let raw = "
    //             if eq(dlen(), 0) {
    //               trap(1)
    //             }

    //             n <- 0
    //             idx <- 0

    //             loop :a {
    //               if eq(dlen(), idx) {
    //                 break :a
    //               }

    //               ascii_digit <- dread1(idx)

    //               if lt(ascii_digit, 0x30) {
    //                 trap(2)
    //               }

    //               if gt(ascii_digit, 0x39) {
    //                 trap(3)
    //               }

    //               digit <- sub(ascii_digit, 0x30)
    //               n <- add(n, mul(digit, exp(10, sub(dlen(), add(idx, 1)))))
    //               idx <- add(idx, 1)
    //             }

    //             alloc(8)
    //             write8(0, n)
    //             exit(0, 8)
    //             ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, b"1234567".into());

    //     let res = vm.run().unwrap();

    //     assert_eq!(res.unwrap(), 1234567_u64.to_be_bytes());
    // }

    // #[test]
    // fn compile_aoc_2025_1() {
    //     init();

    //     let raw = "
    //         if eq(dlen(), 0) {
    //           trap(1)
    //         }

    //         dial <- 100050
    //         total <- 0
    //         n <- 0
    //         idx <- 0
    //         is_right <- 0

    //         loop :a {
    //           if eq(dlen(), idx) {
    //             break :a
    //           }

    //           ascii_digit <- dread1(idx)

    //           if eq(ascii_digit, 76) {
    //             is_right <- 0

    //             idx <- add(idx, 1)
    //             n <- 0
    //             continue :a
    //           } else if eq(ascii_digit, 82) {
    //             is_right <- 1

    //             idx <- add(idx, 1)
    //             n <- 0
    //             continue :a
    //           } else if eq(ascii_digit, 10) {
    //             idx <- add(idx, 1)
    //             if is_right {
    //               dial <- add(dial, n)
    //               if eq(0, mod(dial, 100)) {
    //                 total <- add(total, 1)
    //               }

    //               continue :a
    //             }

    //             dial <- sub(dial, n)
    //             if eq(0, mod(dial, 100)) {
    //               total <- add(total, 1)
    //             }

    //             continue :a
    //           }

    //           if lt(ascii_digit, 0x30) {
    //             trap(2)
    //           }

    //           if gt(ascii_digit, 0x39) {
    //             trap(3)
    //           }

    //           digit <- sub(ascii_digit, 0x30)
    //           n <- mul(n, 10)
    //           n <- add(n, digit)
    //           idx <- add(idx, 1)
    //         }

    //         alloc(8)
    //         write8(0, total)
    //         exit(0, 8)
    //         ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     println!("{obj}");
    //     // dbg!(&obj);

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(
    //         asm,
    //         b"L68
    // L30
    // R48
    // L5
    // R60
    // L55
    // L1
    // L99
    // R14
    // L82
    // "
    //         .into(),
    //     );

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, Some(3_u64.to_be_bytes().to_vec()))
    // }

    // #[test]
    // fn drop_vars_in_if_block() {
    //     init();

    //     let raw = "
    // x <- 1
    // y <- 0
    // t <- 0
    // loop :a {
    //   if lt(t, 24) {
    //     Y <- mod(add(mul(2, x), mul(3, y)), 5)
    //     t <- add(t, 1)
    //   } else {
    //     break :a
    //   }
    // }
    // alloc(8)
    // write8(0, t)
    // exit(0, 8)
    // ";

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     println!("{obj}");

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, b"".into());

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, Some(24_u64.to_be_bytes().to_vec()))
    // }

    // #[test]
    // fn drop_vars_in_def_body() {
    //     init();

    //     let raw = r#"
    // def f(at) -> u {
    //   i <- 7
    // }

    // f(0)
    //         "#;

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, b"".into());

    //     let res = vm.run().unwrap();

    //     assert_eq!(res, None)
    // }

    // #[test]
    // fn stack_depth_after_call_is_correct() {
    //     init();

    //     let raw = r#"
    // def inner(a, b, inner_at, value, c, d) {
    //   write8(inner_at, value)
    // }

    // def outer(a, b, outer_at, value, c, d) -> n, m {
    //   inner(a, b, outer_at, value, c, d)
    //   n <- 0xaa
    //   m <- 0xbb
    //   # inner(y, inner_at, z)
    // }

    // alloc(8)
    // n, m <- outer(0xa, 0xb, 0, 0xFFF, 0xc, 0xd)
    // trap(n)
    //         "#;

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     ctx.compile(&ast).unwrap();

    //     let obj = ctx.into_object();

    //     let asm = obj.assemble();

    //     let mut vm = Vm::new(asm, b"".into());

    //     let err = vm.run().unwrap_err();

    //     assert_eq!(err, Error::Trap(0xaa));
    // }

    // #[test]
    // fn outer_def_cannot_refer_to_called_def_arg() {
    //     init();

    //     let raw = r#"
    // def inner(inner_at) {
    //   write8(inner_at, 0x0)
    // }

    // def outer(outer_at) -> n, m {
    //   inner(inner_at)
    // }

    // alloc(8)
    // outer(1)
    //         "#;

    //     let ast = grammar().block.parse(raw).unwrap();

    //     let mut ctx = Ctx::new_root();

    //     let err = ctx.compile(&ast).unwrap_err();

    //     assert_eq!(
    //         err,
    //         CompileError::VarNotFound {
    //             var: "inner_at".to_owned()
    //         }
    //     );
    // }
}
