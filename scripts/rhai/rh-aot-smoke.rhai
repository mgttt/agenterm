// Prove rh-0 check, AOT pack build, native_hash receipt, and dlopen entry smoke.
//
// Args: REPO [AGENTERM_RH_BINARY]

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len == 1 || args.len == 2, "expected: REPO [AGENTERM_RH_BINARY]");
let repo = std::path::absolute(args[0]).display;
let fixture = std::path::join(repo, "fixtures/rh/entry.rh").display;
require(std::fs::exists(fixture), "rh_aot_missing_fixture");

// Prefer an explicit binary; otherwise pick host layout by path presence
// (std::process::platform is not a shipped Script API).
let rh = if args.len == 2 {
    std::path::absolute(args[1]).display
} else {
    let win = std::path::join(repo, "target/debug/agenterm-rh.exe").display;
    let unix = std::path::join(repo, "target/debug/agenterm-rh").display;
    if std::fs::exists(win) {
        win
    } else {
        unix
    }
};
require(std::fs::exists(rh), "rh_aot_missing_binary:" + rh);

let pack_dir = std::path::join(repo, "target/rh-aot-smoke").display;
if std::fs::exists(pack_dir) {
    std::fs::remove_dir_all(pack_dir);
}
std::fs::create_dir_all(pack_dir);

let check = std::process::command(rh);
check.args(["check", fixture]);
check.current_dir(repo);
check.timeout(std::time::Duration::from_secs(30));
let check_out = check.output();
require(check_out.success, "rh_aot_check:" + check_out.stderr_text());

let build = std::process::command(rh);
build.args(["pack", "build", fixture, "--dir", pack_dir]);
build.current_dir(repo);
build.timeout(std::time::Duration::from_secs(180));
let build_out = build.output();
require(build_out.success, "rh_aot_pack_build:" + build_out.stderr_text());

let native_dll = std::path::join(pack_dir, "pack.dll").display;
let native_dylib = std::path::join(pack_dir, "pack.dylib").display;
let native_so = std::path::join(pack_dir, "pack.so").display;
let native = if std::fs::exists(native_dll) {
    native_dll
} else if std::fs::exists(native_dylib) {
    native_dylib
} else {
    native_so
};

let smoke = std::process::command(rh);
smoke.args(["run-smoke", native]);
smoke.current_dir(repo);
smoke.timeout(std::time::Duration::from_secs(30));
let smoke_out = smoke.output();
require(smoke_out.success, "rh_aot_run_smoke:" + smoke_out.stderr_text());
require(
    smoke_out.stdout_text().contains("rh_entry() = 42"),
    "rh_aot_entry_value"
);

let receipt_path = std::path::join(pack_dir, "qualification.json").display;
let qualify = std::process::command(rh);
qualify.args(["qualify", fixture, "--dir", pack_dir, "-o", receipt_path]);
qualify.current_dir(repo);
qualify.timeout(std::time::Duration::from_secs(180));
let qualify_out = qualify.output();
require(qualify_out.success, "rh_aot_qualify:" + qualify_out.stderr_text());
require(std::fs::exists(receipt_path), "rh_aot_receipt_missing");

print("PASS rh-aot-smoke");
