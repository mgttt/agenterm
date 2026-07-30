@echo off
setlocal
if "%AGENTERM_BOOTSTRAP_TASK%"=="" exit /b 2

for /f "usebackq delims=" %%I in (`git -C "%~dp0." rev-parse --show-toplevel 2^>nul`) do set "AGENTERM_BOOTSTRAP_REPO=%%I"
if "%AGENTERM_BOOTSTRAP_REPO%"=="" exit /b 2
pushd "%AGENTERM_BOOTSTRAP_REPO%"
cargo build --quiet --locked --bin agenterm-script
if errorlevel 1 goto :failed

set "AGENTERM_BOOTSTRAP_TARGET=target"
if defined CARGO_TARGET_DIR set "AGENTERM_BOOTSTRAP_TARGET=%CARGO_TARGET_DIR%"
set "AGENTERM_BOOTSTRAP_SOURCE=%AGENTERM_BOOTSTRAP_TARGET%\debug\agenterm-script.exe"
set "AGENTERM_BOOTSTRAP_DIR=%AGENTERM_BOOTSTRAP_TARGET%\task-bootstrap-%RANDOM%-%RANDOM%"
mkdir "%AGENTERM_BOOTSTRAP_DIR%" >nul 2>nul
copy /y "%AGENTERM_BOOTSTRAP_SOURCE%" "%AGENTERM_BOOTSTRAP_DIR%\agenterm-script.exe" >nul
if errorlevel 1 goto :failed
set "AGENTERM_BOOTSTRAP_WORKER=%AGENTERM_BOOTSTRAP_DIR%\agenterm-script.exe"

"%AGENTERM_BOOTSTRAP_WORKER%" task run "%AGENTERM_BOOTSTRAP_TASK%" --manifest "%AGENTERM_BOOTSTRAP_REPO%\agenterm.tasks.json" -- %*
if errorlevel 1 goto :failed
del /q "%AGENTERM_BOOTSTRAP_WORKER%" >nul 2>nul
rmdir "%AGENTERM_BOOTSTRAP_DIR%" >nul 2>nul
popd
exit /b 0

:failed
set "AGENTERM_BOOTSTRAP_EXIT=%errorlevel%"
if defined AGENTERM_BOOTSTRAP_WORKER del /q "%AGENTERM_BOOTSTRAP_WORKER%" >nul 2>nul
if defined AGENTERM_BOOTSTRAP_DIR rmdir "%AGENTERM_BOOTSTRAP_DIR%" >nul 2>nul
popd
exit /b %AGENTERM_BOOTSTRAP_EXIT%
