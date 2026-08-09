@echo off
setlocal
if "%AGENTERM_BOOTSTRAP_TASK%"=="" exit /b 2
call :clock_cs AGENTERM_BOOTSTRAP_START_CS

for /f "usebackq delims=" %%I in (`git -C "%~dp0." rev-parse --show-toplevel 2^>nul`) do set "AGENTERM_BOOTSTRAP_REPO=%%I"
if "%AGENTERM_BOOTSTRAP_REPO%"=="" exit /b 2
pushd "%AGENTERM_BOOTSTRAP_REPO%"

set "AGENTERM_BOOTSTRAP_TARGET=target"
if defined CARGO_TARGET_DIR set "AGENTERM_BOOTSTRAP_TARGET=%CARGO_TARGET_DIR%"
set "AGENTERM_BOOTSTRAP_SOURCE=%AGENTERM_BOOTSTRAP_TARGET%\debug\agenterm.exe"
rem A staged release reclaims repository Cargo targets. Keep this content-
rem validated worker outside Cargo output so Windows never holds an in-target
rem executable open while release cleanup runs.
if not defined AGENTERM_BOOTSTRAP_CACHE_ROOT set "AGENTERM_BOOTSTRAP_CACHE_ROOT=%LOCALAPPDATA%\AgenTerm\build-cache"
if "%AGENTERM_BOOTSTRAP_CACHE_ROOT%"=="\AgenTerm\build-cache" set "AGENTERM_BOOTSTRAP_CACHE_ROOT=%TEMP%\AgenTerm-build-cache"
set "AGENTERM_BOOTSTRAP_CACHE_DIR=%AGENTERM_BOOTSTRAP_CACHE_ROOT%\windows-%PROCESSOR_ARCHITECTURE%"
set "AGENTERM_BOOTSTRAP_CACHE_WORKER=%AGENTERM_BOOTSTRAP_CACHE_DIR%\agenterm.exe"
set "AGENTERM_BOOTSTRAP_CACHE_STAMP=%AGENTERM_BOOTSTRAP_CACHE_DIR%\worker.stamp"
set "AGENTERM_BOOTSTRAP_IDENTITY=%AGENTERM_BOOTSTRAP_CACHE_DIR%\identity-%RANDOM%-%RANDOM%.txt"
set "AGENTERM_BOOTSTRAP_POST_IDENTITY=%AGENTERM_BOOTSTRAP_CACHE_DIR%\post-identity-%RANDOM%-%RANDOM%.txt"
set "AGENTERM_BOOTSTRAP_UNTRACKED=%AGENTERM_BOOTSTRAP_CACHE_DIR%\untracked-%RANDOM%-%RANDOM%.txt"
set "AGENTERM_BOOTSTRAP_CACHE_TEMP=%AGENTERM_BOOTSTRAP_CACHE_DIR%\worker-%RANDOM%-%RANDOM%.tmp"
set "AGENTERM_BOOTSTRAP_STAMP_TEMP=%AGENTERM_BOOTSTRAP_CACHE_DIR%\stamp-%RANDOM%-%RANDOM%.tmp"
mkdir "%AGENTERM_BOOTSTRAP_CACHE_DIR%" >nul 2>nul
rem Worker reuse uses source content and tool identities, never timestamps.
call :write_identity "%AGENTERM_BOOTSTRAP_IDENTITY%"
if errorlevel 1 goto :failed
for /f "delims=" %%H in ('git hash-object -- "%AGENTERM_BOOTSTRAP_IDENTITY%"') do set "AGENTERM_BOOTSTRAP_FINGERPRINT=%%H"
if not defined AGENTERM_BOOTSTRAP_FINGERPRINT goto :failed

call :validate_cache
if defined AGENTERM_BOOTSTRAP_CACHE_VALID goto :cache_ready
call :clock_cs AGENTERM_BOOTSTRAP_CARGO_START_CS
cargo build --quiet --locked --bin agenterm
if errorlevel 1 goto :failed
call :clock_cs AGENTERM_BOOTSTRAP_CARGO_END_CS
call :elapsed_cs %AGENTERM_BOOTSTRAP_CARGO_START_CS% %AGENTERM_BOOTSTRAP_CARGO_END_CS% AGENTERM_BOOTSTRAP_CARGO_CS
set /a AGENTERM_BOOTSTRAP_CARGO_BUILD_MS=AGENTERM_BOOTSTRAP_CARGO_CS*10
call :write_identity "%AGENTERM_BOOTSTRAP_POST_IDENTITY%"
if errorlevel 1 goto :failed
for /f "delims=" %%H in ('git hash-object -- "%AGENTERM_BOOTSTRAP_POST_IDENTITY%"') do set "AGENTERM_BOOTSTRAP_POST_FINGERPRINT=%%H"
if not "%AGENTERM_BOOTSTRAP_POST_FINGERPRINT%"=="%AGENTERM_BOOTSTRAP_FINGERPRINT%" goto :source_changed
copy /y "%AGENTERM_BOOTSTRAP_SOURCE%" "%AGENTERM_BOOTSTRAP_CACHE_TEMP%" >nul
if errorlevel 1 goto :failed
for /f "delims=" %%H in ('git hash-object -- "%AGENTERM_BOOTSTRAP_CACHE_TEMP%"') do set "AGENTERM_BOOTSTRAP_CACHE_HASH=%%H"
if not defined AGENTERM_BOOTSTRAP_CACHE_HASH goto :failed
"%AGENTERM_BOOTSTRAP_CACHE_TEMP%" rh version >nul
if errorlevel 1 goto :failed
> "%AGENTERM_BOOTSTRAP_STAMP_TEMP%" echo 2 %AGENTERM_BOOTSTRAP_FINGERPRINT% %AGENTERM_BOOTSTRAP_CACHE_HASH%
move /y "%AGENTERM_BOOTSTRAP_CACHE_TEMP%" "%AGENTERM_BOOTSTRAP_CACHE_WORKER%" >nul
if errorlevel 1 goto :failed
move /y "%AGENTERM_BOOTSTRAP_STAMP_TEMP%" "%AGENTERM_BOOTSTRAP_CACHE_STAMP%" >nul
if errorlevel 1 goto :failed
set "AGENTERM_BOOTSTRAP_WORKER_STATE=rebuilt"
set "AGENTERM_BOOTSTRAP_LOCK_WAIT_STATE=included_not_separable"
goto :copy_worker

:cache_ready
set "AGENTERM_BOOTSTRAP_CARGO_BUILD_MS=0"
set "AGENTERM_BOOTSTRAP_WORKER_STATE=reused"
set "AGENTERM_BOOTSTRAP_LOCK_WAIT_STATE=not_applicable"

:copy_worker
set "AGENTERM_BOOTSTRAP_DIR=%AGENTERM_BOOTSTRAP_CACHE_DIR%\task-%RANDOM%-%RANDOM%"
call :clock_cs AGENTERM_BOOTSTRAP_COPY_START_CS
mkdir "%AGENTERM_BOOTSTRAP_DIR%" >nul 2>nul
copy /y "%AGENTERM_BOOTSTRAP_CACHE_WORKER%" "%AGENTERM_BOOTSTRAP_DIR%\agenterm.exe" >nul
if errorlevel 1 goto :failed
for /f "delims=" %%H in ('git hash-object -- "%AGENTERM_BOOTSTRAP_DIR%\agenterm.exe"') do set "AGENTERM_BOOTSTRAP_INVOKED_HASH=%%H"
if not "%AGENTERM_BOOTSTRAP_INVOKED_HASH%"=="%AGENTERM_BOOTSTRAP_CACHE_HASH%" goto :failed
call :clock_cs AGENTERM_BOOTSTRAP_COPY_END_CS
call :elapsed_cs %AGENTERM_BOOTSTRAP_COPY_START_CS% %AGENTERM_BOOTSTRAP_COPY_END_CS% AGENTERM_BOOTSTRAP_COPY_CS
set /a AGENTERM_BOOTSTRAP_WORKER_COPY_MS=AGENTERM_BOOTSTRAP_COPY_CS*10
set "AGENTERM_BOOTSTRAP_WORKER=%AGENTERM_BOOTSTRAP_DIR%\agenterm.exe"
call :clock_cs AGENTERM_BOOTSTRAP_SETUP_END_CS
call :elapsed_cs %AGENTERM_BOOTSTRAP_START_CS% %AGENTERM_BOOTSTRAP_SETUP_END_CS% AGENTERM_BOOTSTRAP_SETUP_CS
set /a AGENTERM_BOOTSTRAP_SETUP_MS=AGENTERM_BOOTSTRAP_SETUP_CS*10
set /a AGENTERM_BOOTSTRAP_OTHER_SETUP_MS=AGENTERM_BOOTSTRAP_SETUP_MS-AGENTERM_BOOTSTRAP_CARGO_BUILD_MS-AGENTERM_BOOTSTRAP_WORKER_COPY_MS
if %AGENTERM_BOOTSTRAP_OTHER_SETUP_MS% LSS 0 set "AGENTERM_BOOTSTRAP_OTHER_SETUP_MS=0"
set "AGENTERM_SCRIPT_BACKEND=rh"
set "AGENTERM_BOOTSTRAP_TIMING_SCHEMA=1"
set "AGENTERM_BOOTSTRAP_CLOCK_RESOLUTION_MS=10"
set "AGENTERM_BOOTSTRAP_CACHE_STAMP=%AGENTERM_BOOTSTRAP_CACHE_STAMP%"

"%AGENTERM_BOOTSTRAP_WORKER%" rh task run "%AGENTERM_BOOTSTRAP_TASK%" --manifest "%AGENTERM_BOOTSTRAP_REPO%\agenterm.tasks.json" -- %*
if errorlevel 1 goto :failed
call :cleanup
popd
exit /b 0

:source_changed
echo bootstrap worker inputs changed during build 1>&2
set "AGENTERM_BOOTSTRAP_EXIT=2"
goto :failed_known

:failed
set "AGENTERM_BOOTSTRAP_EXIT=%errorlevel%"
if "%AGENTERM_BOOTSTRAP_EXIT%"=="0" set "AGENTERM_BOOTSTRAP_EXIT=2"
:failed_known
call :cleanup
popd
exit /b %AGENTERM_BOOTSTRAP_EXIT%

:cleanup
if defined AGENTERM_BOOTSTRAP_WORKER del /q "%AGENTERM_BOOTSTRAP_WORKER%" >nul 2>nul
if defined AGENTERM_BOOTSTRAP_DIR rmdir "%AGENTERM_BOOTSTRAP_DIR%" >nul 2>nul
if defined AGENTERM_BOOTSTRAP_IDENTITY del /q "%AGENTERM_BOOTSTRAP_IDENTITY%" >nul 2>nul
if defined AGENTERM_BOOTSTRAP_POST_IDENTITY del /q "%AGENTERM_BOOTSTRAP_POST_IDENTITY%" >nul 2>nul
if defined AGENTERM_BOOTSTRAP_UNTRACKED del /q "%AGENTERM_BOOTSTRAP_UNTRACKED%" >nul 2>nul
if defined AGENTERM_BOOTSTRAP_CACHE_TEMP del /q "%AGENTERM_BOOTSTRAP_CACHE_TEMP%" >nul 2>nul
if defined AGENTERM_BOOTSTRAP_STAMP_TEMP del /q "%AGENTERM_BOOTSTRAP_STAMP_TEMP%" >nul 2>nul
exit /b 0

:write_identity
> "%~1" echo bootstrap_worker_build_schema=3
>> "%~1" rustc -Vv
if errorlevel 1 exit /b 1
>> "%~1" cargo -Vv
if errorlevel 1 exit /b 1
>> "%~1" echo tracked-index
        >> "%~1" git ls-files -s -- Cargo.toml Cargo.lock build.rs rust-toolchain.toml .cargo crates src docs/agenterm-rhai-runtime.md assets/agenterm.ico agenterm.tasks.json
if errorlevel 1 exit /b 1
>> "%~1" echo tracked-worktree
        >> "%~1" git diff --no-ext-diff --binary -- Cargo.toml Cargo.lock build.rs rust-toolchain.toml .cargo crates src docs/agenterm-rhai-runtime.md assets/agenterm.ico agenterm.tasks.json
if errorlevel 1 exit /b 1
> "%AGENTERM_BOOTSTRAP_UNTRACKED%" git ls-files --others --exclude-standard -- Cargo.toml Cargo.lock build.rs rust-toolchain.toml .cargo crates src docs/agenterm-rhai-runtime.md assets/agenterm.ico agenterm.tasks.json
if errorlevel 1 exit /b 1
>> "%~1" echo untracked-paths
>> "%~1" type "%AGENTERM_BOOTSTRAP_UNTRACKED%"
>> "%~1" echo untracked-content
git hash-object --stdin-paths < "%AGENTERM_BOOTSTRAP_UNTRACKED%" >> "%~1"
if errorlevel 1 exit /b 1
exit /b 0

:validate_cache
set "AGENTERM_BOOTSTRAP_CACHE_VALID="
if not exist "%AGENTERM_BOOTSTRAP_CACHE_WORKER%" exit /b 0
if not exist "%AGENTERM_BOOTSTRAP_CACHE_STAMP%" exit /b 0
set "AGENTERM_BOOTSTRAP_STAMP_SCHEMA="
set "AGENTERM_BOOTSTRAP_STAMP_FINGERPRINT="
set "AGENTERM_BOOTSTRAP_STAMP_HASH="
set "AGENTERM_BOOTSTRAP_STAMP_EXTRA="
set "AGENTERM_BOOTSTRAP_STAMP_RECORDS=0"
for /f "usebackq tokens=1-4" %%A in ("%AGENTERM_BOOTSTRAP_CACHE_STAMP%") do call :read_stamp "%%A" "%%B" "%%C" "%%D"
if not "%AGENTERM_BOOTSTRAP_STAMP_RECORDS%"=="1" exit /b 0
if not "%AGENTERM_BOOTSTRAP_STAMP_SCHEMA%"=="2" exit /b 0
if defined AGENTERM_BOOTSTRAP_STAMP_EXTRA exit /b 0
set "AGENTERM_BOOTSTRAP_ACTUAL_CACHE_HASH="
for /f "delims=" %%H in ('git hash-object -- "%AGENTERM_BOOTSTRAP_CACHE_WORKER%"') do set "AGENTERM_BOOTSTRAP_ACTUAL_CACHE_HASH=%%H"
if not "%AGENTERM_BOOTSTRAP_ACTUAL_CACHE_HASH%"=="%AGENTERM_BOOTSTRAP_STAMP_HASH%" exit /b 0
set "AGENTERM_BOOTSTRAP_CACHE_HASH=%AGENTERM_BOOTSTRAP_STAMP_HASH%"
set "AGENTERM_BOOTSTRAP_CACHE_VALID=1"
exit /b 0

:read_stamp
set /a AGENTERM_BOOTSTRAP_STAMP_RECORDS+=1
set "AGENTERM_BOOTSTRAP_STAMP_SCHEMA=%~1"
set "AGENTERM_BOOTSTRAP_STAMP_FINGERPRINT=%~2"
set "AGENTERM_BOOTSTRAP_STAMP_HASH=%~3"
set "AGENTERM_BOOTSTRAP_STAMP_EXTRA=%~4"
exit /b 0

:clock_cs
for /f "tokens=1-4 delims=:.," %%a in ("%time: =0%") do set /a AGENTERM_BOOTSTRAP_CLOCK_CS=(((1%%a-100)*60+1%%b-100)*60+1%%c-100)*100+1%%d-100
set "%~1=%AGENTERM_BOOTSTRAP_CLOCK_CS%"
exit /b 0

:elapsed_cs
set /a AGENTERM_BOOTSTRAP_ELAPSED_CS=%~2-%~1
if %AGENTERM_BOOTSTRAP_ELAPSED_CS% LSS 0 set /a AGENTERM_BOOTSTRAP_ELAPSED_CS+=8640000
set "%~3=%AGENTERM_BOOTSTRAP_ELAPSED_CS%"
exit /b 0
