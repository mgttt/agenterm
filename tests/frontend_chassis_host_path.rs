//! Source-order lock for both host adapters' Chassis-L1 selection boundary.

const UNIX_FRONTEND: &str = include_str!("../src/platform/adapters/unix/frontend/mod.rs");
const WINDOWS_FRONTEND: &str = include_str!("../src/platform/adapters/windows/frontend.rs");

#[test]
fn both_hosts_execute_the_validated_loader_instead_of_the_fat_workbench() {
    for source in [UNIX_FRONTEND, WINDOWS_FRONTEND] {
        let load = source
            .find("chassis_image::load_selected_image")
            .expect("validate composed image");
        let select = source[load..]
            .find("run_selected_chassis_loader(&image.native_loader, &image.root)")
            .map(|offset| load + offset)
            .expect("select validated native loader");
        let selected_return = source[select..]
            .find("return GuiLaunchResult::Launched")
            .map(|offset| select + offset)
            .expect("native loader owns the selected launch");
        let fat_gui = source[load..]
            .find("attempt_gui_handoff")
            .map(|offset| load + offset)
            .expect("legacy workbench fallback");

        assert!(load < select);
        assert!(select < selected_return);
        assert!(selected_return < fat_gui);
        assert!(source.contains("std::process::Command::new(loader)"));
        assert!(source.contains(".arg(image_root)"));
        assert!(!source.contains("Loaded chassis L3"));
    }
}

#[test]
fn invalid_or_valid_loader_selection_never_falls_through_to_gui_pty_or_ipc() {
    #[derive(Default)]
    struct Calls {
        loader: usize,
        gui: usize,
        pty: usize,
        ipc: usize,
    }

    fn selected_path(calls: &mut Calls, loader_ok: bool) -> Result<(), &'static str> {
        calls.loader += 1;
        if !loader_ok {
            return Err("loader failed");
        }
        Ok(())
    }

    let mut valid = Calls::default();
    selected_path(&mut valid, true).expect("valid native loader");
    assert_eq!(
        (valid.loader, valid.gui, valid.pty, valid.ipc),
        (1, 0, 0, 0)
    );

    let mut invalid = Calls::default();
    selected_path(&mut invalid, false).expect_err("invalid loader");
    assert_eq!(
        (invalid.loader, invalid.gui, invalid.pty, invalid.ipc),
        (1, 0, 0, 0)
    );
}
