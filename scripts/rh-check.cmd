@echo off
setlocal
cd /d "%~dp0.."
echo == build agenterm-rh binary ==
cargo build -p agenterm-rh --locked --bin agenterm-rh
if errorlevel 1 exit /b 1

echo == agenterm-rh crate ==
cargo test -p agenterm-rh --locked
if errorlevel 1 exit /b 1

echo == rh integration tests ==
cargo test --locked --test rh_aot_smoke --test rh_aot_ci_policy --test rh_regression --test rh_backend --test rh_corpus --test rh_framed_worker --test rh_cli_forward
if errorlevel 1 exit /b 1

echo == rh host + cache lib tests ==
cargo test -p agenterm --locked --lib script_rh_host
if errorlevel 1 exit /b 1
cargo test -p agenterm --locked --lib script_rh_cache
if errorlevel 1 exit /b 1

echo PASS rh-check
exit /b 0
