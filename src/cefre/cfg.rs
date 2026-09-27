use core::fmt;

use petgraph::graph::{DiGraph, NodeIndex};

use crate::cefre::ast::{Builtin, BuiltinOrDef, Ident, Val};

pub struct Cfg<'a> {
    cfg: DiGraph<Node<'a>, Edge>,
}

#[derive(Debug)]
enum Node<'a> {
    Root,
    CallEntry,
    CallExit,
    Expr(ExprNode<'a>),
    Assignment { vars: Vec<Ident<'a>>, expr: ExprNode<'a> },
}

impl fmt::Display for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Root => f.write_str("root"),
            Node::Expr(expr) => f.write_fmt(format_args!("{expr}")),
            Node::Assignment(assignment) => f.write_fmt(format_args!("{assignment}")),
            _ => todo!(),
        }
    }
}

#[derive(Debug)]
pub enum ExprNode<'a> {
    Val(Val),
    Var(Ident<'a>),
    Builtin { spread: bool, f: Builtin, args: Vec<ExprNode<'a>> },
    Call { spread: bool, f: Ident<'a>, id: NodeIndex, args: Vec<ExprNode<'a>> },
}

#[derive(Debug)]
enum Edge {
    None,
    Break,
    Continue,
    IfTrue,
    IfFalse,
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Edge::None => f.write_str(""),
            Edge::Break => f.write_str("break"),
            Edge::Continue => f.write_str("continue"),
            Edge::IfTrue => f.write_str("if true"),
            Edge::IfFalse => f.write_str("if false"),
        }
    }
}
