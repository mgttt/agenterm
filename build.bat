@echo off
setlocal
pushd "%~dp0"

set "PROFILE=dev"
set "CARGO_ARGS="
set "SOURCE_EXE=target\debug\agenterm.exe"
set "DIST_DIR=%~dp0dist"

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

if not exist "%DIST_DIR%" mkdir "%DIST_DIR%"
copy /y "%SOURCE_EXE%" "%DIST_DIR%\agenterm.exe" >nul
if errorlevel 1 (
    echo.
    echo Failed to copy agenterm.exe to dist.
    popd
    exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\write-build-metadata.ps1" ^
    -ManifestPath "%DIST_DIR%\agenterm.json" ^
    -ExecutablePath "%DIST_DIR%\agenterm.exe" ^
    -Profile "%PROFILE%"
if errorlevel 1 (
    echo.
    echo Failed to generate agenterm.json.
    popd
    exit /b 1
)

echo.
echo Built:    %DIST_DIR%\agenterm.exe [%PROFILE%]
echo Metadata: %DIST_DIR%\agenterm.json
popd
exit /b 0
