@echo off
setlocal
pushd "%~dp0"
cargo build --quiet --locked --bin agenterm-script
if errorlevel 1 goto :failed
set "AGENTERM_LINT_TARGET=target"
if defined CARGO_TARGET_DIR set "AGENTERM_LINT_TARGET=%CARGO_TARGET_DIR%"
set "AGENTERM_LINT_WORKER=%AGENTERM_LINT_TARGET%\debug\agenterm-script.exe"
set "AGENTERM_LINT_BOOTSTRAP=%AGENTERM_LINT_TARGET%\lint-bootstrap-%RANDOM%"
mkdir "%AGENTERM_LINT_BOOTSTRAP%" >nul 2>nul
copy /y "%AGENTERM_LINT_WORKER%" "%AGENTERM_LINT_BOOTSTRAP%\agenterm-script.exe" >nul
if errorlevel 1 goto :failed
"%AGENTERM_LINT_BOOTSTRAP%\agenterm-script.exe" task run lint --manifest "agenterm.tasks.json" --timeout-ms 120000 --max-operations 10000000 -- "%AGENTERM_LINT_WORKER%" %*
if errorlevel 1 goto :failed
del /q "%AGENTERM_LINT_BOOTSTRAP%\agenterm-script.exe" >nul 2>nul
rmdir "%AGENTERM_LINT_BOOTSTRAP%" >nul 2>nul
popd
exit /b 0

:failed
set "AGENTERM_LINT_EXIT=%errorlevel%"
if defined AGENTERM_LINT_BOOTSTRAP del /q "%AGENTERM_LINT_BOOTSTRAP%\agenterm-script.exe" >nul 2>nul
if defined AGENTERM_LINT_BOOTSTRAP rmdir "%AGENTERM_LINT_BOOTSTRAP%" >nul 2>nul
popd
exit /b %AGENTERM_LINT_EXIT%
