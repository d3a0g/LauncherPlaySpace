[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$toolRoot=Join-Path $workspace '.tools'
[void][IO.Directory]::CreateDirectory($toolRoot)
$env:RUSTUP_HOME=Join-Path $toolRoot 'rustup'
$env:CARGO_HOME=Join-Path $toolRoot 'cargo'
$rustup=Join-Path $toolRoot 'rustup-init.exe'
if(-not(Test-Path (Join-Path $env:CARGO_HOME 'bin/cargo.exe'))){
    Invoke-WebRequest -UseBasicParsing 'https://win.rustup.rs/x86_64' -OutFile $rustup
    & $rustup -y --no-modify-path --default-toolchain stable-x86_64-pc-windows-msvc --profile minimal
    if($LASTEXITCODE -ne 0){throw 'Rust setup failed.'}
}
$vswhere='C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe'
$cpp=$null
if(Test-Path $vswhere){$cpp=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath}
if(-not $cpp){
    $installer=Join-Path $toolRoot 'vs_BuildTools.exe'
    Invoke-WebRequest -UseBasicParsing 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -OutFile $installer
    $destination=Join-Path $toolRoot 'VSBuildTools'
    $process=Start-Process -FilePath $installer -ArgumentList @('--quiet','--wait','--norestart','--nocache','--installPath',('"'+$destination+'"'),'--add','Microsoft.VisualStudio.Workload.VCTools','--includeRecommended') -WindowStyle Hidden -Wait -PassThru
    if($process.ExitCode -notin @(0,3010)){throw ('C++ Build Tools setup exit code: '+$process.ExitCode)}
}
Write-Output 'Rust and C++ prerequisites prepared. User PATH was not modified.'
