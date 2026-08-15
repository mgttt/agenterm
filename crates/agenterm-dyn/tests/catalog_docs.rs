//! Cross-host documentation consistency for Darwin system-probe catalog rows.
//!
//! This suite reads only compile-time host data and repository Markdown. It is
//! intentionally portable and does not claim Darwin native-call evidence.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use agenterm_dyn::{LINUX_X86_64, MACOS_AARCH64, MACOS_X86_64, SystemProbeStatus};

const DARWIN_ONLY_LIVE_EXAMPLES: &[(&str, &str)] = &[
    ("sysctlbyname", "sysctlbyname.md"),
    ("mach_absolute_time", "mach-absolute-time.md"),
    ("getprogname", "getprogname.md"),
    ("issetugid", "issetugid.md"),
    ("nsget_executable_path", "nsget-executable-path.md"),
    ("proc_pidpath", "proc-pidpath.md"),
    ("arc4random", "arc4random.md"),
    ("clock_gettime_nsec_np", "clock-gettime-nsec-np.md"),
    ("sysctl", "sysctl.md"),
    ("mach_timebase_info", "mach-timebase-info.md"),
    ("pthread_main_np", "pthread-main-np.md"),
    ("getlogin_r", "getlogin-r.md"),
    ("pthread_threadid_np", "pthread-threadid-np.md"),
    ("pthread_getname_np", "pthread-getname-np.md"),
    ("proc_pidinfo", "proc-pidinfo.md"),
    ("nsget_argc", "nsget-argc.md"),
    ("nsget_argv", "nsget-argv.md"),
    ("nsget_environ", "nsget-environ.md"),
    ("proc_pid_rusage", "proc-pid-rusage.md"),
    ("dyld_image_count", "dyld-image-count.md"),
    ("getentropy", "getentropy.md"),
    ("proc_name", "proc-name.md"),
    ("pthread_get_stackaddr_np", "pthread-get-stackaddr-np.md"),
    ("pthread_get_stacksize_np", "pthread-get-stacksize-np.md"),
    ("pthread_self", "pthread-self.md"),
    ("pthread_cpu_number_np", "pthread-cpu-number-np.md"),
    ("malloc_good_size", "malloc-good-size.md"),
    ("nsget_progname", "nsget-progname.md"),
    ("proc_libversion", "proc-libversion.md"),
    (
        "pthread_jit_write_protect_supported_np",
        "pthread-jit-write-protect-supported-np.md",
    ),
    ("sysctlnametomib", "sysctlnametomib.md"),
    ("pthread_equal", "pthread-equal.md"),
    ("gethostname", "gethostname.md"),
    ("confstr", "confstr.md"),
    ("clock_getres", "clock-getres.md"),
    ("pthread_is_threaded_np", "pthread-is-threaded-np.md"),
    ("nsget_mach_execute_header", "nsget-mach-execute-header.md"),
    ("dyld_get_image_name", "dyld-get-image-name.md"),
    (
        "dyld_get_image_vmaddr_slide",
        "dyld-get-image-vmaddr-slide.md",
    ),
    ("dladdr", "dladdr.md"),
    ("gethostuuid", "gethostuuid.md"),
    ("dyld_get_image_header", "dyld-get-image-header.md"),
];

#[test]
fn darwin_arch_cells_have_identical_ordered_probe_contracts() {
    assert_eq!(
        MACOS_X86_64.system_probes, MACOS_AARCH64.system_probes,
        "Darwin x86_64 and aarch64 must expose the same ordered names and statuses"
    );
}

#[test]
fn every_darwin_only_live_probe_has_a_linked_example() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("README.md")).expect("crate README is readable");
    let manifest = DARWIN_ONLY_LIVE_EXAMPLES
        .iter()
        .copied()
        .collect::<HashMap<_, _>>();
    assert_eq!(
        manifest.len(),
        DARWIN_ONLY_LIVE_EXAMPLES.len(),
        "Darwin example manifest names must be unique"
    );

    let darwin_only_live = MACOS_X86_64
        .system_probes
        .iter()
        .zip(LINUX_X86_64.system_probes.iter())
        .filter_map(|(darwin, linux)| {
            let darwin_is_live = matches!(darwin.status, SystemProbeStatus::LiveDlcall { .. });
            let linux_is_placeholder = matches!(linux.status, SystemProbeStatus::Placeholder);
            (darwin_is_live && linux_is_placeholder).then_some(darwin.name)
        })
        .collect::<HashSet<_>>();
    let documented = manifest.keys().copied().collect::<HashSet<_>>();
    assert_eq!(
        documented, darwin_only_live,
        "the explicit example manifest must cover every Darwin-only live row exactly"
    );

    for (probe, file) in DARWIN_ONLY_LIVE_EXAMPLES {
        assert!(
            root.join("examples").join(file).is_file(),
            "Darwin live probe `{probe}` is missing examples/{file}"
        );
        assert!(
            readme.contains(&format!("](examples/{file})")),
            "Darwin live probe `{probe}` is missing its README link to examples/{file}"
        );
    }
}

#[test]
fn mach_host_self_remains_placeholder_with_honesty_only_documentation() {
    for cell in [MACOS_X86_64, MACOS_AARCH64] {
        let probe = cell
            .system_probes
            .iter()
            .find(|probe| probe.name == "mach_host_self")
            .expect("Darwin catalog contains mach_host_self");
        assert!(matches!(probe.status, SystemProbeStatus::Placeholder));
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = fs::read_to_string(root.join("examples/mach-host-self.md"))
        .expect("mach_host_self honesty document is readable");
    assert!(example.contains("send right"));
    assert!(
        !example
            .lines()
            .any(|line| line.trim_start().starts_with("```")),
        "mach_host_self honesty documentation must not contain callable code"
    );

    let readme = fs::read_to_string(root.join("README.md")).expect("crate README is readable");
    assert!(readme.contains("](examples/mach-host-self.md)"));
}
