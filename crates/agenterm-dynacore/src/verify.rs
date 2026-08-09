//! Well-formedness gate — ported from `research/dynamic-core/verify/verify.rs`
//! (Q19), adapted to the single-intent IR (`ir::Inst::FleetCall` replaces
//! `ir::Inst::Call` + `ir::Intent`) and extended with the fix
//! `research/dynamic-core/assembled/RESULTS.md` §② **F1** found and
//! deliberately left open there: Q19's verifier only checked IR-internal
//! consistency (a call's declared arity matches its own extern decl) and had
//! no notion of the host's own operation catalog, so a `FleetCall` naming an
//! operation the host doesn't recognize — or one whose `params_json` doesn't
//! match that operation's declared parameters — passed `verify()` and would
//! only be caught (or, per F1, panic) at the seam boundary at runtime. This
//! file closes that gap at produce time, which is exactly what F1 named as
//! the missing "seam-specific validator" a production system would need.
//!
//! Discipline carried over from Q19: no abstract interpretation, no value-range
//! tracking beyond what's needed to catch out-of-range indices, no dataflow —
//! one structural walk, plus (new) one operation-catalog lookup and one
//! parameter-schema check per `FleetCall` site.

use crate::ir::*;

/// A structural fault in an IR. Each variant is a produce-time, execution-free
/// finding — a well-formed IR can still fail at runtime (e.g. the fleet
/// operation itself returns an error), `verify()` makes no claim about that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrFault {
    /// no blocks at all — nothing to enter
    NoBlocks,
    /// entry block index >= blocks.len()
    EntryOutOfRange { entry: u32 },
    /// a Val id referenced or defined is >= n_vals
    ValOutOfRange { block: usize, val: Val },
    /// a Br/BrCond names a block index that does not exist
    BlockTargetOutOfRange { block: usize, target: u32 },
    /// a FleetCall names an extern_id that is not in the extern table
    ExternIdOutOfRange { block: usize, id: u32 },
    /// F1 fix: FleetCall's operation_id is absent from the host's operation
    /// catalog, or present but marked unavailable.
    UnknownOperation {
        block: usize,
        id: u32,
        operation_id: String,
    },
    /// F1 fix: FleetCall's params_json is not a JSON object, or does not
    /// satisfy the operation's declared parameter schema (missing required
    /// param, wrong JSON type, out of declared min/max, or a param name the
    /// operation does not declare).
    ParamsMismatch {
        block: usize,
        id: u32,
        operation_id: String,
        reason: String,
    },
}

/// Generic mirror of the host's per-parameter schema (`OperationParameterSpec`
/// in `src/operations.rs`). This crate owns no product name (same posture as
/// `crates/agenterm-platform`) and does not depend on the root `agenterm`
/// crate, so it cannot reference `OPERATION_CATALOG`'s real type — the host
/// binding (`src/script_dynacore_host.rs`) converts it into this shape once
/// and passes the result in.
#[derive(Clone, Debug)]
pub struct OperationParamSchema {
    pub name: String,
    /// One of the value_type strings the host catalog uses today
    /// (`"string"`, `"uint32"`, `"uint64"`, `"integer"`, `"number"`, or a
    /// product-specific string-shaped type like `"stable_tab_id"`). Any type
    /// name not recognized here is treated as unconstrained (accept any JSON
    /// value) rather than rejected, so a future catalog type addition does
    /// not turn into a spurious verify() failure for this crate to chase.
    pub value_type: String,
    pub required: bool,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
}

/// Generic mirror of the host's `OperationSpec` (`src/operations.rs`), the
/// slice `verify()` checks each `FleetCall` against.
#[derive(Clone, Debug)]
pub struct OperationSchema {
    pub id: String,
    pub available: bool,
    pub parameters: Vec<OperationParamSchema>,
}

/// A module that has PASSED structural verification. Its inner `&Module` is
/// PRIVATE and the ONLY constructor is `verify` — the un-forgettable
/// construction gate `research/dynamic-core/verify/verify.rs` (Q19)
/// established, unchanged here.
pub struct VerifiedModule<'a>(&'a Module);

impl<'a> VerifiedModule<'a> {
    pub fn module(&self) -> &Module {
        self.0
    }
}

/// Structurally verify a module against the host's operation catalog.
/// Produce-time, no execution, one pass. Returns a `VerifiedModule` (the
/// capability to run it) or the first `IrFault` found.
pub fn verify<'a>(m: &'a Module, catalog: &[OperationSchema]) -> Result<VerifiedModule<'a>, IrFault> {
    let nblk = m.blocks.len();
    if nblk == 0 {
        return Err(IrFault::NoBlocks);
    }
    if m.entry as usize >= nblk {
        return Err(IrFault::EntryOutOfRange { entry: m.entry });
    }
    let nv = m.n_vals;
    for (b, blk) in m.blocks.iter().enumerate() {
        for inst in &blk.insts {
            match inst {
                Inst::Set(d, op) => {
                    chk(*d, b, nv)?;
                    each_read(op, |v| chk(v, b, nv))?;
                }
                Inst::FleetCall(d, id) => {
                    chk(*d, b, nv)?;
                    let Some(extern_decl) = m.externs.get(*id as usize) else {
                        return Err(IrFault::ExternIdOutOfRange { block: b, id: *id });
                    };
                    let schema = catalog
                        .iter()
                        .find(|op| op.id == extern_decl.operation_id && op.available);
                    let Some(schema) = schema else {
                        return Err(IrFault::UnknownOperation {
                            block: b,
                            id: *id,
                            operation_id: extern_decl.operation_id.clone(),
                        });
                    };
                    if let Err(reason) = validate_params(schema, &extern_decl.params_json) {
                        return Err(IrFault::ParamsMismatch {
                            block: b,
                            id: *id,
                            operation_id: extern_decl.operation_id.clone(),
                            reason,
                        });
                    }
                }
            }
        }
        match &blk.term {
            Term::Br(t) => tgt(*t, b, nblk)?,
            Term::BrCond(c, nz, z) => {
                chk(*c, b, nv)?;
                tgt(*nz, b, nblk)?;
                tgt(*z, b, nblk)?;
            }
            Term::Ret(v) | Term::Exit(v) => chk(*v, b, nv)?,
        }
    }
    Ok(VerifiedModule(m))
}

/// Check `params_json` is a JSON object satisfying `schema`'s declared
/// parameters: every required param present with a type-matching value,
/// every present param's value type matches its declared `value_type` (when
/// recognized), declared `minimum`/`maximum` bounds hold for integer-typed
/// params, and no param name is present that the operation does not declare.
fn validate_params(schema: &OperationSchema, params_json: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(params_json)
        .map_err(|e| format!("params_json is not valid JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "params_json must be a JSON object".to_string())?;

    for key in object.keys() {
        if !schema.parameters.iter().any(|p| &p.name == key) {
            return Err(format!(
                "params_json has key {key:?} which operation {:?} does not declare",
                schema.id
            ));
        }
    }

    for param in &schema.parameters {
        match object.get(param.name.as_str()) {
            Some(v) => check_param_value(param, v)?,
            None if param.required => {
                return Err(format!("missing required param {:?}", param.name));
            }
            None => {}
        }
    }
    Ok(())
}

fn check_param_value(param: &OperationParamSchema, value: &serde_json::Value) -> Result<(), String> {
    if !json_type_matches(&param.value_type, value) {
        return Err(format!(
            "param {:?} has JSON type {} but operation declares value_type {:?}",
            param.name,
            json_type_name(value),
            param.value_type
        ));
    }
    if let Some(n) = value.as_i64() {
        if let Some(minimum) = param.minimum
            && n < minimum
        {
            return Err(format!(
                "param {:?} = {n} is below declared minimum {minimum}",
                param.name
            ));
        }
        if let Some(maximum) = param.maximum
            && n > maximum
        {
            return Err(format!(
                "param {:?} = {n} is above declared maximum {maximum}",
                param.name
            ));
        }
    }
    Ok(())
}

/// Whether `value`'s JSON type is compatible with the catalog's `value_type`
/// string. Unrecognized `value_type` strings are treated as unconstrained
/// (see `OperationParamSchema::value_type` doc) rather than rejected.
fn json_type_matches(value_type: &str, value: &serde_json::Value) -> bool {
    match value_type {
        "string" | "stable_tab_id" | "session_name" => value.is_string(),
        "bool" | "boolean" => value.is_boolean(),
        "uint32" | "uint64" => value.is_u64(),
        "integer" | "int32" | "int64" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        _ => true,
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[inline]
fn chk(v: Val, b: usize, nv: u32) -> Result<(), IrFault> {
    if v >= nv {
        Err(IrFault::ValOutOfRange { block: b, val: v })
    } else {
        Ok(())
    }
}

#[inline]
fn tgt(t: u32, b: usize, nblk: usize) -> Result<(), IrFault> {
    if t as usize >= nblk {
        Err(IrFault::BlockTargetOutOfRange { block: b, target: t })
    } else {
        Ok(())
    }
}

/// Apply `f` to every Val an op reads. (Const reads no Vals.)
#[inline]
fn each_read(op: &Op, mut f: impl FnMut(Val) -> Result<(), IrFault>) -> Result<(), IrFault> {
    match op {
        Op::Const(_) => Ok(()),
        Op::Add(a, b) | Op::Sub(a, b) | Op::Mul(a, b) | Op::Xor(a, b) | Op::And(a, b) | Op::Or(a, b) | Op::Ult(a, b) => {
            f(*a)?;
            f(*b)
        }
        Op::Shl(a, _) | Op::Shr(a, _) => f(*a),
    }
}
