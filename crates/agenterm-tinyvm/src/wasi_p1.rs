//! Optional, small WASI Preview 1 adapter over [`crate::host`].
//!
//! This is not enabled by default and is not part of TinyArcade's game ABI.
//! It binds only the explicitly implemented `wasi_snapshot_preview1` imports;
//! an unknown import or a wrong standard signature fails before instantiation.

use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Cell, Ref, RefCell, RefMut};

use crate::{
    DescriptorRights, GuestFd, HostBackend, HostClock, HostContext, HostError, OpenOptions, Val,
    ValueType, WasmError, WasmModule,
};

pub const WASI_SNAPSHOT_PREVIEW1: &str = "wasi_snapshot_preview1";
pub const WASI_PROC_EXIT_TRAP: &str = "wasi preview1 proc_exit";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasiErrno(pub u16);

impl WasiErrno {
    pub const SUCCESS: Self = Self(0);
    pub const TOO_BIG: Self = Self(1);
    pub const ACCES: Self = Self(2);
    pub const BADF: Self = Self(8);
    pub const EXIST: Self = Self(20);
    pub const FAULT: Self = Self(21);
    pub const INVAL: Self = Self(28);
    pub const IO: Self = Self(29);
    pub const ISDIR: Self = Self(31);
    pub const MFILE: Self = Self(33);
    pub const NAMETOOLONG: Self = Self(37);
    pub const NOENT: Self = Self(44);
    pub const NOMEM: Self = Self(48);
    pub const NOTDIR: Self = Self(54);
    pub const NOSYS: Self = Self(52);
    pub const OVERFLOW: Self = Self(61);
    pub const NOTCAPABLE: Self = Self(76);
}

impl From<HostError> for WasiErrno {
    fn from(error: HostError) -> Self {
        match error {
            HostError::AllocationFailed => Self::NOMEM,
            HostError::AlreadyExists => Self::EXIST,
            HostError::BadHandle => Self::BADF,
            HostError::Invalid => Self::INVAL,
            HostError::InvalidPath | HostError::NotCapable => Self::NOTCAPABLE,
            HostError::IsDirectory => Self::ISDIR,
            HostError::Io => Self::IO,
            HostError::NotDirectory => Self::NOTDIR,
            HostError::NotFound => Self::NOENT,
            HostError::NotSupported => Self::NOSYS,
            HostError::Overflow => Self::OVERFLOW,
            HostError::PermissionDenied => Self::ACCES,
            HostError::ProcessTooLarge => Self::TOO_BIG,
            HostError::TooManyDescriptors | HostError::TooManyPreopens => Self::MFILE,
        }
    }
}

pub struct WasiPreview1<B: HostBackend> {
    context: Rc<RefCell<HostContext<B>>>,
    exit_code: Rc<Cell<Option<u32>>>,
}

impl<B: HostBackend> Clone for WasiPreview1<B> {
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            exit_code: self.exit_code.clone(),
        }
    }
}

impl<B: HostBackend + 'static> WasiPreview1<B> {
    pub fn new(context: HostContext<B>) -> Self {
        Self {
            context: Rc::new(RefCell::new(context)),
            exit_code: Rc::new(Cell::new(None)),
        }
    }

    pub fn try_context(&self) -> Result<Ref<'_, HostContext<B>>, HostError> {
        self.context.try_borrow().map_err(|_| HostError::Io)
    }

    pub fn try_context_mut(&self) -> Result<RefMut<'_, HostContext<B>>, HostError> {
        self.context.try_borrow_mut().map_err(|_| HostError::Io)
    }

    /// Returns the most recent successful `proc_exit` code without consuming it.
    pub fn exit_code(&self) -> Option<u32> {
        self.exit_code.get()
    }

    /// Takes the most recent successful `proc_exit` code.
    pub fn take_exit_code(&self) -> Option<u32> {
        self.exit_code.take()
    }

    /// Validates and binds every WASI P1 import currently implemented here.
    pub fn bind(&self, module: &mut WasmModule) -> Result<(), WasmError> {
        let mut present = [false; Function::COUNT];
        for (position, import) in module.imports().iter().enumerate() {
            if import.module != WASI_SNAPSHOT_PREVIEW1 {
                continue;
            }
            let function = Function::from_name(&import.field)
                .ok_or(WasmError::Trap("unsupported wasi preview1 import"))?;
            function.validate(module, position)?;
            present[function as usize] = true;
        }

        for function in Function::ALL {
            if !present[function as usize] {
                continue;
            }
            let context = self.context.clone();
            if matches!(function, Function::ProcExit) {
                let exit_code = self.exit_code.clone();
                module.bind_import_typed(
                    WASI_SNAPSHOT_PREVIEW1,
                    function.name(),
                    move |args, _memory| proc_exit(&context, &exit_code, args),
                )?;
                continue;
            }
            module.bind_import_typed(
                WASI_SNAPSHOT_PREVIEW1,
                function.name(),
                move |args, memory| {
                    Ok(vec![Val::I32(i32::from(
                        dispatch(function, &context, args, memory).0,
                    ))])
                },
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
#[repr(usize)]
enum Function {
    ArgsGet,
    ArgsSizesGet,
    EnvironGet,
    EnvironSizesGet,
    ClockTimeGet,
    RandomGet,
    FdPrestatGet,
    FdPrestatDirName,
    FdClose,
    FdRead,
    FdWrite,
    FdSeek,
    FdFilestatGet,
    PathOpen,
    PathUnlinkFile,
    ProcExit,
}

impl Function {
    const COUNT: usize = 16;
    const ALL: [Self; Self::COUNT] = [
        Self::ArgsGet,
        Self::ArgsSizesGet,
        Self::EnvironGet,
        Self::EnvironSizesGet,
        Self::ClockTimeGet,
        Self::RandomGet,
        Self::FdPrestatGet,
        Self::FdPrestatDirName,
        Self::FdClose,
        Self::FdRead,
        Self::FdWrite,
        Self::FdSeek,
        Self::FdFilestatGet,
        Self::PathOpen,
        Self::PathUnlinkFile,
        Self::ProcExit,
    ];

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "args_get" => Self::ArgsGet,
            "args_sizes_get" => Self::ArgsSizesGet,
            "environ_get" => Self::EnvironGet,
            "environ_sizes_get" => Self::EnvironSizesGet,
            "clock_time_get" => Self::ClockTimeGet,
            "random_get" => Self::RandomGet,
            "fd_prestat_get" => Self::FdPrestatGet,
            "fd_prestat_dir_name" => Self::FdPrestatDirName,
            "fd_close" => Self::FdClose,
            "fd_read" => Self::FdRead,
            "fd_write" => Self::FdWrite,
            "fd_seek" => Self::FdSeek,
            "fd_filestat_get" => Self::FdFilestatGet,
            "path_open" => Self::PathOpen,
            "path_unlink_file" => Self::PathUnlinkFile,
            "proc_exit" => Self::ProcExit,
            _ => return None,
        })
    }

    const fn name(self) -> &'static str {
        match self {
            Self::ArgsGet => "args_get",
            Self::ArgsSizesGet => "args_sizes_get",
            Self::EnvironGet => "environ_get",
            Self::EnvironSizesGet => "environ_sizes_get",
            Self::ClockTimeGet => "clock_time_get",
            Self::RandomGet => "random_get",
            Self::FdPrestatGet => "fd_prestat_get",
            Self::FdPrestatDirName => "fd_prestat_dir_name",
            Self::FdClose => "fd_close",
            Self::FdRead => "fd_read",
            Self::FdWrite => "fd_write",
            Self::FdSeek => "fd_seek",
            Self::FdFilestatGet => "fd_filestat_get",
            Self::PathOpen => "path_open",
            Self::PathUnlinkFile => "path_unlink_file",
            Self::ProcExit => "proc_exit",
        }
    }

    fn signature(self) -> (&'static [ValueType], &'static [ValueType]) {
        const I32_I32: &[ValueType] = &[ValueType::I32, ValueType::I32];
        const I32_I32_I32: &[ValueType] = &[ValueType::I32, ValueType::I32, ValueType::I32];
        const CLOCK: &[ValueType] = &[ValueType::I32, ValueType::I64, ValueType::I32];
        const FOUR_I32: &[ValueType] = &[
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
        ];
        const SEEK: &[ValueType] = &[
            ValueType::I32,
            ValueType::I64,
            ValueType::I32,
            ValueType::I32,
        ];
        const PATH_OPEN: &[ValueType] = &[
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::I64,
            ValueType::I64,
            ValueType::I32,
            ValueType::I32,
        ];
        const I32: &[ValueType] = &[ValueType::I32];
        const EMPTY: &[ValueType] = &[];
        match self {
            Self::ArgsGet
            | Self::ArgsSizesGet
            | Self::EnvironGet
            | Self::EnvironSizesGet
            | Self::RandomGet
            | Self::FdPrestatGet => (I32_I32, I32),
            Self::FdPrestatDirName => (I32_I32_I32, I32),
            Self::ClockTimeGet => (CLOCK, I32),
            Self::FdClose => (I32, I32),
            Self::FdRead | Self::FdWrite => (FOUR_I32, I32),
            Self::FdSeek => (SEEK, I32),
            Self::FdFilestatGet => (I32_I32, I32),
            Self::PathOpen => (PATH_OPEN, I32),
            Self::PathUnlinkFile => (I32_I32_I32, I32),
            Self::ProcExit => (I32, EMPTY),
        }
    }

    fn validate(self, module: &WasmModule, position: usize) -> Result<(), WasmError> {
        let (params, results) = self.signature();
        let import = &module.imports()[position];
        if import.n_params != params.len() || import.n_results != results.len() {
            return Err(WasmError::Trap("wasi preview1 import signature"));
        }
        for (index, expected) in params.iter().enumerate() {
            if module.import_parameter_type(position, index) != Some(*expected) {
                return Err(WasmError::Trap("wasi preview1 import signature"));
            }
        }
        for (index, expected) in results.iter().enumerate() {
            if module.import_result_type(position, index) != Some(*expected) {
                return Err(WasmError::Trap("wasi preview1 import signature"));
            }
        }
        Ok(())
    }
}

fn dispatch<B: HostBackend>(
    function: Function,
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    match function {
        Function::ArgsSizesGet => sizes_get(context, false, args, memory),
        Function::EnvironSizesGet => sizes_get(context, true, args, memory),
        Function::ArgsGet => strings_get(context, false, args, memory),
        Function::EnvironGet => strings_get(context, true, args, memory),
        Function::ClockTimeGet => clock_time_get(context, args, memory),
        Function::RandomGet => random_get(context, args, memory),
        Function::FdPrestatGet => fd_prestat_get(context, args, memory),
        Function::FdPrestatDirName => fd_prestat_dir_name(context, args, memory),
        Function::FdClose => fd_close(context, args),
        Function::FdRead => fd_read(context, args, memory),
        Function::FdWrite => fd_write(context, args, memory),
        Function::FdSeek => fd_seek(context, args, memory),
        Function::FdFilestatGet => fd_filestat_get(context, args, memory),
        Function::PathOpen => path_open(context, args, memory),
        Function::PathUnlinkFile => path_unlink_file(context, args, memory),
        Function::ProcExit => WasiErrno::NOSYS,
    }
}

fn sizes_get<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    environment: bool,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let Some((count_ptr, bytes_ptr)) = two_i32(args) else {
        return WasiErrno::INVAL;
    };
    let Ok(context) = context.try_borrow() else {
        return WasiErrno::IO;
    };
    let values = if environment {
        context.environ()
    } else {
        context.args()
    };
    let Ok(count) = u32::try_from(values.len()) else {
        return WasiErrno::OVERFLOW;
    };
    let Ok(bytes) = string_bytes(values) else {
        return WasiErrno::OVERFLOW;
    };
    if memory_range(memory, count_ptr, 4).is_none() || memory_range(memory, bytes_ptr, 4).is_none()
    {
        return WasiErrno::FAULT;
    }
    let _ = write_u32(memory, count_ptr, count);
    let _ = write_u32(memory, bytes_ptr, bytes);
    WasiErrno::SUCCESS
}

fn strings_get<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    environment: bool,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let Some((table_ptr, buffer_ptr)) = two_i32(args) else {
        return WasiErrno::INVAL;
    };
    let Ok(context) = context.try_borrow() else {
        return WasiErrno::IO;
    };
    let values = if environment {
        context.environ()
    } else {
        context.args()
    };
    let Ok(table_bytes) = values
        .len()
        .checked_mul(4)
        .ok_or(())
        .and_then(|n| u32::try_from(n).map_err(|_| ()))
    else {
        return WasiErrno::OVERFLOW;
    };
    let Ok(buffer_bytes) = string_bytes(values) else {
        return WasiErrno::OVERFLOW;
    };
    if memory_range(memory, table_ptr, table_bytes).is_none()
        || memory_range(memory, buffer_ptr, buffer_bytes).is_none()
    {
        return WasiErrno::FAULT;
    }

    let mut cursor = buffer_ptr;
    for (index, value) in values.iter().enumerate() {
        let Some(pointer_slot) = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_mul(4))
            .and_then(|offset| table_ptr.checked_add(offset))
        else {
            return WasiErrno::OVERFLOW;
        };
        if write_u32(memory, pointer_slot, cursor).is_none() {
            return WasiErrno::FAULT;
        }
        let bytes = value.as_bytes();
        let Ok(length) = u32::try_from(bytes.len()) else {
            return WasiErrno::OVERFLOW;
        };
        let Some(length_with_nul) = length.checked_add(1) else {
            return WasiErrno::OVERFLOW;
        };
        let Some(output) = memory_range_mut(memory, cursor, length_with_nul) else {
            return WasiErrno::FAULT;
        };
        output[..bytes.len()].copy_from_slice(bytes);
        output[bytes.len()] = 0;
        let Some(next) = cursor.checked_add(length_with_nul) else {
            return WasiErrno::OVERFLOW;
        };
        cursor = next;
    }
    WasiErrno::SUCCESS
}

fn clock_time_get<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let [Val::I32(raw_clock), Val::I64(_precision), Val::I32(pointer)] = args else {
        return WasiErrno::INVAL;
    };
    let clock = match raw_clock {
        0 => HostClock::Realtime,
        1 => HostClock::Monotonic,
        2 => HostClock::ProcessCpu,
        3 => HostClock::ThreadCpu,
        _ => return WasiErrno::INVAL,
    };
    if memory_range(memory, *pointer as u32, 8).is_none() {
        return WasiErrno::FAULT;
    }
    let Ok(mut context) = context.try_borrow_mut() else {
        return WasiErrno::IO;
    };
    let value = match context.clock_now(clock) {
        Ok(value) => value,
        Err(error) => return error.into(),
    };
    if write_u64(memory, *pointer as u32, value).is_none() {
        return WasiErrno::FAULT;
    }
    WasiErrno::SUCCESS
}

fn random_get<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let Some((pointer, length)) = two_i32(args) else {
        return WasiErrno::INVAL;
    };
    let Some(output) = memory_range_mut(memory, pointer, length) else {
        return WasiErrno::FAULT;
    };
    let Ok(mut context) = context.try_borrow_mut() else {
        return WasiErrno::IO;
    };
    match context.random_fill(output) {
        Ok(()) => WasiErrno::SUCCESS,
        Err(error) => error.into(),
    }
}

fn fd_prestat_get<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let Some((fd, pointer)) = two_i32(args) else {
        return WasiErrno::INVAL;
    };
    let Ok(context) = context.try_borrow() else {
        return WasiErrno::IO;
    };
    let name = match context.preopen_name(GuestFd::new(fd)) {
        Ok(Some(name)) => name,
        Ok(None) => return WasiErrno::BADF,
        Err(error) => return error.into(),
    };
    let Ok(length) = u32::try_from(name.len()) else {
        return WasiErrno::OVERFLOW;
    };
    let Some(output) = memory_range_mut(memory, pointer, 8) else {
        return WasiErrno::FAULT;
    };
    output.fill(0);
    output[4..8].copy_from_slice(&length.to_le_bytes());
    WasiErrno::SUCCESS
}

fn fd_prestat_dir_name<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let [Val::I32(fd), Val::I32(pointer), Val::I32(length)] = args else {
        return WasiErrno::INVAL;
    };
    let Ok(context) = context.try_borrow() else {
        return WasiErrno::IO;
    };
    let name = match context.preopen_name(GuestFd::new(*fd as u32)) {
        Ok(Some(name)) => name,
        Ok(None) => return WasiErrno::BADF,
        Err(error) => return error.into(),
    };
    let Ok(name_length) = u32::try_from(name.len()) else {
        return WasiErrno::OVERFLOW;
    };
    if (*length as u32) < name_length {
        return WasiErrno::NAMETOOLONG;
    }
    let Some(output) = memory_range_mut(memory, *pointer as u32, *length as u32) else {
        return WasiErrno::FAULT;
    };
    output[..name.len()].copy_from_slice(name.as_bytes());
    WasiErrno::SUCCESS
}

fn fd_close<B: HostBackend>(context: &Rc<RefCell<HostContext<B>>>, args: &[Val]) -> WasiErrno {
    let [Val::I32(fd)] = args else {
        return WasiErrno::INVAL;
    };
    let Ok(mut context) = context.try_borrow_mut() else {
        return WasiErrno::IO;
    };
    match context.fd_close(GuestFd::new(*fd as u32)) {
        Ok(()) => WasiErrno::SUCCESS,
        Err(error) => error.into(),
    }
}

const MAX_IOVECS: u32 = 64;

fn fd_read<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let [
        Val::I32(fd),
        Val::I32(iovs),
        Val::I32(iov_count),
        Val::I32(result),
    ] = args
    else {
        return WasiErrno::INVAL;
    };
    let iov_count = *iov_count as u32;
    if memory_range(memory, *result as u32, 4).is_none() {
        return WasiErrno::FAULT;
    }
    let iovecs = match snapshot_iovecs(memory, *iovs as u32, iov_count) {
        Ok(iovecs) => iovecs,
        Err(error) => return error,
    };
    let Ok(mut context) = context.try_borrow_mut() else {
        return WasiErrno::IO;
    };
    let mut total = 0u32;
    for (pointer, length) in iovecs {
        let Some(output) = memory_range_mut(memory, pointer, length) else {
            return WasiErrno::FAULT;
        };
        let count = match context.fd_read(GuestFd::new(*fd as u32), output) {
            Ok(count) if count <= output.len() => count,
            Ok(_) => return WasiErrno::IO,
            Err(error) => return error.into(),
        };
        let Ok(count) = u32::try_from(count) else {
            return WasiErrno::OVERFLOW;
        };
        let Some(next) = total.checked_add(count) else {
            return WasiErrno::OVERFLOW;
        };
        total = next;
        if count < length {
            break;
        }
    }
    let _ = write_u32(memory, *result as u32, total);
    WasiErrno::SUCCESS
}

fn fd_write<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let [
        Val::I32(fd),
        Val::I32(iovs),
        Val::I32(iov_count),
        Val::I32(result),
    ] = args
    else {
        return WasiErrno::INVAL;
    };
    let iov_count = *iov_count as u32;
    if memory_range(memory, *result as u32, 4).is_none() {
        return WasiErrno::FAULT;
    }
    let iovecs = match snapshot_iovecs(memory, *iovs as u32, iov_count) {
        Ok(iovecs) => iovecs,
        Err(error) => return error,
    };
    let Ok(mut context) = context.try_borrow_mut() else {
        return WasiErrno::IO;
    };
    let mut total = 0u32;
    for (pointer, length) in iovecs {
        let Some(input) = memory_range(memory, pointer, length) else {
            return WasiErrno::FAULT;
        };
        let count = match context.fd_write(GuestFd::new(*fd as u32), input) {
            Ok(count) if count <= input.len() => count,
            Ok(_) => return WasiErrno::IO,
            Err(error) => return error.into(),
        };
        let Ok(count) = u32::try_from(count) else {
            return WasiErrno::OVERFLOW;
        };
        let Some(next) = total.checked_add(count) else {
            return WasiErrno::OVERFLOW;
        };
        total = next;
        if count < length {
            break;
        }
    }
    let _ = write_u32(memory, *result as u32, total);
    WasiErrno::SUCCESS
}

fn fd_seek<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let [
        Val::I32(fd),
        Val::I64(offset),
        Val::I32(raw_whence),
        Val::I32(result),
    ] = args
    else {
        return WasiErrno::INVAL;
    };
    if memory_range(memory, *result as u32, 8).is_none() {
        return WasiErrno::FAULT;
    }
    let whence = match raw_whence {
        0 => crate::SeekWhence::Start,
        1 => crate::SeekWhence::Current,
        2 => crate::SeekWhence::End,
        _ => return WasiErrno::INVAL,
    };
    let Ok(mut context) = context.try_borrow_mut() else {
        return WasiErrno::IO;
    };
    let value = match context.fd_seek(GuestFd::new(*fd as u32), *offset, whence) {
        Ok(value) => value,
        Err(error) => return error.into(),
    };
    let _ = write_u64(memory, *result as u32, value);
    WasiErrno::SUCCESS
}

fn fd_filestat_get<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let Some((fd, result)) = two_i32(args) else {
        return WasiErrno::INVAL;
    };
    if memory_range(memory, result, 64).is_none() {
        return WasiErrno::FAULT;
    }
    let Ok(mut context) = context.try_borrow_mut() else {
        return WasiErrno::IO;
    };
    let stat = match context.fd_stat(GuestFd::new(fd)) {
        Ok(stat) => stat,
        Err(error) => return error.into(),
    };
    let Some(output) = memory_range_mut(memory, result, 64) else {
        return WasiErrno::FAULT;
    };
    output.fill(0);
    output[16] = match stat.file_type {
        crate::FileType::Unknown => 0,
        crate::FileType::BlockDevice => 1,
        crate::FileType::CharacterDevice => 2,
        crate::FileType::Directory => 3,
        crate::FileType::RegularFile => 4,
        crate::FileType::Socket => 6,
        crate::FileType::SymbolicLink => 7,
    };
    output[32..40].copy_from_slice(&stat.size.to_le_bytes());
    WasiErrno::SUCCESS
}

const WASI_RIGHT_FD_READ: u64 = 1 << 1;
const WASI_RIGHT_FD_SEEK: u64 = 1 << 2;
const WASI_RIGHT_FD_WRITE: u64 = 1 << 6;
const WASI_RIGHT_FD_FILESTAT_GET: u64 = 1 << 21;
const WASI_SUPPORTED_FILE_RIGHTS: u64 =
    WASI_RIGHT_FD_READ | WASI_RIGHT_FD_SEEK | WASI_RIGHT_FD_WRITE | WASI_RIGHT_FD_FILESTAT_GET;
const WASI_OFLAG_CREATE: u32 = 1;
const WASI_OFLAG_DIRECTORY: u32 = 2;
const WASI_OFLAG_EXCLUSIVE: u32 = 4;
const WASI_OFLAG_TRUNCATE: u32 = 8;
const WASI_KNOWN_OFLAGS: u32 =
    WASI_OFLAG_CREATE | WASI_OFLAG_DIRECTORY | WASI_OFLAG_EXCLUSIVE | WASI_OFLAG_TRUNCATE;

fn path_open<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &mut [u8],
) -> WasiErrno {
    let [
        Val::I32(fd),
        Val::I32(dirflags),
        Val::I32(path_pointer),
        Val::I32(path_length),
        Val::I32(oflags),
        Val::I64(rights_base),
        Val::I64(rights_inheriting),
        Val::I32(fdflags),
        Val::I32(result),
    ] = args
    else {
        return WasiErrno::INVAL;
    };
    if memory_range(memory, *result as u32, 4).is_none() {
        return WasiErrno::FAULT;
    }
    if *dirflags != 0 || *fdflags != 0 || *rights_inheriting != 0 {
        return WasiErrno::NOSYS;
    }
    let oflags = *oflags as u32;
    if oflags & !WASI_KNOWN_OFLAGS != 0 {
        return WasiErrno::INVAL;
    }
    if oflags & WASI_OFLAG_EXCLUSIVE != 0 {
        return WasiErrno::NOSYS;
    }
    let rights_base = *rights_base as u64;
    let Some(rights) = descriptor_rights(rights_base) else {
        return WasiErrno::NOSYS;
    };
    let Some(path_bytes) = memory_range(memory, *path_pointer as u32, *path_length as u32) else {
        return WasiErrno::FAULT;
    };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return WasiErrno::INVAL;
    };
    let options = OpenOptions {
        create: oflags & WASI_OFLAG_CREATE != 0,
        directory: oflags & WASI_OFLAG_DIRECTORY != 0,
        read: rights.contains(DescriptorRights::READ),
        truncate: oflags & WASI_OFLAG_TRUNCATE != 0,
        write: rights.contains(DescriptorRights::WRITE),
    };
    let Ok(mut context) = context.try_borrow_mut() else {
        return WasiErrno::IO;
    };
    let opened = match context.path_open(GuestFd::new(*fd as u32), path, options, rights) {
        Ok(opened) => opened,
        Err(error) => return error.into(),
    };
    let _ = write_u32(memory, *result as u32, opened.raw());
    WasiErrno::SUCCESS
}

fn path_unlink_file<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    args: &[Val],
    memory: &[u8],
) -> WasiErrno {
    let [Val::I32(fd), Val::I32(path_pointer), Val::I32(path_length)] = args else {
        return WasiErrno::INVAL;
    };
    let Some(path_bytes) = memory_range(memory, *path_pointer as u32, *path_length as u32) else {
        return WasiErrno::FAULT;
    };
    let Ok(path) = core::str::from_utf8(path_bytes) else {
        return WasiErrno::INVAL;
    };
    let Ok(mut context) = context.try_borrow_mut() else {
        return WasiErrno::IO;
    };
    match context.path_unlink(GuestFd::new(*fd as u32), path) {
        Ok(()) => WasiErrno::SUCCESS,
        Err(error) => error.into(),
    }
}

fn proc_exit<B: HostBackend>(
    context: &Rc<RefCell<HostContext<B>>>,
    exit_code: &Cell<Option<u32>>,
    args: &[Val],
) -> Result<Vec<Val>, WasmError> {
    let [Val::I32(code)] = args else {
        return Err(WasmError::Trap("wasi preview1 proc_exit arguments"));
    };
    exit_code.set(None);
    let Ok(mut context) = context.try_borrow_mut() else {
        return Err(WasmError::Trap("wasi preview1 host is already borrowed"));
    };
    context
        .exit(*code as u32)
        .map_err(|_| WasmError::Trap("wasi preview1 proc_exit backend"))?;
    exit_code.set(Some(*code as u32));
    Err(WasmError::Trap(WASI_PROC_EXIT_TRAP))
}

fn descriptor_rights(raw: u64) -> Option<DescriptorRights> {
    if raw & !WASI_SUPPORTED_FILE_RIGHTS != 0 {
        return None;
    }
    let mut rights = DescriptorRights::NONE;
    if raw & WASI_RIGHT_FD_READ != 0 {
        rights = rights.union(DescriptorRights::READ);
    }
    if raw & WASI_RIGHT_FD_WRITE != 0 {
        rights = rights.union(DescriptorRights::WRITE);
    }
    if raw & WASI_RIGHT_FD_SEEK != 0 {
        rights = rights.union(DescriptorRights::SEEK);
    }
    if raw & WASI_RIGHT_FD_FILESTAT_GET != 0 {
        rights = rights.union(DescriptorRights::STAT);
    }
    Some(rights)
}

fn snapshot_iovecs(memory: &[u8], table: u32, count: u32) -> Result<Vec<(u32, u32)>, WasiErrno> {
    if count > MAX_IOVECS {
        return Err(WasiErrno::INVAL);
    }
    let Some(table_bytes) = count.checked_mul(8) else {
        return Err(WasiErrno::FAULT);
    };
    if memory_range(memory, table, table_bytes).is_none() {
        return Err(WasiErrno::FAULT);
    }
    let mut iovecs = Vec::new();
    iovecs
        .try_reserve_exact(count as usize)
        .map_err(|_| WasiErrno::NOMEM)?;
    for index in 0..count {
        let (pointer, length) = iovec(memory, table, index).ok_or(WasiErrno::FAULT)?;
        memory_range(memory, pointer, length).ok_or(WasiErrno::FAULT)?;
        iovecs.push((pointer, length));
    }
    Ok(iovecs)
}

fn iovec(memory: &[u8], table: u32, index: u32) -> Option<(u32, u32)> {
    let offset = index.checked_mul(8)?;
    let record = memory_range(memory, table.checked_add(offset)?, 8)?;
    Some((
        u32::from_le_bytes(record[0..4].try_into().ok()?),
        u32::from_le_bytes(record[4..8].try_into().ok()?),
    ))
}

fn two_i32(args: &[Val]) -> Option<(u32, u32)> {
    let [Val::I32(first), Val::I32(second)] = args else {
        return None;
    };
    Some((*first as u32, *second as u32))
}

fn string_bytes(values: &[alloc::string::String]) -> Result<u32, ()> {
    let mut total = 0usize;
    for value in values {
        total = total.checked_add(value.len() + 1).ok_or(())?;
    }
    u32::try_from(total).map_err(|_| ())
}

fn memory_range(memory: &[u8], pointer: u32, length: u32) -> Option<&[u8]> {
    let start = pointer as usize;
    let end = start.checked_add(length as usize)?;
    memory.get(start..end)
}

fn memory_range_mut(memory: &mut [u8], pointer: u32, length: u32) -> Option<&mut [u8]> {
    let start = pointer as usize;
    let end = start.checked_add(length as usize)?;
    memory.get_mut(start..end)
}

fn write_u32(memory: &mut [u8], pointer: u32, value: u32) -> Option<()> {
    memory_range_mut(memory, pointer, 4)?.copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn write_u64(memory: &mut [u8], pointer: u32, value: u64) -> Option<()> {
    memory_range_mut(memory, pointer, 8)?.copy_from_slice(&value.to_le_bytes());
    Some(())
}
