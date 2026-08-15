use std::collections::HashMap;
use std::ffi::{CString, c_void};

use libloading::Library;

use crate::Dyn;
use crate::error::DynError;
use crate::parse::SExpr;
use crate::value::Value;

const MAX_ARGS: usize = 6;
const MAX_LIBRARY_NAME_BYTES: usize = 255;
const MAX_SYMBOL_NAME_BYTES: usize = 255;

#[derive(Default)]
pub(crate) struct LibraryCache {
    libs: HashMap<String, Library>,
}

impl LibraryCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn load(&mut self, path: &str) -> Result<&Library, DynError> {
        if !self.libs.contains_key(path) {
            // SAFETY: loading a host dynamic library by path; callers supply OS-specific names.
            let lib = unsafe { Library::new(path) }
                .map_err(|e| DynError::Library(format!("{path}: {e}")))?;
            self.libs.insert(path.to_owned(), lib);
        }
        Ok(self.libs.get(path).expect("library just inserted"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SigType {
    Void,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    Ptr,
}

impl SigType {
    fn parse(name: &str) -> Result<Self, DynError> {
        match name {
            "void" => Ok(Self::Void),
            "i8" => Ok(Self::I8),
            "u8" => Ok(Self::U8),
            "i16" => Ok(Self::I16),
            "u16" => Ok(Self::U16),
            "i32" => Ok(Self::I32),
            "u32" => Ok(Self::U32),
            "i64" => Ok(Self::I64),
            "u64" => Ok(Self::U64),
            "ptr" => Ok(Self::Ptr),
            other => Err(DynError::Type(format!(
                "unsupported dlcall type `{other}`; only void/integer/pointer types are supported"
            ))),
        }
    }

    fn parse_arg(name: &str) -> Result<Self, DynError> {
        let ty = Self::parse(name)?;
        if ty == Self::Void {
            return Err(DynError::Type("void cannot be an argument type".into()));
        }
        Ok(ty)
    }
}

#[derive(Clone, Copy)]
struct DynArg(u64);

impl DynArg {
    fn from_value(sig: SigType, value: Value) -> Result<Self, DynError> {
        let bits = match sig {
            SigType::I8 => i64::from(
                i8::try_from(value.as_int().map_err(DynError::Type)?)
                    .map_err(|_| DynError::Type("i8 overflow".into()))?,
            ) as u64,
            SigType::U8 => u64::from(
                u8::try_from(value.as_int().map_err(DynError::Type)?)
                    .map_err(|_| DynError::Type("u8 overflow".into()))?,
            ),
            SigType::I16 => i64::from(
                i16::try_from(value.as_int().map_err(DynError::Type)?)
                    .map_err(|_| DynError::Type("i16 overflow".into()))?,
            ) as u64,
            SigType::U16 => u64::from(
                u16::try_from(value.as_int().map_err(DynError::Type)?)
                    .map_err(|_| DynError::Type("u16 overflow".into()))?,
            ),
            SigType::I32 => i64::from(
                i32::try_from(value.as_int().map_err(DynError::Type)?)
                    .map_err(|_| DynError::Type("i32 overflow".into()))?,
            ) as u64,
            SigType::U32 => u64::from(
                u32::try_from(value.as_int().map_err(DynError::Type)?)
                    .map_err(|_| DynError::Type("u32 overflow".into()))?,
            ),
            SigType::I64 => value.as_int().map_err(DynError::Type)? as u64,
            SigType::U64 => u64::try_from(value.as_int().map_err(DynError::Type)?)
                .map_err(|_| DynError::Type("u64 overflow".into()))?,
            SigType::Ptr => u64::try_from(value.as_ptr().map_err(DynError::Type)?)
                .map_err(|_| DynError::Type("pointer does not fit in u64".into()))?,
            SigType::Void => return Err(DynError::Type("void cannot be an argument type".into())),
        };
        Ok(Self(bits))
    }
}

fn expect_string(expr: &SExpr, what: &str) -> Result<String, DynError> {
    match expr {
        SExpr::Str(s) => Ok(s.clone()),
        other => Err(DynError::Type(format!(
            "{what} must be a string, got {other:?}"
        ))),
    }
}

/// Evaluate `(dlcall lib sym rettype [argtype arg]...)`.
///
/// This intentionally supports only fixed, non-variadic C ABI calls with at most six
/// integer/pointer arguments. Floating-point and aggregate ABI classes are rejected.
pub(crate) fn eval_dlcall(env: &mut Dyn, args: &[SExpr]) -> Result<Value, DynError> {
    if args.len() < 3 {
        return Err(DynError::Arity {
            form: "dlcall",
            expected: 3,
            got: args.len(),
        });
    }
    if !args[3..].len().is_multiple_of(2) {
        return Err(DynError::Type(
            "dlcall expects argtype/arg pairs after return type".into(),
        ));
    }

    let lib_name = expect_string(&args[0], "dlcall library")?;
    if lib_name.is_empty() {
        return Err(DynError::Library("library name must not be empty".into()));
    }
    if lib_name.trim().is_empty() {
        return Err(DynError::Library("library name must not be blank".into()));
    }
    if lib_name.len() > MAX_LIBRARY_NAME_BYTES {
        return Err(DynError::Library(format!(
            "library name exceeds {MAX_LIBRARY_NAME_BYTES}-byte limit"
        )));
    }
    if lib_name.as_bytes().contains(&0) {
        return Err(DynError::Library(
            "library name contains interior NUL".into(),
        ));
    }
    let sym_name = expect_string(&args[1], "dlcall symbol")?;
    if sym_name.is_empty() {
        return Err(DynError::DlCall("symbol name must not be empty".into()));
    }
    if sym_name.trim().is_empty() {
        return Err(DynError::DlCall("symbol name must not be blank".into()));
    }
    if sym_name.len() > MAX_SYMBOL_NAME_BYTES {
        return Err(DynError::DlCall(format!(
            "symbol name exceeds {MAX_SYMBOL_NAME_BYTES}-byte limit"
        )));
    }
    let c_name = CString::new(sym_name.as_str())
        .map_err(|_| DynError::DlCall("symbol name contains interior NUL".into()))?;
    let ret_ty = SigType::parse(&expect_string(&args[2], "dlcall return type")?)?;
    let arg_count = args[3..].len() / 2;
    if arg_count > MAX_ARGS {
        return Err(DynError::DlCall(format!(
            "{arg_count} arguments exceed the fixed limit of {MAX_ARGS}"
        )));
    }

    // Validate the complete signature before evaluating any argument expression. A rejected
    // ABI class must not leave mutations from earlier arguments behind.
    let arg_types = args[3..]
        .chunks_exact(2)
        .map(|pair| SigType::parse_arg(&expect_string(&pair[0], "dlcall argument type")?))
        .collect::<Result<Vec<_>, _>>()?;
    #[cfg(target_os = "macos")]
    let darwin_ioctl = is_darwin_ioctl_signature(&sym_name, ret_ty, &arg_types);
    let dyn_args = arg_types
        .iter()
        .copied()
        .zip(args[3..].chunks_exact(2).map(|pair| &pair[1]))
        .map(|(ty, expr)| {
            let value = crate::eval::eval_expr(env, expr)?;
            DynArg::from_value(ty, value)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let lib = env.libs.load(&lib_name)?;
    // SAFETY: the library remains cached in `env`, so the symbol address stays loaded.
    let func_ptr = unsafe {
        let sym: libloading::Symbol<*const c_void> = lib
            .get(c_name.as_bytes_with_nul())
            .map_err(|e| DynError::DlCall(format!("{sym_name}: {e}")))?;
        *sym
    };
    if func_ptr.is_null() {
        return Err(DynError::DlCall(format!(
            "symbol `{sym_name}` resolved to a null address"
        )));
    }

    #[cfg(target_os = "macos")]
    if darwin_ioctl {
        // SAFETY: the resolved `ioctl` symbol has the one supported Darwin signature below;
        // the script types were validated before argument evaluation. Its variadic ABI differs
        // from the general fixed-arity trampoline.
        return unsafe { invoke_darwin_ioctl(func_ptr, &dyn_args) };
    }

    // SAFETY: callers provide the native symbol's signature. `invoke` supports only the
    // integer/pointer C ABI class and a fixed arity, which were validated above.
    unsafe { invoke(func_ptr, &dyn_args, ret_ty) }
}

#[cfg(target_os = "macos")]
fn is_darwin_ioctl_signature(sym_name: &str, ret_ty: SigType, arg_types: &[SigType]) -> bool {
    sym_name == "ioctl"
        && ret_ty == SigType::I32
        && matches!(
            arg_types,
            [SigType::I32, SigType::U64 | SigType::I32, SigType::Ptr]
        )
}

#[cfg(target_os = "macos")]
unsafe fn invoke_darwin_ioctl(func_ptr: *const c_void, args: &[DynArg]) -> Result<Value, DynError> {
    let [fd, request, pointer] = args else {
        unreachable!("darwin ioctl signature checked before invocation");
    };
    type IoctlFn = unsafe extern "C" fn(i32, u64, ...) -> i32;
    // SAFETY: `func_ptr` was resolved from the requested library as `ioctl`, whose address
    // remains valid while `env.libs` owns the library. The signature gate established the typed
    // `(i32, u64, ptr) -> i32` script contract and this function type supplies Darwin's ABI.
    let ioctl: IoctlFn = unsafe { std::mem::transmute(func_ptr) };
    // SAFETY: `ioctl` and its three arguments satisfy the narrow contract above.
    let result = unsafe { ioctl(fd.0 as i32, request.0, pointer.0 as *mut c_void) };
    Ok(Value::Int(i64::from(result)))
}

macro_rules! call_fixed {
    ($ptr:expr, $args:expr, $ret:ty) => {{
        match $args {
            [] => unsafe { std::mem::transmute::<*const c_void, unsafe extern "C" fn() -> $ret>($ptr)() },
            [a] => unsafe { std::mem::transmute::<*const c_void, unsafe extern "C" fn(u64) -> $ret>($ptr)(a.0) },
            [a, b] => unsafe { std::mem::transmute::<*const c_void, unsafe extern "C" fn(u64, u64) -> $ret>($ptr)(a.0, b.0) },
            [a, b, c] => unsafe { std::mem::transmute::<*const c_void, unsafe extern "C" fn(u64, u64, u64) -> $ret>($ptr)(a.0, b.0, c.0) },
            [a, b, c, d] => unsafe { std::mem::transmute::<*const c_void, unsafe extern "C" fn(u64, u64, u64, u64) -> $ret>($ptr)(a.0, b.0, c.0, d.0) },
            [a, b, c, d, e] => unsafe { std::mem::transmute::<*const c_void, unsafe extern "C" fn(u64, u64, u64, u64, u64) -> $ret>($ptr)(a.0, b.0, c.0, d.0, e.0) },
            [a, b, c, d, e, f] => unsafe { std::mem::transmute::<*const c_void, unsafe extern "C" fn(u64, u64, u64, u64, u64, u64) -> $ret>($ptr)(a.0, b.0, c.0, d.0, e.0, f.0) },
            _ => unreachable!("arity checked before invoke"),
        }
    }};
}

unsafe fn invoke(
    func_ptr: *const c_void,
    args: &[DynArg],
    ret_ty: SigType,
) -> Result<Value, DynError> {
    if args.len() > MAX_ARGS {
        return Err(DynError::DlCall(format!(
            "{} arguments exceed the fixed limit of {MAX_ARGS}",
            args.len()
        )));
    }
    let value = match ret_ty {
        SigType::Void => {
            call_fixed!(func_ptr, args, ());
            Value::Nil
        }
        SigType::I8 => Value::Int(i64::from(call_fixed!(func_ptr, args, i8))),
        SigType::U8 => Value::Int(i64::from(call_fixed!(func_ptr, args, u8))),
        SigType::I16 => Value::Int(i64::from(call_fixed!(func_ptr, args, i16))),
        SigType::U16 => Value::Int(i64::from(call_fixed!(func_ptr, args, u16))),
        SigType::I32 => Value::Int(i64::from(call_fixed!(func_ptr, args, i32))),
        SigType::U32 => Value::Int(i64::from(call_fixed!(func_ptr, args, u32))),
        SigType::I64 => Value::Int(call_fixed!(func_ptr, args, i64)),
        SigType::U64 => Value::Int(
            i64::try_from(call_fixed!(func_ptr, args, u64))
                .map_err(|_| DynError::DlCall("u64 return does not fit in i64".into()))?,
        ),
        SigType::Ptr => Value::Ptr(call_fixed!(func_ptr, args, *mut c_void) as usize),
    };
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn sum_six(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> u64 {
        a + b + c + d + e + f
    }

    #[test]
    fn invoke_accepts_every_supported_arity() {
        unsafe extern "C" fn forty_two() -> i32 {
            42
        }
        unsafe extern "C" fn one(a: u64) -> u64 {
            a
        }
        unsafe extern "C" fn two(a: u64, b: u64) -> u64 {
            a + b
        }
        unsafe extern "C" fn three(a: u64, b: u64, c: u64) -> u64 {
            a + b + c
        }
        unsafe extern "C" fn four(a: u64, b: u64, c: u64, d: u64) -> u64 {
            a + b + c + d
        }
        unsafe extern "C" fn five(a: u64, b: u64, c: u64, d: u64, e: u64) -> u64 {
            a + b + c + d + e
        }

        let no_args = unsafe { invoke(forty_two as *const c_void, &[], SigType::I32) };
        assert_eq!(no_args.expect("zero-argument call"), Value::Int(42));

        let args = [
            DynArg(1),
            DynArg(2),
            DynArg(3),
            DynArg(4),
            DynArg(5),
            DynArg(6),
        ];
        for (func, arity, expected) in [
            (one as *const c_void, 1, 1),
            (two as *const c_void, 2, 3),
            (three as *const c_void, 3, 6),
            (four as *const c_void, 4, 10),
            (five as *const c_void, 5, 15),
        ] {
            let result = unsafe { invoke(func, &args[..arity], SigType::U64) };
            assert_eq!(result.expect("fixed-arity call"), Value::Int(expected));
        }
        let six_args = unsafe { invoke(sum_six as *const c_void, &args, SigType::U64) };
        assert_eq!(six_args.expect("six-argument call"), Value::Int(21));
    }

    #[test]
    fn invoke_rejects_unchecked_arity_without_calling() {
        let args = [DynArg(0); MAX_ARGS + 1];
        let result = unsafe { invoke(sum_six as *const c_void, &args, SigType::U64) };
        assert_eq!(
            result,
            Err(DynError::DlCall(
                "7 arguments exceed the fixed limit of 6".into()
            ))
        );
    }
}
