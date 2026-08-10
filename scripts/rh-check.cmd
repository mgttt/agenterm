@echo off
setlocal
cd /d "%~dp0.."
echo == build agenterm (hosted rh engine) ==
cargo build --locked --bin agenterm
if errorlevel 1 exit /b 1

echo == agenterm-rh crate ==
cargo test -p agenterm-rh --locked
if errorlevel 1 exit /b 1

echo == rh integration tests ==
cargo test --locked --test rh_aot_smoke --test rh_aot_ci_policy --test rh_regression --test rh_backend --test rh_corpus --test rh_framed_worker --test rh_cli_forward --test rh_standalone_cli --test rh_native_task --test script_check_many --test performance_experiment_policy
if errorlevel 1 exit /b 1

echo == all-engine execution parity ==
cargo test --locked --all-features --test script_engine_exec_parity
if errorlevel 1 exit /b 1

echo == rh task-entry native packs ==
cargo test --locked --test rh_task_entry_regression
if errorlevel 1 exit /b 1

echo == rh host + cache lib tests ==
cargo test -p agenterm --locked --lib script_rh_host
if errorlevel 1 exit /b 1
cargo test -p agenterm --locked --lib script_rh_cache
if errorlevel 1 exit /b 1

echo PASS rh-check
exit /b 0
