//! `rhai::AST` → crate-private [`IrModule`] (design §6).
//!
//! This is the **only** module allowed to walk rhai nodes for the product
//! path (in-tree `validate_ast` is the other rhai walker, on the AgenTerm
//! side). Walking rhai here is an implementation tactic, not a freeze
//! surface: `rhai::*` never appears in the public API, and the AST is dropped
//! as soon as this returns.
//!
//! Anything outside Language 1 is reported as `Error::Unsupported` rather
//! than silently mis-executed.

use rhai::{ASTFlags, Expr, Stmt, Token};

use crate::ir::{BinOp, IrExpr, IrFunction, IrModule, IrStmt, IrTarget, UnOp};
use crate::lang_error::Error;

/// Lower a validated AST. The caller drops the `AST` immediately after.
pub(crate) fn lower_ast(ast: &rhai::AST) -> Result<IrModule, Error> {
    let mut module = IrModule::default();
    for def in ast.iter_fn_def() {
        module.functions.push(IrFunction {
            name: def.name.to_string(),
            params: def.params.iter().map(|param| param.to_string()).collect(),
            body: lower_block(def.body.iter())?,
        });
    }
    module.main = lower_block(ast.statements().iter())?;
    Ok(module)
}

fn lower_block<'a>(stmts: impl Iterator<Item = &'a Stmt>) -> Result<Vec<IrStmt>, Error> {
    stmts.map(lower_stmt).collect()
}

fn lower_stmt(stmt: &Stmt) -> Result<IrStmt, Error> {
    match stmt {
        Stmt::Expr(expr) => Ok(IrStmt::Expr(lower_expr(expr.as_ref())?)),
        Stmt::Noop(..) => Ok(IrStmt::Expr(IrExpr::Unit)),
        // rhai encodes `throw x` as `Stmt::Return` with `ASTFlags::BREAK`
        // set; a plain `return x` has it clear. `subset.rs`'s
        // `block_has_invalid_return` relies on the same distinction.
        Stmt::Return(expr, flags, ..) => {
            let value = expr.as_ref().map(|e| lower_expr(e)).transpose()?;
            if flags.contains(ASTFlags::BREAK) {
                Ok(IrStmt::Throw(value.unwrap_or(IrExpr::Unit)))
            } else {
                Ok(IrStmt::Return(value))
            }
        }
        Stmt::Var(boxed, ..) => {
            let (ident, value, _) = boxed.as_ref();
            Ok(IrStmt::Let {
                name: ident.name.to_string(),
                value: lower_expr(value)?,
            })
        }
        Stmt::Assignment(boxed, ..) => {
            let (op, binary) = boxed.as_ref();
            let target = lower_target(&binary.lhs)?;
            let op = match op.get_op_assignment_info() {
                Some((.., syntax, _, _)) => Some(compound_op(syntax)?),
                None => None,
            };
            Ok(IrStmt::Assign {
                target,
                op,
                value: lower_expr(&binary.rhs)?,
            })
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            Ok(IrStmt::If {
                cond: lower_expr(&flow.expr)?,
                then_block: lower_block(flow.body.iter())?,
                else_block: lower_block(flow.branch.iter())?,
            })
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            Ok(IrStmt::While {
                cond: lower_expr(&flow.expr)?,
                body: lower_block(flow.body.iter())?,
            })
        }
        Stmt::For(boxed, ..) => {
            let (counter, index, flow) = boxed.as_ref();
            if index.is_some() {
                // `for x, i in ...` binds a second index variable; not Language 1.
                return Err(unsupported("for-index-binding"));
            }
            Ok(IrStmt::For {
                var: counter.name.to_string(),
                iterable: lower_expr(&flow.expr)?,
                body: lower_block(flow.body.iter())?,
            })
        }
        Stmt::TryCatch(boxed, ..) => {
            let flow = boxed.as_ref();
            let catch_var = match &flow.expr {
                Expr::Unit(..) => None,
                Expr::Variable(ident, ..) => Some(ident.1.to_string()),
                _ => return Err(unsupported("catch-binding")),
            };
            Ok(IrStmt::TryCatch {
                body: lower_block(flow.body.iter())?,
                catch_var,
                catch_block: lower_block(flow.branch.iter())?,
            })
        }
        Stmt::Block(block) => Ok(IrStmt::Block(lower_block(block.iter())?)),
        Stmt::BreakLoop(value, flags, ..) => {
            if value.is_some() {
                return Err(unsupported("break-with-value"));
            }
            if flags.contains(ASTFlags::BREAK) {
                Ok(IrStmt::Break)
            } else {
                Ok(IrStmt::Continue)
            }
        }
        Stmt::FnCall(call, ..) if call.name == "throw" => {
            let value = match call.args.first() {
                Some(arg) => lower_expr(arg)?,
                None => IrExpr::Unit,
            };
            Ok(IrStmt::Throw(value))
        }
        Stmt::FnCall(call, ..) => Ok(IrStmt::Expr(lower_call(call)?)),
        other => Err(unsupported(stmt_kind(other))),
    }
}

fn lower_target(expr: &Expr) -> Result<IrTarget, Error> {
    match expr {
        Expr::Variable(ident, ..) => Ok(IrTarget::Var(ident.1.to_string())),
        Expr::Index(boxed, ..) => Ok(IrTarget::Index {
            base: Box::new(lower_expr(&boxed.lhs)?),
            index: Box::new(lower_expr(&boxed.rhs)?),
        }),
        Expr::Dot(boxed, ..) => match &boxed.rhs {
            Expr::Property(prop, ..) => Ok(IrTarget::Field {
                base: Box::new(lower_expr(&boxed.lhs)?),
                name: prop.2.to_string(),
            }),
            // `a.b.c = ...` — nest through the intermediate field.
            Expr::Dot(..) | Expr::Index(..) => Ok(IrTarget::Field {
                base: Box::new(lower_expr(&boxed.lhs)?),
                name: nested_target_name(&boxed.rhs)?,
            }),
            _ => Err(unsupported("assign-target")),
        },
        _ => Err(unsupported("assign-target")),
    }
}

/// `a.b.c = v` arrives as Dot(a, Dot(b, c)); Language 1 keeps assignment
/// targets shallow, so anything deeper than one field is refused rather than
/// silently writing to the wrong place.
fn nested_target_name(expr: &Expr) -> Result<String, Error> {
    let _ = expr;
    Err(unsupported("nested-assign-target"))
}

fn compound_op(syntax: &str) -> Result<BinOp, Error> {
    Ok(match syntax {
        "+=" => BinOp::Add,
        "-=" => BinOp::Sub,
        "*=" => BinOp::Mul,
        "/=" => BinOp::Div,
        "%=" => BinOp::Rem,
        other => return Err(Error::unsupported(format!("compound-assign:{other}"))),
    })
}

fn lower_expr(expr: &Expr) -> Result<IrExpr, Error> {
    match expr {
        Expr::Unit(..) => Ok(IrExpr::Unit),
        Expr::BoolConstant(value, ..) => Ok(IrExpr::Bool(*value)),
        Expr::IntegerConstant(value, ..) => Ok(IrExpr::Int(*value)),
        Expr::StringConstant(value, ..) => Ok(IrExpr::Str(value.to_string())),
        // Language 1 maps `Char` to a one-scalar `String` (value-model table).
        Expr::CharConstant(value, ..) => Ok(IrExpr::Str(value.to_string())),
        Expr::Variable(ident, ..) => Ok(IrExpr::Var(ident.1.to_string())),
        Expr::Array(items, ..) => Ok(IrExpr::Array(
            items.iter().map(lower_expr).collect::<Result<_, _>>()?,
        )),
        Expr::Map(map, ..) => {
            let mut entries = Vec::with_capacity(map.0.len());
            for (key, value) in map.0.iter() {
                entries.push((key.name.to_string(), lower_expr(value)?));
            }
            Ok(IrExpr::Map(entries))
        }
        Expr::Index(boxed, ..) => Ok(IrExpr::Index {
            base: Box::new(lower_expr(&boxed.lhs)?),
            index: Box::new(lower_expr(&boxed.rhs)?),
        }),
        Expr::Dot(boxed, ..) => lower_dot(&boxed.lhs, &boxed.rhs),
        Expr::And(args, ..) => lower_nary_logical(args, true),
        Expr::Or(args, ..) => lower_nary_logical(args, false),
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => lower_call(call),
        Expr::Stmt(block) => Ok(IrExpr::Block(lower_block(block.iter())?)),
        other => Err(unsupported(expr_kind(other))),
    }
}

fn lower_nary_logical(args: &[Expr], is_and: bool) -> Result<IrExpr, Error> {
    let mut iter = args.iter();
    let first = iter.next().ok_or_else(|| unsupported("empty-logical"))?;
    let mut acc = lower_expr(first)?;
    for arg in iter {
        let rhs = lower_expr(arg)?;
        acc = if is_and {
            IrExpr::And(Box::new(acc), Box::new(rhs))
        } else {
            IrExpr::Or(Box::new(acc), Box::new(rhs))
        };
    }
    Ok(acc)
}

fn lower_dot(lhs: &Expr, rhs: &Expr) -> Result<IrExpr, Error> {
    match rhs {
        Expr::Property(prop, ..) => Ok(IrExpr::Field {
            base: Box::new(lower_expr(lhs)?),
            name: prop.2.to_string(),
        }),
        Expr::MethodCall(call, ..) => Ok(IrExpr::Method {
            receiver: Box::new(lower_expr(lhs)?),
            name: call.name.to_string(),
            args: call.args.iter().map(lower_expr).collect::<Result<_, _>>()?,
        }),
        // `a.b.c` / `a.b[i]` — chain left-to-right.
        Expr::Dot(inner, ..) => {
            let base = lower_dot(lhs, &inner.lhs)?;
            lower_dot_tail(base, &inner.rhs)
        }
        Expr::Index(inner, ..) => {
            let base = lower_dot(lhs, &inner.lhs)?;
            Ok(IrExpr::Index {
                base: Box::new(base),
                index: Box::new(lower_expr(&inner.rhs)?),
            })
        }
        other => Err(unsupported(expr_kind(other))),
    }
}

fn lower_dot_tail(base: IrExpr, rhs: &Expr) -> Result<IrExpr, Error> {
    match rhs {
        Expr::Property(prop, ..) => Ok(IrExpr::Field {
            base: Box::new(base),
            name: prop.2.to_string(),
        }),
        Expr::MethodCall(call, ..) => Ok(IrExpr::Method {
            receiver: Box::new(base),
            name: call.name.to_string(),
            args: call.args.iter().map(lower_expr).collect::<Result<_, _>>()?,
        }),
        other => Err(unsupported(expr_kind(other))),
    }
}

fn lower_call(call: &rhai::FnCallExpr) -> Result<IrExpr, Error> {
    if let Some(op) = &call.op_token {
        return lower_operator(op, &call.args);
    }
    let args: Vec<IrExpr> = call.args.iter().map(lower_expr).collect::<Result<_, _>>()?;
    if call.namespace.is_empty() {
        Ok(IrExpr::Call {
            name: call.name.to_string(),
            args,
        })
    } else {
        // `std::fs::exists(..)` / `rh::json::parse(..)`: the frozen script
        // spelling is the `Host::call` name (Language 1 §3).
        Ok(IrExpr::HostCall {
            name: format!("{}::{}", call.namespace, call.name),
            args,
        })
    }
}

fn lower_operator(op: &Token, args: &[Expr]) -> Result<IrExpr, Error> {
    let binary = |op: BinOp| -> Result<IrExpr, Error> {
        Ok(IrExpr::Binary {
            op,
            lhs: Box::new(lower_expr(&args[0])?),
            rhs: Box::new(lower_expr(&args[1])?),
        })
    };
    match (op, args.len()) {
        (Token::Plus, 2) => binary(BinOp::Add),
        (Token::Minus, 2) => binary(BinOp::Sub),
        (Token::Multiply, 2) => binary(BinOp::Mul),
        (Token::Divide, 2) => binary(BinOp::Div),
        (Token::Modulo, 2) => binary(BinOp::Rem),
        (Token::Equals, 2) | (Token::EqualsTo, 2) => binary(BinOp::Eq),
        (Token::NotEqualsTo, 2) => binary(BinOp::Ne),
        (Token::LessThan, 2) => binary(BinOp::Lt),
        (Token::LessThanEqualsTo, 2) => binary(BinOp::Le),
        (Token::GreaterThan, 2) => binary(BinOp::Gt),
        (Token::GreaterThanEqualsTo, 2) => binary(BinOp::Ge),
        (Token::And, 2) => Ok(IrExpr::And(
            Box::new(lower_expr(&args[0])?),
            Box::new(lower_expr(&args[1])?),
        )),
        (Token::Or, 2) => Ok(IrExpr::Or(
            Box::new(lower_expr(&args[0])?),
            Box::new(lower_expr(&args[1])?),
        )),
        (Token::ExclusiveRange, 2) => Ok(IrExpr::Range {
            start: Box::new(lower_expr(&args[0])?),
            end: Box::new(lower_expr(&args[1])?),
            inclusive: false,
        }),
        (Token::InclusiveRange, 2) => Ok(IrExpr::Range {
            start: Box::new(lower_expr(&args[0])?),
            end: Box::new(lower_expr(&args[1])?),
            inclusive: true,
        }),
        (Token::Minus, 1) => Ok(IrExpr::Unary {
            op: UnOp::Neg,
            operand: Box::new(lower_expr(&args[0])?),
        }),
        (Token::Bang, 1) => Ok(IrExpr::Unary {
            op: UnOp::Not,
            operand: Box::new(lower_expr(&args[0])?),
        }),
        _ => Err(Error::unsupported(format!("operator:{op:?}"))),
    }
}

fn unsupported(kind: &str) -> Error {
    Error::unsupported(format!("lower:{kind}"))
}

fn stmt_kind(stmt: &Stmt) -> &'static str {
    match stmt {
        Stmt::Do(..) => "do",
        Stmt::Switch(..) => "switch",
        Stmt::Share(..) => "closure-capture",
        Stmt::Import(..) => "import",
        Stmt::Export(..) => "export",
        _ => "stmt",
    }
}

fn expr_kind(expr: &Expr) -> &'static str {
    match expr {
        Expr::FloatConstant(..) => "float",
        Expr::InterpolatedString(..) => "interpolation",
        Expr::Property(..) => "property",
        Expr::ThisPtr(..) => "this",
        _ => "expr",
    }
}

#[cfg(test)]
mod tests {
    use super::lower_ast;
    use crate::check::parse_rh_ast;
    use crate::ir::{IrExpr, IrStmt};
    use crate::lang_error::Error;

    fn lower(source: &str) -> crate::ir::IrModule {
        let ast = parse_rh_ast(source).expect("parse");
        let module = lower_ast(&ast).expect("lower");
        // The AST is dropped here, before the IR is used: that is the D17
        // contract in miniature.
        drop(ast);
        module
    }

    #[test]
    fn lowers_entry_returning_an_int() {
        let module = lower("fn entry() { 42 }");
        let entry = module.function("entry", 0).expect("entry");
        assert!(matches!(
            entry.body.as_slice(),
            [IrStmt::Expr(IrExpr::Int(42))]
        ));
    }

    #[test]
    fn lowers_let_if_while_for() {
        for source in [
            "fn entry() { let x = 1; x }",
            "fn entry() { if true { 1 } else { 2 } }",
            "fn entry() { let n = 0; while n < 2 { n += 1; } n }",
            "fn entry() { let n = 0; for i in 1..3 { n += i; } n }",
        ] {
            let module = lower(source);
            assert!(module.function("entry", 0).is_some(), "{source}");
        }
    }

    /// `throw` and `return` share `Stmt::Return` in rhai and are told apart by
    /// `ASTFlags::BREAK`. Getting this backwards makes `try/catch` silently
    /// return the thrown value instead of running the catch block.
    #[test]
    fn throw_and_return_lower_differently() {
        let module = lower("fn entry() { return 1; }");
        assert!(matches!(
            module.function("entry", 0).expect("entry").body.as_slice(),
            [IrStmt::Return(Some(IrExpr::Int(1)))]
        ));

        let module = lower("fn entry() { throw 1; }");
        assert!(matches!(
            module.function("entry", 0).expect("entry").body.as_slice(),
            [IrStmt::Throw(IrExpr::Int(1))]
        ));
    }

    #[test]
    fn namespaced_calls_become_host_calls_with_their_script_spelling() {
        let module = lower(r#"fn entry() { std::fs::exists("/tmp") }"#);
        let entry = module.function("entry", 0).expect("entry");
        match entry.body.as_slice() {
            [IrStmt::Expr(IrExpr::HostCall { name, .. })] => {
                assert_eq!(name, "std::fs::exists");
            }
            other => panic!("unexpected lowering: {other:?}"),
        }
    }

    /// Constructs outside Language 1 are refused rather than guessed at.
    #[test]
    fn out_of_language_constructs_are_refused() {
        let ast = parse_rh_ast("fn entry() { 1.5 }").expect("parse");
        let error = lower_ast(&ast).expect_err("floats are not Language 1");
        assert!(
            matches!(&error, Error::Unsupported { feature } if feature == "lower:float"),
            "{error}"
        );
    }
}
