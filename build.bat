@echo off
setlocal
pushd "%~dp0"

set "PROFILE=dev"
set "CARGO_ARGS="
set "SOURCE_EXE=target\debug\agenterm.exe"

if /i "%~1"=="release" (
    set "PROFILE=release"
    set "CARGO_ARGS=--release"
    set "SOURCE_EXE=target\release\agenterm.exe"
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

copy /y "%SOURCE_EXE%" "agenterm.exe" >nul
if errorlevel 1 (
    echo.
    echo Failed to copy agenterm.exe to the repository root.
    popd
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\write-build-metadata.ps1" ^
    -ManifestPath "%~dp0agenterm.json" ^
    -ExecutablePath "%~dp0agenterm.exe" ^
    -Profile "%PROFILE%"
if errorlevel 1 (
    echo.
    echo Failed to generate agenterm.json.
    popd
    exit /b 1
)

echo.
echo Built:    %CD%\agenterm.exe [%PROFILE%]
echo Metadata: %CD%\agenterm.json
popd
exit /b 0
