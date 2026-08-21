//! Crate-private owned IR for Language 1.
//!
//! This is the type `Engine` and `Compiled` actually store. It exists because
//! `rhai::AST` is `!Send` (the crate builds `rhai` **without** its `sync`
//! feature), so an `Engine` that held an AST could never satisfy D17. `check`
//! lowers into this and then **drops** the `rhai::AST`.
//!
//! Nothing here is public. It is not a `Backend` argument an embedder can see,
//! and per D10 its shape is explicitly **not** frozen — that is what lets a
//! future Cranelift or copy-and-patch backend land in-tree without breaking
//! anyone. Everything is owned (`String`, `Vec`), never `Rc`, so it is `Send`.

/// A lowered program.
#[derive(Clone, Debug, Default)]
pub(crate) struct IrModule {
    pub(crate) functions: Vec<IrFunction>,
    /// Top-level statements, in source order.
    pub(crate) main: Vec<IrStmt>,
}

impl IrModule {
    pub(crate) fn function(&self, name: &str, arity: usize) -> Option<&IrFunction> {
        self.functions
            .iter()
            .find(|def| def.name == name && def.params.len() == arity)
    }

    pub(crate) fn has_function(&self, name: &str) -> bool {
        self.functions.iter().any(|def| def.name == name)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct IrFunction {
    pub(crate) name: String,
    pub(crate) params: Vec<String>,
    pub(crate) body: Vec<IrStmt>,
}

#[derive(Clone, Debug)]
pub(crate) enum IrStmt {
    /// An expression evaluated for its value; the last one in a block is the
    /// block's value.
    Expr(IrExpr),
    Let {
        name: String,
        value: IrExpr,
    },
    Assign {
        target: IrTarget,
        /// `None` = plain `=`; `Some(op)` = compound (`+=`, `-=`, …).
        op: Option<BinOp>,
        value: IrExpr,
    },
    If {
        cond: IrExpr,
        then_block: Vec<IrStmt>,
        else_block: Vec<IrStmt>,
    },
    While {
        cond: IrExpr,
        body: Vec<IrStmt>,
    },
    For {
        var: String,
        iterable: IrExpr,
        body: Vec<IrStmt>,
    },
    TryCatch {
        body: Vec<IrStmt>,
        catch_var: Option<String>,
        catch_block: Vec<IrStmt>,
    },
    Block(Vec<IrStmt>),
    Return(Option<IrExpr>),
    Break,
    Continue,
    Throw(IrExpr),
}

/// The left-hand side of an assignment.
#[derive(Clone, Debug)]
pub(crate) enum IrTarget {
    Var(String),
    /// `base[index] = …`
    Index {
        base: Box<IrExpr>,
        index: Box<IrExpr>,
    },
    /// `base.field = …`
    Field {
        base: Box<IrExpr>,
        name: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug)]
pub(crate) enum IrExpr {
    Unit,
    Bool(bool),
    Int(i64),
    Str(String),
    /// A variable read.
    Var(String),
    Array(Vec<IrExpr>),
    Map(Vec<(String, IrExpr)>),
    Index {
        base: Box<IrExpr>,
        index: Box<IrExpr>,
    },
    /// `base.name` where `name` is a property, not a call.
    Field {
        base: Box<IrExpr>,
        name: String,
    },
    Unary {
        op: UnOp,
        operand: Box<IrExpr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<IrExpr>,
        rhs: Box<IrExpr>,
    },
    /// Short-circuiting.
    And(Box<IrExpr>, Box<IrExpr>),
    Or(Box<IrExpr>, Box<IrExpr>),
    /// `start..end` / `start..=end`.
    Range {
        start: Box<IrExpr>,
        end: Box<IrExpr>,
        inclusive: bool,
    },
    /// A call to a script-defined function or an interpreter builtin.
    Call {
        name: String,
        args: Vec<IrExpr>,
    },
    /// `receiver.name(args)` — core-type methods are interpreter builtins.
    Method {
        receiver: Box<IrExpr>,
        name: String,
        args: Vec<IrExpr>,
    },
    /// A `{ ... }` block used as an expression; its value is the last
    /// statement's value.
    Block(Vec<IrStmt>),
    /// A namespaced host surface (`std::fs::exists`, `rh::json::parse`).
    /// Routed to `Host::call` with its script spelling. PR-A3 fills these in
    /// via `StdHost`; with a bare host they fail closed.
    HostCall {
        name: String,
        args: Vec<IrExpr>,
    },
}

#[cfg(test)]
mod tests {
    use super::{IrExpr, IrFunction, IrModule, IrStmt, IrTarget};

    /// The reason this module exists. If `IrModule` ever stops being `Send`,
    /// `Engine: Send` (D17) is unsatisfiable and the product cannot be a
    /// library.
    #[test]
    fn ir_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<IrModule>();
        assert_send::<IrFunction>();
        assert_send::<IrStmt>();
        assert_send::<IrExpr>();
        assert_send::<IrTarget>();
    }

    #[test]
    fn function_lookup_is_by_name_and_arity() {
        let module = IrModule {
            functions: vec![IrFunction {
                name: "entry".to_owned(),
                params: Vec::new(),
                body: vec![IrStmt::Expr(IrExpr::Int(42))],
            }],
            main: Vec::new(),
        };
        assert!(module.function("entry", 0).is_some());
        assert!(module.function("entry", 1).is_none());
        assert!(module.has_function("entry"));
        assert!(!module.has_function("missing"));
    }
}
