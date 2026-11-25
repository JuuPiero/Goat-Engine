@echo off
rem GoatEngine - build.
rem
rem   build.bat                  the game for desktop and 3DS (3DS skipped when its toolchain isn't installed)
rem   build.bat desktop          out\desktop\goat-hello.exe
rem   build.bat 3ds              out\3ds\goat-hello.3dsx (+ .smdh)
rem   build.bat editor           out\editor\goat-editor.exe (standalone, no Vite server needed)
rem   build.bat all              desktop + 3ds + editor
rem   add --debug for a debug build of the game (desktop / 3ds)
setlocal EnableDelayedExpansion
cd /d "%~dp0"

set TARGETS=
set DEBUG=
for %%a in (%*) do (
    if /i "%%~a"=="--debug" (set DEBUG=--debug) else set TARGETS=!TARGETS! %%~a
)
if not defined TARGETS set TARGETS=game
set TARGETS=!TARGETS:all=game editor!

echo === goat CLI ===
cargo build --release -p goat || goto :fail

for %%t in (%TARGETS%) do (
    if /i "%%t"=="game" (
        call :game desktop || goto :fail
        target\release\goat.exe platforms | findstr /i "3ds" | findstr /i /c:"missing" >nul
        if errorlevel 1 (
            call :game 3ds || goto :fail
        ) else (
            echo.
            echo [!] 3DS skipped: toolchain not installed ^(install.bat 3ds, then goat doctor^)
        )
    ) else if /i "%%t"=="desktop" (
        call :game desktop || goto :fail
    ) else if /i "%%t"=="3ds" (
        call :game 3ds || goto :fail
    ) else if /i "%%t"=="editor" (
        call :editor || goto :fail
    ) else (
        echo Unknown target "%%t". Use: build.bat [desktop] [3ds] [editor] [all] [--debug]
        exit /b 2
    )
)

echo.
echo Build finished: see the out\ folder.
exit /b 0

rem ---------------------------------------------------------------- helpers
:game
echo.
echo === Game: %1 ===
target\release\goat.exe build %1 %DEBUG%
exit /b %ERRORLEVEL%

:editor
echo.
echo === Editor ===
if not exist editor\node_modules (echo Run install.bat first. & exit /b 1)
cargo build --target wasm32-unknown-unknown --release -p goat-editor-engine || exit /b 1
wasm-bindgen --target web --out-dir editor\src\engine-pkg target\wasm32-unknown-unknown\release\goat_editor_engine.wasm || exit /b 1
pushd editor
call npm run build
if errorlevel 1 (popd & exit /b 1)
popd
rem custom-protocol = serve the built UI (editor\dist) from the exe instead of the Vite dev server
cargo build --release -p goat-editor --features tauri/custom-protocol || exit /b 1
if not exist out\editor mkdir out\editor
copy /y target\release\goat-editor.exe out\editor\goat-editor.exe >nul || exit /b 1
echo out\editor\goat-editor.exe
exit /b 0

:fail
echo.
echo Build failed.
exit /b 1
