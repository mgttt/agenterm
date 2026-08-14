use std::collections::HashMap;
use std::ffi::{CString, c_void};

use libffi::high::CodePtr;
use libffi::middle::{Cif, Type, arg};
use libloading::Library;

use crate::Dyn;
use crate::error::DynError;
use crate::parse::SExpr;
use crate::value::Value;

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
            // SAFETY: loading a host dynamic library by path; callers supply OS-specific names as script data.
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
            other => Err(DynError::Type(format!("unknown ffi type `{other}`"))),
        }
    }

    fn libffi(self) -> Type {
        match self {
            Self::Void => Type::void(),
            Self::I8 => Type::i8(),
            Self::U8 => Type::u8(),
            Self::I16 => Type::i16(),
            Self::U16 => Type::u16(),
            Self::I32 => Type::i32(),
            Self::U32 => Type::u32(),
            Self::I64 => Type::i64(),
            Self::U64 => Type::u64(),
            Self::Ptr => Type::pointer(),
        }
    }
}

struct DynArg {
    storage: ArgStorage,
}

enum ArgStorage {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Ptr(*mut c_void),
}

impl DynArg {
    fn from_value(sig: SigType, value: Value) -> Result<Self, DynError> {
        let storage = match sig {
            SigType::I8 => ArgStorage::I8(
                value
                    .as_int()
                    .map_err(DynError::Type)?
                    .try_into()
                    .map_err(|_| DynError::Type("i8 overflow".into()))?,
            ),
            SigType::U8 => ArgStorage::U8(
                value
                    .as_int()
                    .map_err(DynError::Type)?
                    .try_into()
                    .map_err(|_| DynError::Type("u8 overflow".into()))?,
            ),
            SigType::I16 => ArgStorage::I16(
                value
                    .as_int()
                    .map_err(DynError::Type)?
                    .try_into()
                    .map_err(|_| DynError::Type("i16 overflow".into()))?,
            ),
            SigType::U16 => ArgStorage::U16(
                value
                    .as_int()
                    .map_err(DynError::Type)?
                    .try_into()
                    .map_err(|_| DynError::Type("u16 overflow".into()))?,
            ),
            SigType::I32 => ArgStorage::I32(
                value
                    .as_int()
                    .map_err(DynError::Type)?
                    .try_into()
                    .map_err(|_| DynError::Type("i32 overflow".into()))?,
            ),
            SigType::U32 => ArgStorage::U32(
                value
                    .as_int()
                    .map_err(DynError::Type)?
                    .try_into()
                    .map_err(|_| DynError::Type("u32 overflow".into()))?,
            ),
            SigType::I64 => ArgStorage::I64(value.as_int().map_err(DynError::Type)?),
            SigType::U64 => ArgStorage::U64(
                value
                    .as_int()
                    .map_err(DynError::Type)?
                    .try_into()
                    .map_err(|_| DynError::Type("u64 overflow".into()))?,
            ),
            SigType::Ptr => ArgStorage::Ptr(value.as_ptr().map_err(DynError::Type)? as *mut c_void),
            SigType::Void => return Err(DynError::Type("void cannot be an argument type".into())),
        };
        Ok(Self { storage })
    }

    fn libffi_arg(&self) -> libffi::middle::Arg<'_> {
        match &self.storage {
            ArgStorage::I8(v) => arg(v),
            ArgStorage::U8(v) => arg(v),
            ArgStorage::I16(v) => arg(v),
            ArgStorage::U16(v) => arg(v),
            ArgStorage::I32(v) => arg(v),
            ArgStorage::U32(v) => arg(v),
            ArgStorage::I64(v) => arg(v),
            ArgStorage::U64(v) => arg(v),
            ArgStorage::Ptr(v) => arg(v),
        }
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
    let sym_name = expect_string(&args[1], "dlcall symbol")?;
    let ret_ty = SigType::parse(&expect_string(&args[2], "dlcall return type")?)?;

    let mut arg_types = Vec::new();
    let mut dyn_args = Vec::new();
    let mut i = 3;
    while i < args.len() {
        let ty = SigType::parse(&expect_string(&args[i], "dlcall argument type")?)?;
        let value = crate::eval::eval_expr(env, &args[i + 1])?;
        arg_types.push(ty);
        dyn_args.push(DynArg::from_value(ty, value)?);
        i += 2;
    }

    let lib = env.libs.load(&lib_name)?;
    let c_name = CString::new(sym_name.as_str())
        .map_err(|_| DynError::DlCall("symbol name contains interior NUL".into()))?;
    // SAFETY: resolving a symbol from a loaded library; pointer validity is the library's contract.
    let func_ptr: *mut c_void = unsafe {
        let sym: libloading::Symbol<*mut c_void> = lib
            .get(c_name.as_bytes())
            .map_err(|e| DynError::DlCall(format!("{sym_name}: {e}")))?;
        *sym
    };

    let arg_types_ffi: Vec<Type> = arg_types.iter().map(|t| t.libffi()).collect();
    let cif = Cif::new(arg_types_ffi, ret_ty.libffi());
    let ffi_args: Vec<_> = dyn_args.iter().map(DynArg::libffi_arg).collect();

    // SAFETY: `func_ptr` came from the platform loader for `sym_name`; `cif` and `ffi_args`
    // were built from the script-provided signature. Wrong signatures are UB — tests lock the
    // happy path; callers are expected to pass correct script data.
    let result = unsafe { invoke(&cif, func_ptr, &ffi_args, ret_ty)? };
    Ok(result)
}

unsafe fn invoke(
    cif: &Cif,
    func_ptr: *mut c_void,
    ffi_args: &[libffi::middle::Arg<'_>],
    ret_ty: SigType,
) -> Result<Value, DynError> {
    let code = CodePtr(func_ptr);
    match ret_ty {
        SigType::Void => {
            unsafe {
                cif.call::<()>(code, ffi_args);
            }
            Ok(Value::Nil)
        }
        SigType::I8 => Ok(Value::Int(i64::from(unsafe {
            cif.call::<i8>(code, ffi_args)
        }))),
        SigType::U8 => Ok(Value::Int(i64::from(unsafe {
            cif.call::<u8>(code, ffi_args)
        }))),
        SigType::I16 => Ok(Value::Int(i64::from(unsafe {
            cif.call::<i16>(code, ffi_args)
        }))),
        SigType::U16 => Ok(Value::Int(i64::from(unsafe {
            cif.call::<u16>(code, ffi_args)
        }))),
        SigType::I32 => Ok(Value::Int(i64::from(unsafe {
            cif.call::<i32>(code, ffi_args)
        }))),
        SigType::U32 => Ok(Value::Int(i64::from(unsafe {
            cif.call::<u32>(code, ffi_args)
        }))),
        SigType::I64 => Ok(Value::Int(unsafe { cif.call::<i64>(code, ffi_args) })),
        SigType::U64 => Ok(Value::Int(
            i64::try_from(unsafe { cif.call::<u64>(code, ffi_args) })
                .map_err(|_| DynError::DlCall("u64 return does not fit in i64".into()))?,
        )),
        SigType::Ptr => {
            let p: *mut c_void = unsafe { cif.call(code, ffi_args) };
            Ok(Value::Ptr(p as usize))
        }
    }
}
