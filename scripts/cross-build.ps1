param([ValidateSet('windows-x86','windows-arm64','linux-x64','linux-x86','linux-arm64')][string]$Platform)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    $env:RUSTUP_HOME="$projectRoot\tools\rustup"
    $env:CARGO_HOME="$projectRoot\tools\cargo"
    $env:PATH="$projectRoot\tools\cargo\bin;$projectRoot\tools\mingw64\bin;$env:PATH"
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$projectRoot\tools\mingw64\bin\gcc.exe"
    switch ($Platform) {
        'windows-x86' {
            $env:RC_i686_pc_windows_gnu="$projectRoot\tools\mingw32\bin\windres.exe"
            $env:CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER="$projectRoot\tools\mingw32\bin\gcc.exe"
            & cargo build --release --locked --offline --target i686-pc-windows-gnu
        }
        'windows-arm64' {
            $env:PATH="$projectRoot\tools\llvm-mingw-20261006-ucrt-x86_64\bin;$env:PATH"
            $env:RC_aarch64_pc_windows_gnullvm="$projectRoot\tools\llvm-mingw-20261006-ucrt-x86_64\bin\aarch64-w64-mingw32-windres.exe"
            $env:CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER="$projectRoot\tools\llvm-mingw-20261006-ucrt-x86_64\bin\aarch64-w64-mingw32-clang.exe"
            & cargo build --release --locked --offline --target aarch64-pc-windows-gnullvm
        }
        default {
            $target=switch ($Platform) {'linux-x64' {'x86_64-unknown-linux-gnu.2.28'} 'linux-x86' {'i686-unknown-linux-gnu.2.28'} 'linux-arm64' {'aarch64-unknown-linux-gnu.2.28'}}
            $env:CARGO_ZIGBUILD_ZIG_PATH="$projectRoot\tools\zig-x86_64-windows-0.15.2\zig.exe"
            $env:CARGO_ZIGBUILD_CACHE_DIR="$projectRoot\tools\zigbuild-cache"
            $env:ZIG_GLOBAL_CACHE_DIR="$projectRoot\tools\zig-global-cache"
            $env:ZIG_LOCAL_CACHE_DIR="$projectRoot\tmp\zig-local-cache"
            & "$projectRoot\tools\zigbuild\cargo-zigbuild.exe" zigbuild --release --locked --offline --target $target
        }
    }
    if ($LASTEXITCODE -ne 0) {throw "Cross build failed: $LASTEXITCODE"}
} finally {Pop-Location}
