@echo off
setlocal
pushd "%~dp0"
set "AGENTERM_RELEASE_MODE=publish"
if /I "%~1"=="--rehearse" (
    set "AGENTERM_RELEASE_MODE=rehearse"
    shift
)
if not "%~1"=="" (
    echo Usage: release.cmd [--rehearse] 1>&2
    popd
    exit /b 2
)
cargo build --quiet --locked --bin agenterm-script
if errorlevel 1 goto :failed
set "AGENTERM_RELEASE_TARGET=target"
if defined CARGO_TARGET_DIR set "AGENTERM_RELEASE_TARGET=%CARGO_TARGET_DIR%"
set "AGENTERM_RELEASE_WORKER=%AGENTERM_RELEASE_TARGET%\debug\agenterm-script.exe"
set "AGENTERM_RELEASE_BOOTSTRAP=%AGENTERM_RELEASE_TARGET%\release-bootstrap-%RANDOM%"
mkdir "%AGENTERM_RELEASE_BOOTSTRAP%" >nul 2>nul
copy /y "%AGENTERM_RELEASE_WORKER%" "%AGENTERM_RELEASE_BOOTSTRAP%\agenterm-script.exe" >nul
if errorlevel 1 goto :failed
"%AGENTERM_RELEASE_BOOTSTRAP%\agenterm-script.exe" task run release --manifest "agenterm.tasks.json" --timeout-ms 3600000 --max-operations 10000000 --max-collection-items 100000 --max-string-bytes 8388608 --max-output-bytes 1048576 -- "%CD%" "%AGENTERM_RELEASE_MODE%"
if errorlevel 1 goto :failed
del /q "%AGENTERM_RELEASE_BOOTSTRAP%\agenterm-script.exe" >nul 2>nul
rmdir "%AGENTERM_RELEASE_BOOTSTRAP%" >nul 2>nul
popd
exit /b 0

:failed
set "AGENTERM_RELEASE_EXIT=%errorlevel%"
if defined AGENTERM_RELEASE_BOOTSTRAP del /q "%AGENTERM_RELEASE_BOOTSTRAP%\agenterm-script.exe" >nul 2>nul
if defined AGENTERM_RELEASE_BOOTSTRAP rmdir "%AGENTERM_RELEASE_BOOTSTRAP%" >nul 2>nul
popd
exit /b %AGENTERM_RELEASE_EXIT%
