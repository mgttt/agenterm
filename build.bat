@echo off
setlocal
pushd "%~dp0"

set "PROFILE=dev"
set "CARGO_ARGS="
set "CARGO_OUTPUT_DIR=target"
if defined CARGO_TARGET_DIR set "CARGO_OUTPUT_DIR=%CARGO_TARGET_DIR%"
set "CARGO_PROFILE_DIR=%CARGO_OUTPUT_DIR%\debug"
set "DIST_DIR=%~dp0dist"
set "POWERSHELL_EXE=powershell.exe"
where pwsh.exe >nul 2>nul
if not errorlevel 1 set "POWERSHELL_EXE=pwsh.exe"

if /i "%~1"=="release" (
    set "PROFILE=release"
    set "CARGO_ARGS=--release"
    set "CARGO_PROFILE_DIR=%CARGO_OUTPUT_DIR%\release"
) else if /i "%~1"=="release-fast" (
    set "PROFILE=release-fast"
    set "CARGO_ARGS=--profile release-fast"
    set "CARGO_PROFILE_DIR=%CARGO_OUTPUT_DIR%\release-fast"
) else if not "%~1"=="" (
    echo Usage: build.bat [release^|release-fast]
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

"%POWERSHELL_EXE%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\stage-build.ps1" ^
    -SourceDirectory "%CARGO_PROFILE_DIR%" ^
    -DestinationDirectory "%DIST_DIR%" ^
    -Profile "%PROFILE%"
if errorlevel 1 (
    echo.
    echo Failed to stage AgenTerm artifacts and metadata.
    popd
    exit /b 1
)

if /i "%PROFILE%"=="release" (
    cargo clean
    if errorlevel 1 (
        echo.
        echo Release artifacts were staged, but the Cargo target cleanup failed.
        popd
        exit /b 1
    )
)

echo.
echo Built:    %DIST_DIR%\agenterm.exe [%PROFILE%]
echo CLI:      %DIST_DIR%\agenterm-cli.exe
echo Mux:      %DIST_DIR%\agenterm-mux.exe
echo Script:   %DIST_DIR%\agenterm-script.exe
echo Metadata: %DIST_DIR%\agenterm.json
popd
exit /b 0
