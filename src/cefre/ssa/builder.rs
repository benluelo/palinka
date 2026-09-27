use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self},
};

use crate::cefre::ssa::id_map::IdMap;

macro_rules! id {
    ($Ty:ident, $prefix:literal) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $Ty(usize);

        impl crate::cefre::ssa::id_map::Id for $Ty {
            fn as_usize(self) -> usize {
                self.0
            }

            fn from_usize(id: usize) -> Self {
                Self(id)
            }
        }

        impl fmt::Debug for $Ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{self}")
            }
        }

        impl fmt::Display for $Ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }
    };
}

#[derive(Debug)]
pub struct Builder {
    blocks: IdMap<BlockId, Block>,
    defs: IdMap<DefId, ()>,
    vars: IdMap<VarId, BlockId>,
}

impl fmt::Display for Builder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (block_id, block) in self.blocks.iter() {
            if block.args.is_empty() {
                writeln!(f, "{block_id}:")?;
            } else {
                write!(f, "{block_id}(")?;
                for arg in &block.args {
                    write!(f, "{arg} ")?;
                }
                write!(f, "):")?;
            }

            for statement in &block.instructions {
                writeln!(f, "  {statement}")?;
            }

            writeln!(f, "  {}", block.terminal)?;

            writeln!(f)?;
        }

        Ok(())
    }
}

impl Builder {
    #[expect(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { blocks: IdMap::new(), defs: IdMap::new(), vars: IdMap::new() }
    }

    // pub fn blocks(&self) -> &BTreeMap<BlockId, Block> {
    //     &self.blocks
    // }

    pub fn push_var(&mut self, in_block: BlockId) -> VarId {
        let var_id = self.vars.insert(in_block);
        self.blocks[in_block].def_vars.insert(var_id);
        var_id
    }

    // pub fn pop_var(&mut self, id: VarId) {
    //     self.vars.remove(id)
    // }

    pub fn push_def(&mut self) -> DefId {
        self.defs.insert(())
    }

    pub fn push_statements(
        &mut self,
        block_id: BlockId,
        statements: impl IntoIterator<Item = Statement>,
    ) {
        self.blocks.get_mut(block_id).instructions.extend(statements);
    }

    pub fn push_block(&mut self) -> BlockId {
        self.blocks.insert(Block {
            predecessors: vec![],
            args: vec![],
            instructions: vec![],
            terminal: Terminal::None,
            sealed: Sealed::No { vars: BTreeSet::new() },
            def_vars: BTreeSet::new(),
        })
    }

    pub fn block(&self, block_id: BlockId) -> &Block {
        self.blocks.get(block_id)
    }

    pub fn block_mut(&mut self, block_id: BlockId) -> &mut Block {
        self.blocks.get_mut(block_id)
    }

    /// Get the block that a var was defined in.
    pub fn get_var_def_block(&self, var_id: VarId) -> BlockId {
        *self.vars.get(var_id)
    }

    // pub fn pop_def(&mut self, id: DefId) {
    //     self.defs.remove(id);
    // }
}

id!(BlockId, "bb");
id!(VarId, "t");
id!(DefId, "f");

#[derive(Debug, Clone)]
pub struct Block {
    pub predecessors: Vec<BlockId>,
    pub args: Vec<VarId>,
    pub instructions: Vec<Statement>,
    pub terminal: Terminal,
    pub sealed: Sealed,
    pub def_vars: BTreeSet<VarId>,
}

impl Block {
    pub fn is_sealed(&self) -> bool {
        matches!(self.sealed, Sealed::Yes)
    }

    // pub fn undef_vars(&self) -> impl Iterator<Item = VarId> {
    //     self.instructions.iter().flat_map(|s| match s {
    //         Statement::Builtin(builtin_call) => todo!(),
    //         Statement::Assignment { var_id, expr } => todo!(),
    //         Statement::Call { def_id, args, rets } => todo!(),
    //     })
    // }
}

#[derive(Debug, Clone)]
pub enum Sealed {
    Yes,
    No { vars: BTreeSet<VarId> },
}

#[derive(Clone)]
pub enum Statement {
    /// Call to a builtin
    Builtin(BuiltinCall),
    /// Assignment of an expression to a variable
    Assignment { var_id: VarId, expr: Expr },
    /// Call to a def, returning 0 or more values as a result
    Call {
        def_id: DefId,
        /// Arguments to the function (either variables or constants)
        args: Vec<Operand>,
        /// Return values of the function
        rets: Vec<VarId>,
    },
}

impl fmt::Debug for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Statement::Builtin(builtin_call) => write!(f, "{builtin_call}"),
            Statement::Assignment { var_id, expr } => write!(f, "{var_id} <- {expr}"),
            Statement::Call { def_id, args, rets } => {
                write!(
                    f,
                    "{rets} <- call {def_id}({args})",
                    rets = rets.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","),
                    args = args.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","),
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    /// Constant value
    Const(u64),
    /// Block-local variable
    Var(VarId),
    /// Call to a builtin
    Builtin(BuiltinCall),
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Const(c) => write!(f, "{c}"),
            Expr::Var(v) => write!(f, "{v}"),
            Expr::Builtin(builtin_call) => write!(f, "{builtin_call}"),
        }
    }
}

#[derive(Clone)]
pub enum BuiltinCall {
    Add(Operand, Operand),
    Sub(Operand, Operand),
    Mul(Operand, Operand),
    Div(Operand, Operand),
    Exp(Operand, Operand),
    Mod(Operand, Operand),
    Eq(Operand, Operand),
    Lt(Operand, Operand),
    Gt(Operand, Operand),
    Shl(Operand, Operand),
    Shr(Operand, Operand),
    Or(Operand, Operand),
    Xor(Operand, Operand),
    And(Operand, Operand),
    Not(Operand),
    Neg(Operand),
    Dread1(Operand),
    Dread2(Operand),
    Dread3(Operand),
    Dread4(Operand),
    Dread5(Operand),
    Dread6(Operand),
    Dread7(Operand),
    Dread8(Operand),
    Dlen(),
    Read1(Operand),
    Read2(Operand),
    Read3(Operand),
    Read4(Operand),
    Read5(Operand),
    Read6(Operand),
    Read7(Operand),
    Read8(Operand),
    Alloc(Operand),
    Write1(Operand, Operand),
    Write2(Operand, Operand),
    Write3(Operand, Operand),
    Write4(Operand, Operand),
    Write5(Operand, Operand),
    Write6(Operand, Operand),
    Write7(Operand, Operand),
    Write8(Operand, Operand),
    Dcopy(Operand, Operand, Operand),
    Exit(Operand, Operand),
    Trap(Operand),
}

impl fmt::Debug for BuiltinCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for BuiltinCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuiltinCall::Add(a, b) => write!(f, "add({a}, {b})"),
            BuiltinCall::Sub(a, b) => write!(f, "sub({a}, {b})"),
            BuiltinCall::Mul(a, b) => write!(f, "mul({a}, {b})"),
            BuiltinCall::Div(a, b) => write!(f, "div({a}, {b})"),
            BuiltinCall::Exp(a, b) => write!(f, "exp({a}, {b})"),
            BuiltinCall::Mod(a, b) => write!(f, "mod({a}, {b})"),
            BuiltinCall::Eq(a, b) => write!(f, "eq({a}, {b})"),
            BuiltinCall::Lt(a, b) => write!(f, "lt({a}, {b})"),
            BuiltinCall::Gt(a, b) => write!(f, "gt({a}, {b})"),
            BuiltinCall::Shl(a, b) => write!(f, "shl({a}, {b})"),
            BuiltinCall::Shr(a, b) => write!(f, "shr({a}, {b})"),
            BuiltinCall::Or(a, b) => write!(f, "or({a}, {b})"),
            BuiltinCall::Xor(a, b) => write!(f, "xor({a}, {b})"),
            BuiltinCall::And(a, b) => write!(f, "and({a}, {b})"),
            BuiltinCall::Not(a) => write!(f, "not({a})"),
            BuiltinCall::Neg(a) => write!(f, "neg({a})"),
            BuiltinCall::Dread1(a) => write!(f, "dread1({a})"),
            BuiltinCall::Dread2(a) => write!(f, "dread2({a})"),
            BuiltinCall::Dread3(a) => write!(f, "dread3({a})"),
            BuiltinCall::Dread4(a) => write!(f, "dread4({a})"),
            BuiltinCall::Dread5(a) => write!(f, "dread5({a})"),
            BuiltinCall::Dread6(a) => write!(f, "dread6({a})"),
            BuiltinCall::Dread7(a) => write!(f, "dread7({a})"),
            BuiltinCall::Dread8(a) => write!(f, "dread8({a})"),
            BuiltinCall::Dlen() => write!(f, "dlen()"),
            BuiltinCall::Read1(a) => write!(f, "read1({a})"),
            BuiltinCall::Read2(a) => write!(f, "read2({a})"),
            BuiltinCall::Read3(a) => write!(f, "read3({a})"),
            BuiltinCall::Read4(a) => write!(f, "read4({a})"),
            BuiltinCall::Read5(a) => write!(f, "read5({a})"),
            BuiltinCall::Read6(a) => write!(f, "read6({a})"),
            BuiltinCall::Read7(a) => write!(f, "read7({a})"),
            BuiltinCall::Read8(a) => write!(f, "read8({a})"),
            BuiltinCall::Alloc(a) => write!(f, "alloc({a})"),
            BuiltinCall::Write1(a, b) => write!(f, "write1({a}, {b})"),
            BuiltinCall::Write2(a, b) => write!(f, "write2({a}, {b})"),
            BuiltinCall::Write3(a, b) => write!(f, "write3({a}, {b})"),
            BuiltinCall::Write4(a, b) => write!(f, "write4({a}, {b})"),
            BuiltinCall::Write5(a, b) => write!(f, "write5({a}, {b})"),
            BuiltinCall::Write6(a, b) => write!(f, "write6({a}, {b})"),
            BuiltinCall::Write7(a, b) => write!(f, "write7({a}, {b})"),
            BuiltinCall::Write8(a, b) => write!(f, "write8({a}, {b})"),
            BuiltinCall::Dcopy(a, b, c) => write!(f, "dcopy({a}, {b}, {c})"),
            BuiltinCall::Exit(a, b) => write!(f, "exit({a}, {b})"),
            BuiltinCall::Trap(a) => write!(f, "trap({a})"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Operand {
    Const(u64),
    Var(VarId),
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Const(c) => write!(f, "{c}"),
            Operand::Var(v) => write!(f, "{v}"),
        }
    }
}

#[derive(Clone)]
pub enum Terminal {
    /// Conditional jump to the provided label
    Jump {
        cond: Expr,
        then: Goto,
        else_: Goto,
    },
    /// Unconditional jump to the provided goto label
    Goto(Goto),
    // Trap (Operand),
    // Exit (Operand, Operand),
    End,

    None,
}

impl fmt::Debug for Terminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for Terminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Terminal::Jump { cond, then, else_ } => write!(f, "if {cond} then {then} else {else_}"),
            Terminal::Goto(goto) => write!(f, "goto {goto}"),
            Terminal::End => write!(f, "end"),
            Terminal::None => write!(f, "<NONE>"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Goto {
    pub label: BlockId,
    pub args: Vec<Operand>,
}

impl fmt::Display for Goto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label)?;
        if !self.args.is_empty() {
            write!(f, "(")?;
            for (idx, arg) in self.args.iter().enumerate() {
                write!(f, "{arg}")?;
                if idx < self.args.len() - 1 {
                    write!(f, ", ")?;
                }
            }
            write!(f, ")")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Def {
    params: Vec<VarId>,
    rets: Vec<VarId>,
    blocks: BTreeMap<BlockId, Block>,
}
