[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$env:RUSTUP_HOME=Join-Path $workspace '.tools/rustup'
$env:CARGO_HOME=Join-Path $workspace '.tools/cargo'
$env:PATH=(Join-Path $env:CARGO_HOME 'bin')+';'+$env:PATH
$vcvars=Join-Path $workspace '.tools/VSBuildTools/VC/Auxiliary/Build/vcvars64.bat'
if(Test-Path -LiteralPath $vcvars){
    $lines=& $env:ComSpec /d /s /c ('""'+$vcvars+'" >nul && set"')
    if($LASTEXITCODE -ne 0){throw 'C++ build environment failed.'}
    foreach($line in $lines){$split=$line.IndexOf('=');if($split -gt 0){$name=$line.Substring(0,$split);if($name -notin @('HOME','CODEX_HOME')){[Environment]::SetEnvironmentVariable($name,$line.Substring($split+1),'Process')}}}
}
Push-Location (Join-Path $workspace 'PlaySpaceNext/src-tauri')
try{& cargo.exe test --offline;if($LASTEXITCODE -ne 0){throw 'Native service tests failed.'}}finally{Pop-Location}
