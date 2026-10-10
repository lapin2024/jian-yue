param([ValidateSet('check','test','build','release','fmt','clippy')][string]$Task='release',[string]$Target='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    if (Test-Path -LiteralPath "$projectRoot\tools\cargo\bin\cargo.exe") {
        $env:RUSTUP_HOME="$projectRoot\tools\rustup"
        $env:CARGO_HOME="$projectRoot\tools\cargo"
        $cargo="$projectRoot\tools\cargo\bin\cargo.exe"
        $env:PATH="$projectRoot\tools\cargo\bin;$env:PATH"
        if (Test-Path -LiteralPath "$projectRoot\tools\mingw64\bin\gcc.exe") {
            $env:PATH="$projectRoot\tools\mingw64\bin;$env:PATH"
            $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$projectRoot\tools\mingw64\bin\gcc.exe"
        }
    } else { $cargo='cargo' }
    $cargoArgs=switch($Task) {
        'release' { @('build','--release','--locked') }
        'test' { @('test','--locked') }
        'fmt' { @('fmt','--all','--','--check') }
        'clippy' { @('clippy','--all-targets','--locked','--','-D','warnings') }
        default { @($Task,'--locked') }
    }
    if ($Target -and $Task -notin @('fmt','clippy')) { $cargoArgs+=@('--target',$Target) }
    & $cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed: $LASTEXITCODE" }
} finally { Pop-Location }
