[CmdletBinding()]
param([switch]$BuildOnly)
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$project=Join-Path $workspace 'PlaySpaceNext'
$env:RUSTUP_HOME=Join-Path $workspace '.tools/rustup'
$env:CARGO_HOME=Join-Path $workspace '.tools/cargo'
$env:npm_config_cache=Join-Path $workspace '.tools/npm-cache'
$env:PATH=(Join-Path $env:CARGO_HOME 'bin')+';'+$env:PATH
$vcvars=Join-Path $workspace '.tools/VSBuildTools/VC/Auxiliary/Build/vcvars64.bat'
if(Test-Path -LiteralPath $vcvars){
    $lines=& $env:ComSpec /d /s /c ('""'+$vcvars+'" >nul && set"')
    if($LASTEXITCODE -ne 0){throw 'C++ build environment is not ready. Finish Setup-PlaySpaceNext.ps1 first.'}
    foreach($line in $lines){$split=$line.IndexOf('=');if($split -gt 0){$name=$line.Substring(0,$split);if($name -notin @('HOME','CODEX_HOME')){[Environment]::SetEnvironmentVariable($name,$line.Substring($split+1),'Process')}}}
}
if(-not(Test-Path -LiteralPath (Join-Path $env:CARGO_HOME 'bin/cargo.exe'))){throw 'Run tools/Setup-PlaySpaceNext.ps1 to install Rust first.'}
Push-Location $project
try{
    if(-not(Test-Path -LiteralPath 'node_modules')){& npm.cmd install;if($LASTEXITCODE -ne 0){throw 'Dependency installation failed.'}}
    if($BuildOnly){& npm.cmd run tauri -- build --no-bundle}else{& npm.cmd run tauri -- dev}
    if($LASTEXITCODE -ne 0){throw 'PlaySpace Next build or launch failed.'}
}finally{Pop-Location}
