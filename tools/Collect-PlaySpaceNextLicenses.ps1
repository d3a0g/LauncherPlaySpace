[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$env:RUSTUP_HOME=Join-Path $workspace '.tools/rustup'
$env:CARGO_HOME=Join-Path $workspace '.tools/cargo'
$cargo=Join-Path $env:CARGO_HOME 'bin/cargo.exe'
$manifest=Join-Path $workspace 'PlaySpaceNext/src-tauri/Cargo.toml'
$json=& $cargo metadata --offline --format-version 1 --manifest-path $manifest
if($LASTEXITCODE -ne 0){throw 'Dependency metadata failed.'}
$metadata=$json | ConvertFrom-Json
$destination=Join-Path $workspace 'artifacts/playspace-next-native/app/licenses'
[void][IO.Directory]::CreateDirectory($destination)
$inventory=@()
foreach($package in $metadata.packages){
    if(-not $package.source){continue}
    $name=$package.name+'-'+$package.version
    if($name -notmatch '^[A-Za-z0-9_.+-]+$'){throw 'Invalid dependency name.'}
    $packageRoot=Split-Path -Parent $package.manifest_path
    $licenseFiles=@(Get-ChildItem -LiteralPath $packageRoot -File | Where-Object Name -Match '^(LICENSE|LICENCE|COPYING|NOTICE|UNLICENSE)([-._].*)?$')
    $out=Join-Path $destination $name
    [void][IO.Directory]::CreateDirectory($out)
    foreach($file in $licenseFiles){Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $out $file.Name) -Force}
    $inventory+=@{name=$package.name;version=$package.version;license=$package.license;repository=$package.repository;licenseFiles=@($licenseFiles.Name)}
}
$nodeModules=Join-Path $workspace 'PlaySpaceNext/node_modules'
foreach($packageName in @('react','react-dom','scheduler','lucide-react','typescript','vite','sass','@tauri-apps/api','@tauri-apps/plugin-dialog')){
    $root=Join-Path $nodeModules $packageName
    if(-not(Test-Path -LiteralPath $root)){continue}
    $out=Join-Path $destination ('javascript/'+$packageName.Replace('/','_'))
    [void][IO.Directory]::CreateDirectory($out)
    foreach($file in Get-ChildItem -LiteralPath $root -File | Where-Object Name -Match '^(LICENSE|LICENCE|COPYING|NOTICE)([-._].*)?$'){Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $out $file.Name) -Force}
}
[IO.File]::WriteAllText((Join-Path $destination 'DEPENDENCIES.json'),(ConvertTo-Json -InputObject @($inventory) -Depth 5),[Text.UTF8Encoding]::new($false))
Write-Output ('Collected notices for '+$inventory.Count+' Rust dependencies and JavaScript runtime/build packages.')
