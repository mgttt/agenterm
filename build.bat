@echo off
setlocal
pushd "%~dp0"

set "PROFILE=dev"
set "CARGO_ARGS="
set "CARGO_OUTPUT_DIR=target"
if defined CARGO_TARGET_DIR set "CARGO_OUTPUT_DIR=%CARGO_TARGET_DIR%"
set "GUI_SOURCE_EXE=%CARGO_OUTPUT_DIR%\debug\agenterm.exe"
set "CLI_SOURCE_EXE=%CARGO_OUTPUT_DIR%\debug\agentermctl.exe"
set "MUX_SOURCE_EXE=%CARGO_OUTPUT_DIR%\debug\agenterm-mux.exe"
set "SCRIPT_SOURCE_EXE=%CARGO_OUTPUT_DIR%\debug\agenterm-script.exe"
set "DIST_DIR=%~dp0dist"

if /i "%~1"=="release" (
    set "PROFILE=release"
    set "CARGO_ARGS=--release"
    set "GUI_SOURCE_EXE=%CARGO_OUTPUT_DIR%\release\agenterm.exe"
    set "CLI_SOURCE_EXE=%CARGO_OUTPUT_DIR%\release\agentermctl.exe"
    set "MUX_SOURCE_EXE=%CARGO_OUTPUT_DIR%\release\agenterm-mux.exe"
    set "SCRIPT_SOURCE_EXE=%CARGO_OUTPUT_DIR%\release\agenterm-script.exe"
) else if not "%~1"=="" (
    echo Usage: build.bat [release]
    popd
    exit /b 2
)

cargo build %CARGO_ARGS%
if errorlevel 1 (
    echo.
    echo AgenTerm %PROFILE% build failed.
    popd
    exit /b 1
)

if not exist "%DIST_DIR%" mkdir "%DIST_DIR%"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\stage-artifact.ps1" ^
    -Source "%GUI_SOURCE_EXE%" -Destination "%DIST_DIR%\agenterm.exe"
if errorlevel 1 (
    echo.
    echo Failed to copy agenterm.exe to dist.
    popd
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\stage-artifact.ps1" ^
    -Source "%CLI_SOURCE_EXE%" -Destination "%DIST_DIR%\agentermctl.exe"
if errorlevel 1 (
    echo.
    echo Failed to copy agentermctl.exe to dist.
    popd
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\stage-artifact.ps1" ^
    -Source "%MUX_SOURCE_EXE%" -Destination "%DIST_DIR%\agenterm-mux.exe"
if errorlevel 1 (
    echo.
    echo Failed to copy agenterm-mux.exe to dist.
    popd
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\stage-artifact.ps1" ^
    -Source "%SCRIPT_SOURCE_EXE%" -Destination "%DIST_DIR%\agenterm-script.exe"
if errorlevel 1 (
    echo.
    echo Failed to copy agenterm-script.exe to dist.
    popd
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\write-build-metadata.ps1" ^
    -ManifestPath "%DIST_DIR%\agenterm.json" ^
    -ExecutablePath "%DIST_DIR%\agenterm.exe" ^
    -CliExecutablePath "%DIST_DIR%\agentermctl.exe" ^
    -MuxExecutablePath "%DIST_DIR%\agenterm-mux.exe" ^
    -ScriptExecutablePath "%DIST_DIR%\agenterm-script.exe" ^
    -Profile "%PROFILE%"
if errorlevel 1 (
    echo.
    echo Failed to generate agenterm.json.
    popd
    exit /b 1
)

echo.
echo Built:    %DIST_DIR%\agenterm.exe [%PROFILE%]
echo CLI:      %DIST_DIR%\agentermctl.exe
echo Mux:      %DIST_DIR%\agenterm-mux.exe
echo Script:   %DIST_DIR%\agenterm-script.exe
echo Metadata: %DIST_DIR%\agenterm.json
popd
exit /b 0
