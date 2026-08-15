//! macOS `ioctl` must use Unix's variadic ABI, not dyn's fixed-arity trampoline.

#[cfg(target_os = "macos")]
mod macos {
    use agenterm_dyn::{Dyn, DynError, Value};

    fn eval_native(env: &mut Dyn, source: &str) -> Result<Value, DynError> {
        // SAFETY: the test owns the `winsize` buffer and validates Unix ioctl's ABI on macOS.
        unsafe { env.eval_native(source) }
    }

    struct Fd(libc::c_int);

    impl Drop for Fd {
        fn drop(&mut self) {
            if self.0 >= 0 {
                // SAFETY: this test owns descriptors initialized by `openpty`.
                // A failed call may initialize either one, so both are guarded.
                unsafe {
                    libc::close(self.0);
                }
            }
        }
    }

    #[test]
    fn dlcall_ioctl_reads_openpty_slave_winsize_through_unix_variadic_abi() {
        let mut master = -1;
        let mut slave = -1;
        let mut requested = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: all out-pointers are valid for the duration of the call.
        let status = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut requested,
            )
        };
        // Take ownership before asserting: a failed openpty may still have
        // initialized either output descriptor.
        let _master = Fd(master);
        let slave = Fd(slave);
        assert_eq!(
            status, 0,
            "openpty must provide a slave for the macOS ioctl smoke"
        );

        let mut observed: libc::winsize = unsafe { std::mem::zeroed() };
        let mut env = Dyn::new();
        env.bind("winsize", (&mut observed as *mut libc::winsize).cast())
            .expect("bind caller-owned winsize");
        let result = eval_native(
            &mut env,
            &format!(
                r#"(dlcall "libSystem.B.dylib" "ioctl" "i32" "i32" {} "u64" {} "ptr" winsize)"#,
                slave.0,
                libc::TIOCGWINSZ
            ),
        )
        .expect("Unix variadic ioctl dlcall");

        assert_eq!(result, Value::Int(0));
        assert_eq!(observed.ws_row, 24);
        assert_eq!(observed.ws_col, 80);
    }
}
