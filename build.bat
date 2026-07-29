@echo off
setlocal
pushd "%~dp0"

set "PROFILE=dev"
set "CARGO_ARGS="
set "CARGO_OUTPUT_DIR=target"
set "USING_EXTERNAL_CARGO_TARGET=0"
if defined CARGO_TARGET_DIR (
    set "CARGO_OUTPUT_DIR=%CARGO_TARGET_DIR%"
    set "USING_EXTERNAL_CARGO_TARGET=1"
)
set "CARGO_PROFILE_DIR=%CARGO_OUTPUT_DIR%\debug"
set "DIST_DIR=%~dp0dist"
set "POWERSHELL_EXE=powershell.exe"
where pwsh.exe >nul 2>nul
if not errorlevel 1 set "POWERSHELL_EXE=pwsh.exe"

if /i "%~1"=="release" (
    set "PROFILE=release"
    set "CARGO_ARGS=--release"
    if "%USING_EXTERNAL_CARGO_TARGET%"=="0" (
        set "CARGO_OUTPUT_DIR=target-release"
        set "CARGO_PROFILE_DIR=target-release\release"
        set "CARGO_TARGET_DIR=%CD%\target-release"
    ) else (
        set "CARGO_PROFILE_DIR=%CARGO_OUTPUT_DIR%\release"
    )
) else if /i "%~1"=="release-fast" (
    set "PROFILE=release-fast"
    set "CARGO_ARGS=--profile release-fast"
    set "CARGO_PROFILE_DIR=%CARGO_OUTPUT_DIR%\release-fast"
) else if not "%~1"=="" (
    echo Usage: build.bat [release^|release-fast]
    popd
    exit /b 2
)

set "BUILD_IDENTITY_ENV=%TEMP%\agenterm-build-identity-%RANDOM%-%RANDOM%.cmd"
"%POWERSHELL_EXE%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\build-identity.ps1" ^
    -Profile "%PROFILE%" ^
    -OutputPath "%BUILD_IDENTITY_ENV%"
if errorlevel 1 (
    echo.
    echo Failed to determine truthful AgenTerm build identity.
    popd
    exit /b 1
)
call "%BUILD_IDENTITY_ENV%"
set "BUILD_IDENTITY_RESULT=%ERRORLEVEL%"
del /q "%BUILD_IDENTITY_ENV%" >nul 2>nul
if not "%BUILD_IDENTITY_RESULT%"=="0" (
    echo.
    echo Failed to import AgenTerm build identity.
    popd
    exit /b 1
)

cargo build --locked %CARGO_ARGS%
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
    if "%USING_EXTERNAL_CARGO_TARGET%"=="1" (
        echo Skipped Cargo cleanup because CARGO_TARGET_DIR is externally configured.
    ) else (
        "%DIST_DIR%\agenterm-cli.exe" script run "%~dp0scripts\rhai\target-report.rhai" --profile local --timeout-ms 10000 --max-operations 10000000 -- "%CD%" "%CARGO_OUTPUT_DIR%"
        if errorlevel 1 (
            echo.
            echo Release artifacts were staged, but the Cargo target report failed.
            popd
            exit /b 1
        )
        "%POWERSHELL_EXE%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\prepare-target-clean.ps1" -RepoRoot "%CD%" -TargetPath "%CARGO_OUTPUT_DIR%"
        if errorlevel 1 (
            echo.
            echo Release artifacts were staged, but exact target cleanup preparation failed.
            popd
            exit /b 1
        )
        cargo clean --target-dir "%CARGO_OUTPUT_DIR%"
        if errorlevel 1 (
            echo.
            echo Release artifacts were staged, but the repository-local Cargo target cleanup failed.
            popd
            exit /b 1
        )
    )
)

echo.
echo Built:    %DIST_DIR%\agenterm.exe [%PROFILE%]
echo Server:   %DIST_DIR%\agenterm-server.exe
echo CLI:      %DIST_DIR%\agenterm-cli.exe
echo Mux:      %DIST_DIR%\agenterm-mux.exe
echo Script:   %DIST_DIR%\agenterm-script.exe
echo Metadata: %DIST_DIR%\agenterm.json
popd
exit /b 0
