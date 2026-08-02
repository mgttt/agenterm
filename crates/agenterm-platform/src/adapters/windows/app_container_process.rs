use std::os::windows::{
    ffi::OsStrExt as _,
    io::{AsHandle as _, AsRawHandle as _, BorrowedHandle, FromRawHandle as _, OwnedHandle},
};

use windows_sys::Win32::{
    Foundation::{GetLastError, HANDLE},
    Security::{SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES},
    System::Threading::{
        CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
        DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
        InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
        PROCESS_INFORMATION, ResumeThread, STARTUPINFOEXW, UpdateProcThreadAttribute,
    },
};

use crate::{
    adapters::windows::app_container::{aligned_sid_copy, validate_sid_bytes},
    app_container_process::{AppContainerProcessError, PreparedAppContainerProcess},
    process_reference::ProcessReference,
};

pub(crate) struct ResumeToken(OwnedHandle);

pub(crate) fn spawn(
    options: &PreparedAppContainerProcess<'_>,
) -> Result<(ProcessReference, ResumeToken), AppContainerProcessError> {
    spawn_with_handles(options, &[])
}

pub(crate) fn spawn_with_handles(
    options: &PreparedAppContainerProcess<'_>,
    handles: &[BorrowedHandle<'_>],
) -> Result<(ProcessReference, ResumeToken), AppContainerProcessError> {
    const OPERATION: &str = "spawn-app-container-process";
    validate_sid_bytes(OPERATION, options.app_container_sid)
        .map_err(|error| AppContainerProcessError::invalid(OPERATION, error.to_string()))?;
    let app_container_sid = aligned_sid_copy(OPERATION, options.app_container_sid)
        .map_err(|error| AppContainerProcessError::invalid(OPERATION, error.to_string()))?;
    let capability_sids = options
        .capabilities
        .iter()
        .map(|capability| aligned_sid_copy(OPERATION, capability.sid()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppContainerProcessError::invalid(OPERATION, error.to_string()))?;
    let mut capabilities = options
        .capabilities
        .iter()
        .zip(&capability_sids)
        .map(|(capability, sid)| SID_AND_ATTRIBUTES {
            Sid: sid.as_ptr().cast_mut().cast(),
            Attributes: capability.attributes(),
        })
        .collect::<Vec<_>>();
    let capability_count = u32::try_from(capabilities.len()).map_err(|_| {
        AppContainerProcessError::invalid(OPERATION, "capability count exceeds u32")
    })?;
    let mut security_capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: app_container_sid.as_ptr().cast_mut().cast(),
        Capabilities: if capabilities.is_empty() {
            std::ptr::null_mut()
        } else {
            capabilities.as_mut_ptr()
        },
        CapabilityCount: capability_count,
        Reserved: 0,
    };

    let mut inherited = handles
        .iter()
        .map(|handle| handle.as_raw_handle() as HANDLE)
        .collect::<Vec<_>>();
    inherited.sort_unstable_by_key(|handle| *handle as usize);
    inherited.dedup();

    let attribute_count = if inherited.is_empty() { 1 } else { 2 };
    let mut attributes = AttributeList::new(attribute_count)?;
    attributes.update(
        PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
        (&raw mut security_capabilities).cast(),
        std::mem::size_of::<SECURITY_CAPABILITIES>(),
        "set-app-container-security-capabilities",
    )?;
    if !inherited.is_empty() {
        attributes.update(
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherited.as_mut_ptr().cast(),
            inherited
                .len()
                .checked_mul(std::mem::size_of::<HANDLE>())
                .ok_or_else(|| {
                    AppContainerProcessError::invalid(
                        OPERATION,
                        "inherited HANDLE list size overflows",
                    )
                })?,
            "set-app-container-handle-list",
        )?;
    }

    let mut command_line = options
        .command_line
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let working_directory = encode_path(options.working_directory)?;
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attributes.raw;
    let mut information: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let flags = CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT;
    let borrowed = inherited
        .iter()
        .map(|handle| unsafe { BorrowedHandle::borrow_raw(*handle as _) })
        .collect::<Vec<_>>();

    let created = crate::process_spawn::with_inheritable_handles(borrowed.as_slice(), || unsafe {
        let ok = CreateProcessW(
            std::ptr::null(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            i32::from(!inherited.is_empty()),
            flags,
            options.environment.as_ptr().cast(),
            working_directory.as_ptr(),
            &startup.StartupInfo,
            &raw mut information,
        );
        let error = if ok == 0 { GetLastError() } else { 0 };
        (ok, error)
    });

    let (ok, error) = match created {
        Ok(result) => result,
        Err(error) => {
            cleanup_information(information);
            return Err(AppContainerProcessError::io(
                "restore-app-container-inherited-handles",
                error,
            ));
        }
    };
    if ok == 0 {
        return Err(AppContainerProcessError::native(OPERATION, error));
    }

    let mut created = CreatedProcess::new(information);
    let process = ProcessReference::duplicate_from(created.process.as_handle())
        .map_err(|error| AppContainerProcessError::io("retain-app-container-process", error))?;
    let thread = created
        .thread
        .take()
        .expect("successful process creation owns a primary thread");
    created.disarm();
    Ok((process, ResumeToken(thread)))
}

pub(crate) fn resume(token: &mut ResumeToken) -> Result<(), AppContainerProcessError> {
    if unsafe { ResumeThread(token.0.as_raw_handle()) } == u32::MAX {
        Err(AppContainerProcessError::native(
            "resume-app-container-process",
            unsafe { GetLastError() },
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn abort_suspended(process: &ProcessReference, _token: &mut ResumeToken) {
    let _ = crate::process_control::terminate_handle(process.as_handle(), 1);
    let _ = crate::process_reference::wait_handle(process.as_handle(), None);
}

fn encode_path(path: &std::path::Path) -> Result<Vec<u16>, AppContainerProcessError> {
    let mut encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if encoded.contains(&0) {
        return Err(AppContainerProcessError::invalid(
            "prepare-app-container-working-directory",
            "working directory contains NUL",
        ));
    }
    encoded.push(0);
    Ok(encoded)
}

struct AttributeList {
    _storage: Vec<usize>,
    raw: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl AttributeList {
    fn new(count: u32) -> Result<Self, AppContainerProcessError> {
        let mut bytes = 0_usize;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), count, 0, &raw mut bytes);
        }
        if bytes == 0 {
            return Err(AppContainerProcessError::native(
                "size-app-container-attribute-list",
                unsafe { GetLastError() },
            ));
        }
        let mut storage = vec![0_usize; bytes.div_ceil(std::mem::size_of::<usize>())];
        let raw = storage.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(raw, count, 0, &raw mut bytes) } == 0 {
            return Err(AppContainerProcessError::native(
                "initialize-app-container-attribute-list",
                unsafe { GetLastError() },
            ));
        }
        Ok(Self {
            _storage: storage,
            raw,
        })
    }

    fn update(
        &mut self,
        kind: usize,
        value: *const core::ffi::c_void,
        bytes: usize,
        operation: &'static str,
    ) -> Result<(), AppContainerProcessError> {
        if unsafe {
            UpdateProcThreadAttribute(
                self.raw,
                0,
                kind,
                value,
                bytes,
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        } == 0
        {
            Err(AppContainerProcessError::native(operation, unsafe {
                GetLastError()
            }))
        } else {
            Ok(())
        }
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.raw) };
    }
}

struct CreatedProcess {
    process: OwnedHandle,
    thread: Option<OwnedHandle>,
    armed: bool,
}

impl CreatedProcess {
    fn new(information: PROCESS_INFORMATION) -> Self {
        Self {
            process: unsafe { OwnedHandle::from_raw_handle(information.hProcess) },
            thread: Some(unsafe { OwnedHandle::from_raw_handle(information.hThread) }),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CreatedProcess {
    fn drop(&mut self) {
        if self.armed {
            let _ = crate::process_control::terminate_handle(self.process.as_handle(), 1);
            let _ = crate::process_reference::wait_handle(self.process.as_handle(), None);
        }
    }
}

fn cleanup_information(information: PROCESS_INFORMATION) {
    if information.hProcess.is_null() {
        return;
    }
    drop(CreatedProcess::new(information));
}
