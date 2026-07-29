@echo off
setlocal
pushd "%~dp0"
cargo build --quiet --locked --bin agenterm-script
if errorlevel 1 goto :failed
set "AGENTERM_LINT_WORKER=target\debug\agenterm-script.exe"
"%AGENTERM_LINT_WORKER%" task run lint --manifest "agenterm.tasks.json" --timeout-ms 120000 --max-operations 10000000 -- "." "%AGENTERM_LINT_WORKER%" %*
if errorlevel 1 goto :failed
popd
exit /b 0

:failed
set "AGENTERM_LINT_EXIT=%errorlevel%"
popd
exit /b %AGENTERM_LINT_EXIT%
