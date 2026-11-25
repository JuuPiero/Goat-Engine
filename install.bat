@echo off
rem GoatEngine - install what the engine, the editor and the tools need.
rem
rem   install.bat          editor + desktop (wasm target, wasm-bindgen, npm packages, goat-cs, goat CLI)
rem   install.bat 3ds      also the Nintendo 3DS toolchain (Rust nightly + rust-src, cargo-3ds)
rem
rem Needs (install them yourself first): Rust (https://rustup.rs), Node.js 20+, .NET SDK 9 (C# scripts, optional).
rem 3DS also needs devkitPro (3ds-dev) and LLVM; their paths go in goat.local.toml (see README).
setlocal
cd /d "%~dp0"
set WASM_BINDGEN_VERSION=0.2.130

echo === Checking tools ===
where cargo >nul 2>nul || (echo [x] Rust is missing: install it from https://rustup.rs & goto :fail)
where rustup >nul 2>nul || (echo [x] rustup is missing: install Rust from https://rustup.rs & goto :fail)
where npm >nul 2>nul || (echo [x] Node.js is missing: install it from https://nodejs.org & goto :fail)
set HAVE_DOTNET=1
where dotnet >nul 2>nul || set HAVE_DOTNET=0
if "%HAVE_DOTNET%"=="0" echo [!] .NET SDK not found: C# scripts won't be translated (https://dotnet.microsoft.com/download)

echo.
echo === Rust target for the editor's engine (wasm32) ===
rustup target add wasm32-unknown-unknown || goto :fail

echo.
echo === wasm-bindgen-cli %WASM_BINDGEN_VERSION% ===
set BINDGEN_OK=0
for /f "tokens=2" %%v in ('wasm-bindgen --version 2^>nul') do if "%%v"=="%WASM_BINDGEN_VERSION%" set BINDGEN_OK=1
if "%BINDGEN_OK%"=="1" (
    echo already installed
) else (
    cargo install wasm-bindgen-cli --version %WASM_BINDGEN_VERSION% --locked || goto :fail
)

echo.
echo === Editor npm packages ===
pushd editor
call npm install || (popd & goto :fail)
popd

if "%HAVE_DOTNET%"=="1" (
    echo.
    echo === goat-cs: C# to JavaScript translator ===
    pushd tools\goat-cs
    dotnet build -c Release -o bin || (popd & goto :fail)
    popd
)

if /i "%~1"=="3ds" (
    echo.
    echo === Nintendo 3DS toolchain ===
    rustup toolchain install nightly --component rust-src || goto :fail
    where cargo-3ds >nul 2>nul && (echo cargo-3ds already installed) || (cargo install --locked cargo-3ds || goto :fail)
)

echo.
echo === goat CLI ===
cargo build --release -p goat || goto :fail

echo.
echo === Toolchain check (goat doctor) ===
target\release\goat.exe doctor

echo.
echo Done. Next: run.bat (editor) or build.bat.
exit /b 0

:fail
echo.
echo Install failed.
exit /b 1
