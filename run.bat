@echo off
rem GoatEngine - run.
rem
rem   run.bat                         the editor: the Project Hub (recent projects, examples, New project)
rem   run.bat editor [project-dir]    the editor on a project (a folder holding assets\)
rem   run.bat desktop [args...]       the game in a desktop window (e.g. run.bat desktop --layout 3ds)
rem   run.bat 3ds                     the game in the 3DS emulator set in goat.local.toml (citra = ...)
rem
rem The editor runs in development mode (Vite + Tauri): close its window to stop.
setlocal
cd /d "%~dp0"

set WHAT=%~1
if "%WHAT%"=="" set WHAT=editor

if /i "%WHAT%"=="editor" goto :editor
if /i "%WHAT%"=="desktop" goto :game
if /i "%WHAT%"=="3ds" goto :game
echo Unknown target "%WHAT%". Use: run.bat [editor [project-dir] ^| desktop [args...] ^| 3ds]
exit /b 2

:editor
if not exist editor\node_modules (echo Run install.bat first. & exit /b 1)
call :engine_wasm || goto :fail
set PROJECT_ARGS=
if not "%~2"=="" set PROJECT_ARGS=-- -- --project "%~f2"
pushd editor
rem tauri dev starts the Vite server (beforeDevCommand), builds goat-editor and opens it
call node_modules\.bin\tauri.cmd dev %PROJECT_ARGS%
set RESULT=%ERRORLEVEL%
popd
exit /b %RESULT%

:game
call :goat_cli || goto :fail
rem everything after the platform goes to the game
set GAME_ARGS=
shift
:collect
if "%~1"=="" goto :run_game
set GAME_ARGS=%GAME_ARGS% %1
shift
goto :collect
:run_game
if defined GAME_ARGS (
    target\release\goat.exe run %WHAT% --%GAME_ARGS%
) else (
    target\release\goat.exe run %WHAT%
)
exit /b %ERRORLEVEL%

rem ---------------------------------------------------------------- helpers
:engine_wasm
echo === Engine for the editor (wasm) ===
cargo build --target wasm32-unknown-unknown --release -p goat-editor-engine || exit /b 1
wasm-bindgen --target web --out-dir editor\src\engine-pkg target\wasm32-unknown-unknown\release\goat_editor_engine.wasm || exit /b 1
exit /b 0

:goat_cli
cargo build --release -p goat || exit /b 1
exit /b 0

:fail
echo.
echo Run failed.
exit /b 1
