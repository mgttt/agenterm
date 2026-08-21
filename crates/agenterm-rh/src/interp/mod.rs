//! The default tree-walk backend over the owned IR (design D8).
//!
//! PR-A2 scope: Language-1 syntax over **pure values**. The core-type method
//! surface (`String` / `Array` / `Map` / `Bytes`) is implemented **here**, as
//! interpreter builtins — it deliberately does **not** go through
//! `Host::call`, so a program that only manipulates values runs correctly on
//! a host that implements nothing. Host surfaces (`std::fs::*`, `rh::json::*`)
//! are routed to `Host::call` and fail closed until PR-A3 lands `StdHost`.

mod methods;

use crate::backend::{Backend, Limits, Scope};
use crate::host::Host;
use crate::ir::{BinOp, IrExpr, IrModule, IrStmt, IrTarget};
use crate::lang_error::Error;
use crate::value::Value;

/// How far a block got before it stopped.
enum Flow {
    /// Ran to the end; carries the last statement's value.
    Normal(Value),
    Return(Value),
    Break,
    Continue,
}

/// A `throw`n value travelling to the nearest `catch`.
struct Thrown(Value);

#[derive(Debug, Default)]
pub(crate) struct InterpBackend;

/// Per-run mutable state.
struct Run<'a> {
    ir: &'a IrModule,
    host: &'a mut dyn Host,
    limits: &'a Limits,
    fuel: u64,
    depth: usize,
}

/// A lexical frame: `Vec` rather than a map because Language-1 frames are
/// small and insertion order matters for shadowing.
#[derive(Default)]
struct Frame {
    vars: Vec<(String, Value)>,
}

impl Frame {
    fn get(&self, name: &str) -> Option<&Value> {
        self.vars
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.vars
            .iter_mut()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    fn declare(&mut self, name: String, value: Value) {
        self.vars.push((name, value));
    }

    fn len(&self) -> usize {
        self.vars.len()
    }

    fn truncate(&mut self, len: usize) {
        self.vars.truncate(len);
    }
}

impl Backend for InterpBackend {
    fn eval(
        &mut self,
        ir: &IrModule,
        scope: &mut Scope,
        host: &mut dyn Host,
        limits: &Limits,
    ) -> Result<Value, Error> {
        let mut run = Run {
            ir,
            host,
            limits,
            fuel: limits.fuel.unwrap_or(u64::MAX),
            depth: 0,
        };
        let mut frame = Frame::default();
        for (name, value) in scope.entries() {
            frame.declare(name.clone(), value.clone());
        }

        // `entry()` wins over top-level statements (D21).
        let body = match ir.function("entry", 0) {
            Some(entry) => &entry.body,
            None => {
                if ir.has_function("entry") {
                    return Err(Error::runtime("fn entry() must take no parameters"));
                }
                &ir.main
            }
        };
        match run.block(body, &mut frame) {
            Ok(Flow::Normal(value) | Flow::Return(value)) => Ok(value),
            Ok(Flow::Break | Flow::Continue) => {
                Err(Error::runtime("break/continue outside a loop"))
            }
            Err(error) => Err(error.into_error()),
        }
    }
}

/// Either a real error or an in-flight `throw`.
enum Raise {
    Error(Error),
    Thrown(Thrown),
}

impl Raise {
    fn into_error(self) -> Error {
        match self {
            Self::Error(error) => error,
            Self::Thrown(Thrown(value)) => {
                Error::runtime(format!("uncaught throw: {}", methods::display(&value)))
            }
        }
    }
}

impl From<Error> for Raise {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

type Exec<T> = Result<T, Raise>;

impl Run<'_> {
    /// Charge one unit of fuel and honour cancellation.
    fn tick(&mut self) -> Exec<()> {
        if self.limits.cancel.is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        if self.limits.fuel.is_some() {
            if self.fuel == 0 {
                return Err(Error::OutOfFuel.into());
            }
            self.fuel -= 1;
        }
        Ok(())
    }

    fn block(&mut self, stmts: &[IrStmt], frame: &mut Frame) -> Exec<Flow> {
        let mark = frame.len();
        let flow = self.block_inner(stmts, frame);
        frame.truncate(mark);
        flow
    }

    fn block_inner(&mut self, stmts: &[IrStmt], frame: &mut Frame) -> Exec<Flow> {
        let mut last = Value::Unit;
        for stmt in stmts {
            self.tick()?;
            match self.stmt(stmt, frame)? {
                Flow::Normal(value) => last = value,
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal(last))
    }

    fn stmt(&mut self, stmt: &IrStmt, frame: &mut Frame) -> Exec<Flow> {
        match stmt {
            IrStmt::Expr(expr) => Ok(Flow::Normal(self.expr(expr, frame)?)),
            IrStmt::Let { name, value } => {
                let value = self.expr(value, frame)?;
                frame.declare(name.clone(), value);
                Ok(Flow::Normal(Value::Unit))
            }
            IrStmt::Assign { target, op, value } => {
                let value = self.expr(value, frame)?;
                self.assign(target, *op, value, frame)?;
                Ok(Flow::Normal(Value::Unit))
            }
            IrStmt::If {
                cond,
                then_block,
                else_block,
            } => {
                if self.condition(cond, frame)? {
                    self.block(then_block, frame)
                } else {
                    self.block(else_block, frame)
                }
            }
            IrStmt::While { cond, body } => {
                while self.condition(cond, frame)? {
                    self.tick()?;
                    match self.block(body, frame)? {
                        Flow::Break => break,
                        Flow::Return(value) => return Ok(Flow::Return(value)),
                        Flow::Normal(_) | Flow::Continue => {}
                    }
                }
                Ok(Flow::Normal(Value::Unit))
            }
            IrStmt::For {
                var,
                iterable,
                body,
            } => self.run_for(var, iterable, body, frame),
            IrStmt::TryCatch {
                body,
                catch_var,
                catch_block,
            } => match self.block(body, frame) {
                Err(Raise::Thrown(Thrown(value))) => {
                    let mark = frame.len();
                    if let Some(name) = catch_var {
                        frame.declare(name.clone(), value);
                    }
                    let flow = self.block_inner(catch_block, frame);
                    frame.truncate(mark);
                    flow
                }
                other => other,
            },
            IrStmt::Block(stmts) => self.block(stmts, frame),
            IrStmt::Return(value) => {
                let value = match value {
                    Some(expr) => self.expr(expr, frame)?,
                    None => Value::Unit,
                };
                Ok(Flow::Return(value))
            }
            IrStmt::Break => Ok(Flow::Break),
            IrStmt::Continue => Ok(Flow::Continue),
            IrStmt::Throw(expr) => {
                let value = self.expr(expr, frame)?;
                Err(Raise::Thrown(Thrown(value)))
            }
        }
    }

    fn run_for(
        &mut self,
        var: &str,
        iterable: &IrExpr,
        body: &[IrStmt],
        frame: &mut Frame,
    ) -> Exec<Flow> {
        let items = self.iterable(iterable, frame)?;
        for item in items {
            self.tick()?;
            let mark = frame.len();
            frame.declare(var.to_owned(), item);
            let flow = self.block_inner(body, frame);
            frame.truncate(mark);
            match flow? {
                Flow::Break => break,
                Flow::Return(value) => return Ok(Flow::Return(value)),
                Flow::Normal(_) | Flow::Continue => {}
            }
        }
        Ok(Flow::Normal(Value::Unit))
    }

    /// Language 1 iterates ranges and arrays. A map is not iterable; use
    /// `.keys()`.
    fn iterable(&mut self, expr: &IrExpr, frame: &mut Frame) -> Exec<Vec<Value>> {
        if let IrExpr::Range {
            start,
            end,
            inclusive,
        } = expr
        {
            let start = self.int(start, frame)?;
            let end = self.int(end, frame)?;
            let end = if *inclusive {
                end.saturating_add(1)
            } else {
                end
            };
            let mut out = Vec::new();
            let mut current = start;
            while current < end {
                out.push(Value::Int(current));
                current += 1;
            }
            return Ok(out);
        }
        match self.expr(expr, frame)? {
            Value::Array(items) => Ok(items),
            other => Err(Error::runtime(format!(
                "`for` needs a range or array, got {}",
                other.type_name()
            ))
            .into()),
        }
    }

    fn condition(&mut self, expr: &IrExpr, frame: &mut Frame) -> Exec<bool> {
        let value = self.expr(expr, frame)?;
        value.as_bool().ok_or_else(|| {
            Raise::Error(Error::runtime(format!(
                "condition must be a bool, got {}",
                value.type_name()
            )))
        })
    }

    fn int(&mut self, expr: &IrExpr, frame: &mut Frame) -> Exec<i64> {
        let value = self.expr(expr, frame)?;
        value.as_int().ok_or_else(|| {
            Raise::Error(Error::runtime(format!(
                "expected an int, got {}",
                value.type_name()
            )))
        })
    }

    fn assign(
        &mut self,
        target: &IrTarget,
        op: Option<BinOp>,
        value: Value,
        frame: &mut Frame,
    ) -> Exec<()> {
        match target {
            IrTarget::Var(name) => {
                let next = match op {
                    Some(op) => {
                        let current = frame.get(name).cloned().ok_or_else(|| undefined(name))?;
                        methods::binary(op, &current, &value)?
                    }
                    None => value,
                };
                match frame.get_mut(name) {
                    Some(slot) => {
                        *slot = next;
                        Ok(())
                    }
                    None => Err(undefined(name)),
                }
            }
            IrTarget::Index { base, index } => {
                let index = self.expr(index, frame)?;
                let name = base_var(base)?;
                let current = frame.get(&name).cloned().ok_or_else(|| undefined(&name))?;
                let updated = methods::set_index(current, &index, op, value)?;
                *frame.get_mut(&name).ok_or_else(|| undefined(&name))? = updated;
                Ok(())
            }
            IrTarget::Field { base, name: field } => {
                let name = base_var(base)?;
                let current = frame.get(&name).cloned().ok_or_else(|| undefined(&name))?;
                let updated =
                    methods::set_index(current, &Value::String(field.clone()), op, value)?;
                *frame.get_mut(&name).ok_or_else(|| undefined(&name))? = updated;
                Ok(())
            }
        }
    }

    fn expr(&mut self, expr: &IrExpr, frame: &mut Frame) -> Exec<Value> {
        self.tick()?;
        match expr {
            IrExpr::Unit => Ok(Value::Unit),
            IrExpr::Bool(value) => Ok(Value::Bool(*value)),
            IrExpr::Int(value) => Ok(Value::Int(*value)),
            IrExpr::Str(value) => Ok(Value::String(value.clone())),
            IrExpr::Var(name) => frame.get(name).cloned().ok_or_else(|| undefined(name)),
            IrExpr::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.expr(item, frame)?);
                }
                Ok(Value::Array(out))
            }
            IrExpr::Map(entries) => {
                let mut out = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    let value = self.expr(value, frame)?;
                    out.push((key.clone(), value));
                }
                Ok(Value::Map(out))
            }
            IrExpr::Index { base, index } => {
                // `args[i]` is `Host::arg`, not an ordinary index.
                if is_args(base, frame) {
                    let i = self.int(index, frame)?;
                    let i = u32::try_from(i)
                        .map_err(|_| Error::runtime("`args` index must be non-negative"))?;
                    return Ok(Value::String(self.host.arg(i)?));
                }
                let base = self.expr(base, frame)?;
                let index = self.expr(index, frame)?;
                Ok(methods::index(&base, &index)?)
            }
            IrExpr::Field { base, name } => {
                // `args.len` is `Host::args_len`.
                if is_args(base, frame) && name == "len" {
                    return Ok(Value::Int(self.host.args_len()?));
                }
                // A dotted chain rooted at an unbound identifier is a
                // host surface in **dot form** (`fleet.tabs.list`). This is
                // the seam AgenTerm's adapter uses to inject Fleet in PR-D1;
                // `StdHost` refuses these by name.
                if let Some(path) = dotted_path(base, frame) {
                    return Ok(self.host.call(&format!("{path}.{name}"), &[])?);
                }
                let base = self.expr(base, frame)?;
                // A property read on a host object is a `Host::call` with the
                // frozen `Type.property` spelling (`DirEntry.file_name`).
                if let Value::Host(object) = &base {
                    let name = host_member(object.type_id(), name);
                    return Ok(self.host.call(&name, std::slice::from_ref(&base))?);
                }
                Ok(methods::field(&base, name)?)
            }
            IrExpr::Unary { op, operand } => {
                let value = self.expr(operand, frame)?;
                Ok(methods::unary(*op, &value)?)
            }
            IrExpr::Binary { op, lhs, rhs } => {
                let lhs = self.expr(lhs, frame)?;
                let rhs = self.expr(rhs, frame)?;
                Ok(methods::binary(*op, &lhs, &rhs)?)
            }
            IrExpr::And(lhs, rhs) => {
                if self.condition(lhs, frame)? {
                    Ok(Value::Bool(self.condition(rhs, frame)?))
                } else {
                    Ok(Value::Bool(false))
                }
            }
            IrExpr::Or(lhs, rhs) => {
                if self.condition(lhs, frame)? {
                    Ok(Value::Bool(true))
                } else {
                    Ok(Value::Bool(self.condition(rhs, frame)?))
                }
            }
            IrExpr::Range { .. } => {
                Err(Error::runtime("a range is only valid as a `for` iterable").into())
            }
            IrExpr::Block(stmts) => match self.block(stmts, frame)? {
                Flow::Normal(value) => Ok(value),
                _ => Err(Error::runtime("control flow escaped a block expression").into()),
            },
            IrExpr::Method {
                receiver,
                name,
                args,
            } => {
                let mut evaluated = Vec::with_capacity(args.len());
                for arg in args {
                    evaluated.push(self.expr(arg, frame)?);
                }
                // `a.push(x)` must grow `a`, not produce a discarded copy.
                if methods::is_mutating(name) {
                    let var = match receiver.as_ref() {
                        IrExpr::Var(name) => name.clone(),
                        _ => {
                            return Err(Error::runtime(format!(
                                "`{name}` needs a variable receiver"
                            ))
                            .into());
                        }
                    };
                    let current = frame.get(&var).cloned().ok_or_else(|| undefined(&var))?;
                    let (result, updated) = methods::call_mutating(&current, name, &evaluated)?;
                    *frame.get_mut(&var).ok_or_else(|| undefined(&var))? = updated;
                    return Ok(result);
                }
                if let Some(path) = dotted_path(receiver, frame) {
                    return Ok(self.host.call(&format!("{path}.{name}"), &evaluated)?);
                }
                let receiver = self.expr(receiver, frame)?;
                if let Value::Host(object) = &receiver {
                    let call_name = host_member(object.type_id(), name);
                    let mut call_args = Vec::with_capacity(evaluated.len() + 1);
                    call_args.push(receiver.clone());
                    call_args.extend(evaluated);
                    return Ok(self.host.call(&call_name, &call_args)?);
                }
                Ok(methods::call_method(&receiver, name, &evaluated)?)
            }
            IrExpr::Call { name, args } => {
                let mut evaluated = Vec::with_capacity(args.len());
                for arg in args {
                    evaluated.push(self.expr(arg, frame)?);
                }
                self.call(name, evaluated, frame)
            }
            // `rh::fail` is frozen as a *builtin* (Language 1 §3 "Builtins"),
            // not a host surface, so it works without a host.
            //
            // SEMANTIC NOTE: under AOT this is `RH_HOST_UTILITY_FAIL`, which
            // records a host error and evaluates to the sentinel int `-5`
            // (`src/script_rh_host.rs`). A pure-value interpreter has no host
            // error channel, so it raises `Error::Host` instead. That is a
            // deliberate divergence from the AOT sentinel and is flagged for
            // an owner ruling; the design freezes the *name*, not the value.
            IrExpr::HostCall { name, args } if name == "rh::fail" => {
                let mut evaluated = Vec::with_capacity(args.len());
                for arg in args {
                    evaluated.push(self.expr(arg, frame)?);
                }
                let message = evaluated.first().map(methods::display).unwrap_or_default();
                Err(Error::Host(message).into())
            }
            IrExpr::HostCall { name, args } => {
                let mut evaluated = Vec::with_capacity(args.len());
                for arg in args {
                    evaluated.push(self.expr(arg, frame)?);
                }
                Ok(self.host.call(name, &evaluated)?)
            }
        }
    }

    fn call(&mut self, name: &str, args: Vec<Value>, frame: &mut Frame) -> Exec<Value> {
        // A script-defined function wins over a builtin of the same name.
        if let Some(def) = self.ir.function(name, args.len()) {
            if self.depth >= MAX_CALL_DEPTH {
                return Err(Error::runtime("call depth exceeded").into());
            }
            let mut callee = Frame::default();
            for (param, value) in def.params.iter().zip(args) {
                callee.declare(param.clone(), value);
            }
            self.depth += 1;
            let flow = self.block(&def.body, &mut callee);
            self.depth -= 1;
            return match flow? {
                Flow::Normal(value) | Flow::Return(value) => Ok(value),
                Flow::Break | Flow::Continue => {
                    Err(Error::runtime("break/continue outside a loop").into())
                }
            };
        }
        if self.ir.has_function(name) {
            return Err(Error::runtime(format!("`{name}` called with wrong arity")).into());
        }
        self.builtin(name, args, frame)
    }

    /// The Language-1 builtins that are not core-type methods.
    fn builtin(&mut self, name: &str, args: Vec<Value>, _frame: &mut Frame) -> Exec<Value> {
        match (name, args.len()) {
            ("print", 1) => {
                self.host.print(&methods::display(&args[0]))?;
                Ok(Value::Unit)
            }
            ("debug", 1) => {
                self.host.print(&methods::debug(&args[0]))?;
                Ok(Value::Unit)
            }
            ("type_of", 1) => Ok(Value::String(args[0].type_name().to_owned())),
            ("to_string", 1) => Ok(Value::String(methods::display(&args[0]))),
            ("to_debug", 1) => Ok(Value::String(methods::debug(&args[0]))),
            _ => Err(Error::unsupported_name(name).into()),
        }
    }
}

const MAX_CALL_DEPTH: usize = 64;

/// `args` is a host-provided array-like, not a variable — unless the program
/// shadowed it with a real binding, in which case the binding wins.
fn is_args(expr: &IrExpr, frame: &Frame) -> bool {
    matches!(expr, IrExpr::Var(name) if name == "args") && frame.get("args").is_none()
}

/// Render a chain of `Var` + `Field` as a dotted path, but only when the root
/// identifier is **not** a bound variable — an unbound root means the chain is
/// a host surface (`fleet.tabs.list`), not a value access.
fn dotted_path(expr: &IrExpr, frame: &Frame) -> Option<String> {
    match expr {
        IrExpr::Var(name) => {
            if name == "args" || frame.get(name).is_some() {
                None
            } else {
                Some(name.clone())
            }
        }
        IrExpr::Field { base, name } => {
            let prefix = dotted_path(base, frame)?;
            Some(format!("{prefix}.{name}"))
        }
        _ => None,
    }
}

/// The frozen `Type.member` spelling for a host object, derived from its
/// `type_id`: `"std.fs.DirEntry"` + `"file_name"` -> `"DirEntry.file_name"`.
fn host_member(type_id: &str, member: &str) -> String {
    let short = type_id.rsplit('.').next().unwrap_or(type_id);
    format!("{short}.{member}")
}

fn undefined(name: &str) -> Raise {
    Raise::Error(Error::runtime(format!("undefined variable `{name}`")))
}

/// Assignment through an index or field needs a named base binding; Language 1
/// does not have references.
fn base_var(expr: &IrExpr) -> Result<String, Raise> {
    match expr {
        IrExpr::Var(name) => Ok(name.clone()),
        _ => Err(Raise::Error(Error::runtime(
            "assignment target must start from a variable",
        ))),
    }
}
