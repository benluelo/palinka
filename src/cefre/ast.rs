use std::{
    borrow::Cow,
    cmp::Ordering,
    fmt::{self, Write},
};

use chumsky::span::{SimpleSpan, Spanned};

#[derive(Clone)]
pub struct Ident<'a>(Spanned<Cow<'a, str>>);

impl<'a> Ord for Ident<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.inner.cmp(&other.0.inner)
    }
}

impl<'a> PartialOrd for Ident<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> Eq for Ident<'a> {}

impl<'a> Ident<'a> {
    pub fn new_spanned(spanned: Spanned<impl Into<Cow<'a, str>>>) -> Self {
        Self(Spanned { inner: spanned.inner.into(), span: spanned.span })
    }

    pub fn new(ident: impl Into<Cow<'a, str>>) -> Self {
        Self::new_spanned(Spanned { inner: ident.into(), span: (0..0).into() })
    }

    pub fn as_static(&self) -> Ident<'static> {
        Ident(Spanned { inner: self.0.inner.clone().into_owned().into(), span: self.0.span })
    }

    pub fn span(&self) -> SimpleSpan {
        self.0.span
    }
}

impl<'a> fmt::Debug for Ident<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "Ident(\"{}\" @ {})", self.0.inner, self.0.span)
        } else {
            write!(f, "Ident(\"{}\")", self.0.inner)
        }
    }
}

impl<'a> PartialEq for Ident<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.0.inner == other.0.inner
    }
}

impl<'a> PartialEq<str> for Ident<'a> {
    fn eq(&self, other: &str) -> bool {
        self.0.inner == other
    }
}

impl<'a> fmt::Display for Ident<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy)]
pub struct Val(Spanned<u64>);

impl fmt::Debug for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "Val({} @ {})", self.0.inner, self.0.span)
        } else {
            write!(f, "Val({})", self.0.inner)
        }
    }
}

impl PartialEq for Val {
    fn eq(&self, other: &Self) -> bool {
        self.0.inner == other.0.inner
    }
}

impl Val {
    pub fn value(&self) -> u64 {
        self.0.inner
    }

    pub fn new_spanned(spanned: Spanned<u64>) -> Val {
        Self(spanned)
    }

    pub fn new(ident: u64) -> Self {
        Self::new_spanned(Spanned { inner: ident, span: (0..0).into() })
    }
}

impl fmt::Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.inner, f)
    }
}

impl fmt::LowerHex for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0.inner, f)
    }
}

#[derive(Clone, Copy, Eq, PartialOrd, Ord)]
pub struct Label<'a>(Spanned<&'a str>);

impl<'a> Label<'a> {
    pub fn new_spanned(spanned: Spanned<&'a str>) -> Self {
        Self(spanned)
    }

    pub fn new(label: &'a str) -> Self {
        Self(Spanned { inner: label, span: (0..0).into() })
    }

    pub fn span(&self) -> SimpleSpan {
        self.0.span
    }
}

impl<'a> fmt::Debug for Label<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "Label(\"{}\" @ {})", self.0.inner, self.0.span)
        } else {
            write!(f, "Label(\"{}\")", self.0.inner)
        }
    }
}

impl<'a> PartialEq for Label<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.0.inner == other.0.inner
    }
}

impl<'a> fmt::Display for Label<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Block<'a>(Spanned<Vec<Statement<'a>>>);

impl<'a> PartialEq for Block<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.0.inner == other.0.inner
    }
}

impl<'a> Block<'a> {
    pub fn new_spanned(spanned: Spanned<Vec<Statement<'a>>>) -> Self {
        Self(spanned)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Statement<'a>> {
        self.0.inner.iter()
    }

    pub fn span(&self) -> SimpleSpan {
        self.0.span
    }

    pub fn new(statements: Vec<Statement<'a>>, span: impl Into<SimpleSpan>) -> Self {
        Self(Spanned { inner: statements, span: span.into() })
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn statements(&self) -> &[Statement<'a>] {
        &self.0
    }

    pub fn statements_mut(&mut self) -> &mut [Statement<'a>] {
        &mut self.0
    }
}

impl<'a> IntoIterator for Block<'a> {
    type Item = Statement<'a>;

    type IntoIter = std::vec::IntoIter<Statement<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.inner.into_iter()
    }
}

impl<'a, 'b: 'a> IntoIterator for &'b Block<'a> {
    type Item = &'b Statement<'a>;

    type IntoIter = std::slice::Iter<'b, Statement<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.inner.iter()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement<'a> {
    Expr(Expr<'a>),
    Loop(Loop<'a>),
    Break(Break<'a>),
    Continue(Continue<'a>),
    If(If<'a>),
    Assignment(Assignment<'a>),
    Def(Def<'a>),
}

#[derive(Clone)]
pub enum Expr<'a> {
    Val(Val),
    Var(Ident<'a>),
    Call { spread: bool, f: Spanned<BuiltinOrDef<'a>>, args: Vec<Expr<'a>> },
}

impl<'a> Expr<'a> {
    pub fn val(val: u64, span: SimpleSpan) -> Expr<'a> {
        Expr::Val(Val::new_spanned(Spanned { inner: val, span }))
    }

    // pub fn call(f: impl Into<BuiltinOrDef<'a>>, span: SimpleSpan) -> Expr<'a> {
    //     Expr::Val(Val::new_spanned(Spanned { inner: f.into(), span }))
    // }

    pub fn builtin(f: Builtin, span: SimpleSpan, args: Vec<Expr<'a>>) -> Expr<'a> {
        // builtins only ever return 0 or 1 value, can never spread
        Expr::Call { spread: false, f: Spanned { inner: BuiltinOrDef::Builtin(f), span }, args }
    }

    pub fn as_val(&self) -> Option<&Val> {
        if let Self::Val(v) = self { Some(v) } else { None }
    }

    pub fn as_val_mut(&mut self) -> Option<&mut Val> {
        if let Self::Val(v) = self { Some(v) } else { None }
    }
}

impl<'a> fmt::Debug for Expr<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Val(val) => {
                if f.alternate() {
                    write!(f, "{val:#?}")
                } else {
                    write!(f, "{val:?}")
                }
            }
            Self::Var(var) => {
                if f.alternate() {
                    write!(f, "{var:#?}")
                } else {
                    write!(f, "{var:?}")
                }
            }
            Self::Call { spread, f: f_, args } => f
                .debug_struct("Call")
                .field("spread", spread)
                // TODO: Include the span here in the alternate format (probably with a newtype for
                // debug)
                .field("f", &**f_)
                .field("args", args)
                .finish(),
        }
    }
}

impl<'a> PartialEq for Expr<'a> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Val(l), Self::Val(r)) => l == r,
            (Self::Var(l), Self::Var(r)) => l == r,
            (
                Self::Call { spread: l_spread, f: l_f, args: l_args },
                Self::Call { spread: r_spread, f: r_f, args: r_args },
            ) => l_spread == r_spread && l_f.inner == r_f.inner && l_args == r_args,
            _ => false,
        }
    }
}

impl Expr<'_> {
    pub fn span(&self) -> SimpleSpan {
        match self {
            Expr::Val(val) => val.0.span,
            Expr::Var(var) => var.0.span,
            // TODO: Include the spread and args spans
            Expr::Call { spread: _, f, args: _ } => f.span,
        }
    }

    /// Returns `true` if the expr is [`Val`].
    ///
    /// [`Val`]: Expr::Val
    #[must_use]
    pub fn is_val(&self) -> bool {
        matches!(self, Self::Val(..))
    }
}

impl<'a> fmt::Display for Expr<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Val(val) => f.write_fmt(format_args!("0x{val:x}")),
            Expr::Var(var) => f.write_fmt(format_args!("{var}")),
            Expr::Call { spread, f: call, args } => {
                if *spread {
                    f.write_str("...")?;
                }
                f.write_fmt(format_args!("{}", call.inner))?;
                f.write_char('(')?;
                for (i, expr) in args.iter().enumerate() {
                    f.write_fmt(format_args!("{expr}"))?;
                    if i < args.len() - 1 {
                        f.write_str(", ")?;
                    }
                }
                f.write_char(')')
            }
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum BuiltinOrDef<'a> {
    Builtin(Builtin),
    Def(Ident<'a>),
}

impl<'a> fmt::Debug for BuiltinOrDef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(builtin) => {
                write!(f, "Builtin({builtin})")
            }
            Self::Def(def) => write!(f, "Def({def:?})"),
        }
    }
}

impl<'a> From<Builtin> for BuiltinOrDef<'a> {
    fn from(v: Builtin) -> Self {
        Self::Builtin(v)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Builtin {
    Add,
    Sub,
    Mul,
    Div,
    Exp,
    Mod,
    Eq,
    Lt,
    Gt,
    Shl,
    Shr,
    Or,
    Xor,
    And,
    Not,
    Neg,
    Dread1,
    Dread2,
    Dread3,
    Dread4,
    Dread5,
    Dread6,
    Dread7,
    Dread8,
    Dlen,
    Read1,
    Read2,
    Read3,
    Read4,
    Read5,
    Read6,
    Read7,
    Read8,
    Alloc,
    Write1,
    Write2,
    Write3,
    Write4,
    Write5,
    Write6,
    Write7,
    Write8,
    Dcopy,
    Exit,
    Trap,
}

impl fmt::Display for Builtin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mul => "mul",
            Self::Div => "div",
            Self::Exp => "exp",
            Self::Mod => "mod",
            Self::Eq => "eq",
            Self::Lt => "lt",
            Self::Gt => "gt",
            Self::Shl => "shl",
            Self::Shr => "shr",
            Self::Or => "or",
            Self::Xor => "xor",
            Self::And => "and",
            Self::Not => "not",
            Self::Neg => "neg",
            Self::Dread1 => "dread1",
            Self::Dread2 => "dread2",
            Self::Dread3 => "dread3",
            Self::Dread4 => "dread4",
            Self::Dread5 => "dread5",
            Self::Dread6 => "dread6",
            Self::Dread7 => "dread7",
            Self::Dread8 => "dread8",
            Self::Dlen => "dlen",
            Self::Read1 => "read1",
            Self::Read2 => "read2",
            Self::Read3 => "read3",
            Self::Read4 => "read4",
            Self::Read5 => "read5",
            Self::Read6 => "read6",
            Self::Read7 => "read7",
            Self::Read8 => "read8",
            Self::Alloc => "alloc",
            Self::Write1 => "write1",
            Self::Write2 => "write2",
            Self::Write3 => "write3",
            Self::Write4 => "write4",
            Self::Write5 => "write5",
            Self::Write6 => "write6",
            Self::Write7 => "write7",
            Self::Write8 => "write8",
            Self::Dcopy => "dcopy",
            Self::Exit => "exit",
            Self::Trap => "trap",
        })
    }
}

impl<'a> fmt::Display for BuiltinOrDef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(builtin) => f.write_fmt(format_args!("{builtin}")),
            Self::Def(ident) => f.write_fmt(format_args!("{ident}")),
        }
    }
}

impl<'a> From<Ident<'a>> for BuiltinOrDef<'a> {
    fn from(ident: Ident<'a>) -> Self {
        match &*ident.0.inner {
            "add" => Self::Builtin(Builtin::Add),
            "sub" => Self::Builtin(Builtin::Sub),
            "mul" => Self::Builtin(Builtin::Mul),
            "div" => Self::Builtin(Builtin::Div),
            "exp" => Self::Builtin(Builtin::Exp),
            "mod" => Self::Builtin(Builtin::Mod),
            "eq" => Self::Builtin(Builtin::Eq),
            "lt" => Self::Builtin(Builtin::Lt),
            "gt" => Self::Builtin(Builtin::Gt),
            "shl" => Self::Builtin(Builtin::Shl),
            "shr" => Self::Builtin(Builtin::Shr),
            "or" => Self::Builtin(Builtin::Or),
            "xor" => Self::Builtin(Builtin::Xor),
            "and" => Self::Builtin(Builtin::And),
            "not" => Self::Builtin(Builtin::Not),
            "neg" => Self::Builtin(Builtin::Neg),
            "dread1" => Self::Builtin(Builtin::Dread1),
            "dread2" => Self::Builtin(Builtin::Dread2),
            "dread3" => Self::Builtin(Builtin::Dread3),
            "dread4" => Self::Builtin(Builtin::Dread4),
            "dread5" => Self::Builtin(Builtin::Dread5),
            "dread6" => Self::Builtin(Builtin::Dread6),
            "dread7" => Self::Builtin(Builtin::Dread7),
            "dread8" => Self::Builtin(Builtin::Dread8),
            "dlen" => Self::Builtin(Builtin::Dlen),
            "read1" => Self::Builtin(Builtin::Read1),
            "read2" => Self::Builtin(Builtin::Read2),
            "read3" => Self::Builtin(Builtin::Read3),
            "read4" => Self::Builtin(Builtin::Read4),
            "read5" => Self::Builtin(Builtin::Read5),
            "read6" => Self::Builtin(Builtin::Read6),
            "read7" => Self::Builtin(Builtin::Read7),
            "read8" => Self::Builtin(Builtin::Read8),
            "alloc" => Self::Builtin(Builtin::Alloc),
            "write1" => Self::Builtin(Builtin::Write1),
            "write2" => Self::Builtin(Builtin::Write2),
            "write3" => Self::Builtin(Builtin::Write3),
            "write4" => Self::Builtin(Builtin::Write4),
            "write5" => Self::Builtin(Builtin::Write5),
            "write6" => Self::Builtin(Builtin::Write6),
            "write7" => Self::Builtin(Builtin::Write7),
            "write8" => Self::Builtin(Builtin::Write8),
            "dcopy" => Self::Builtin(Builtin::Dcopy),
            "exit" => Self::Builtin(Builtin::Exit),
            "trap" => Self::Builtin(Builtin::Trap),
            _ => Self::Def(ident),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Break<'a>(pub Label<'a>);

#[derive(Debug, Clone, PartialEq)]
pub struct Continue<'a>(pub Label<'a>);

#[derive(Debug, Clone, PartialEq)]
pub struct Loop<'a> {
    pub label: Label<'a>,
    pub block: Block<'a>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct If<'a> {
    pub cond: Expr<'a>,
    pub block: Block<'a>,
    pub else_: Option<Else<'a>>,
}

#[derive(Debug, Clone)]
pub enum Else<'a> {
    ElseIf { if_: Box<Spanned<If<'a>>> },
    Tail { block: Block<'a> },
}

impl<'a> PartialEq for Else<'a> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::ElseIf { if_: l_if_ }, Self::ElseIf { if_: r_if_ }) => {
                l_if_.inner == r_if_.inner
            }
            (Self::Tail { block: l_block }, Self::Tail { block: r_block }) => l_block == r_block,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assignment<'a> {
    pub const_: bool,
    pub vars: Vec<Ident<'a>>,
    pub expr: Expr<'a>,
}

impl<'a> fmt::Display for Assignment<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, var) in self.vars.iter().enumerate() {
            write!(f, "{var}")?;
            if (i + 1) < self.vars.len() {
                f.write_str(", ")?;
            }
        }
        f.write_str(" <- ")?;
        write!(f, "{}", self.expr)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Def<'a> {
    pub ident: Ident<'a>,
    pub args: Vec<Ident<'a>>,
    pub rets: Vec<Ident<'a>>,
    pub body: Block<'a>,
}
