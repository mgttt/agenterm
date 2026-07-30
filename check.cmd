@echo off
setlocal
pushd "%~dp0"
cargo build --quiet --locked --bin agenterm-script
if errorlevel 1 goto :failed
set "AGENTERM_CHECK_TARGET=target"
if defined CARGO_TARGET_DIR set "AGENTERM_CHECK_TARGET=%CARGO_TARGET_DIR%"
set "AGENTERM_CHECK_WORKER=%AGENTERM_CHECK_TARGET%\debug\agenterm-script.exe"
set "AGENTERM_CHECK_BOOTSTRAP=%AGENTERM_CHECK_TARGET%\check-bootstrap-%RANDOM%"
mkdir "%AGENTERM_CHECK_BOOTSTRAP%" >nul 2>nul
copy /y "%AGENTERM_CHECK_WORKER%" "%AGENTERM_CHECK_BOOTSTRAP%\agenterm-script.exe" >nul
if errorlevel 1 goto :failed
"%AGENTERM_CHECK_BOOTSTRAP%\agenterm-script.exe" task run check --manifest "agenterm.tasks.json" --timeout-ms 3600000 --max-operations 100000000 --max-string-bytes 8388608 --max-output-bytes 1048576 -- "%AGENTERM_CHECK_WORKER%" %*
if errorlevel 1 goto :failed
del /q "%AGENTERM_CHECK_BOOTSTRAP%\agenterm-script.exe" >nul 2>nul
rmdir "%AGENTERM_CHECK_BOOTSTRAP%" >nul 2>nul
popd
exit /b 0

:failed
set "AGENTERM_CHECK_EXIT=%errorlevel%"
if defined AGENTERM_CHECK_BOOTSTRAP del /q "%AGENTERM_CHECK_BOOTSTRAP%\agenterm-script.exe" >nul 2>nul
if defined AGENTERM_CHECK_BOOTSTRAP rmdir "%AGENTERM_CHECK_BOOTSTRAP%" >nul 2>nul
popd
exit /b %AGENTERM_CHECK_EXIT%
