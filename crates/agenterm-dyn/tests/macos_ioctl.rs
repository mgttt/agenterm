//! Darwin `ioctl` must use its variadic ABI, not dyn's fixed-arity trampoline.

#[cfg(target_os = "macos")]
mod macos {
    use agenterm_dyn::{Dyn, Value};

    struct Fd(libc::c_int);

    impl Drop for Fd {
        fn drop(&mut self) {
            // SAFETY: this test owns the descriptors returned by `openpty`.
            unsafe {
                libc::close(self.0);
            }
        }
    }

    #[test]
    fn dlcall_ioctl_reads_openpty_slave_winsize_through_darwin_variadic_abi() {
        let mut master = -1;
        let mut slave = -1;
        let mut requested = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: all out-pointers are valid for the duration of the call.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut requested,
                )
            },
            0,
            "openpty must provide a slave for the Darwin ioctl smoke"
        );
        let _master = Fd(master);
        let _slave = Fd(slave);

        let mut observed: libc::winsize = unsafe { std::mem::zeroed() };
        let mut env = Dyn::new();
        env.bind("winsize", (&mut observed as *mut libc::winsize).cast())
            .expect("bind caller-owned winsize");
        let result = env
            .eval(&format!(
                r#"(dlcall "libSystem.B.dylib" "ioctl" "i32" "i32" {slave} "u64" {} "ptr" winsize)"#,
                libc::TIOCGWINSZ
            ))
            .expect("Darwin variadic ioctl dlcall");

        assert_eq!(result, Value::Int(0));
        assert_eq!(observed.ws_row, 24);
        assert_eq!(observed.ws_col, 80);
    }
}
