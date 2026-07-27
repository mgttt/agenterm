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
set "DIST_DIR=%~dp0dist"

if /i "%~1"=="release" (
    set "PROFILE=release"
    set "CARGO_ARGS=--release"
    set "GUI_SOURCE_EXE=%CARGO_OUTPUT_DIR%\release\agenterm.exe"
    set "CLI_SOURCE_EXE=%CARGO_OUTPUT_DIR%\release\agentermctl.exe"
    set "MUX_SOURCE_EXE=%CARGO_OUTPUT_DIR%\release\agenterm-mux.exe"
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
copy /y "%GUI_SOURCE_EXE%" "%DIST_DIR%\agenterm.exe" >nul
if errorlevel 1 (
    echo.
    echo Failed to copy agenterm.exe to dist.
    popd
    exit /b 1
)

copy /y "%CLI_SOURCE_EXE%" "%DIST_DIR%\agentermctl.exe" >nul
if errorlevel 1 (
    echo.
    echo Failed to copy agentermctl.exe to dist.
    popd
    exit /b 1
)

copy /y "%MUX_SOURCE_EXE%" "%DIST_DIR%\agenterm-mux.exe" >nul
if errorlevel 1 (
    echo.
    echo Failed to copy agenterm-mux.exe to dist.
    popd
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\write-build-metadata.ps1" ^
    -ManifestPath "%DIST_DIR%\agenterm.json" ^
    -ExecutablePath "%DIST_DIR%\agenterm.exe" ^
    -CliExecutablePath "%DIST_DIR%\agentermctl.exe" ^
    -MuxExecutablePath "%DIST_DIR%\agenterm-mux.exe" ^
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
echo Metadata: %DIST_DIR%\agenterm.json
popd
exit /b 0
