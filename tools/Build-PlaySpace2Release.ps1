[CmdletBinding()]
param([switch]$SkipBuild,[string]$WebViewBootstrap='artifacts/release-publication-2.0.0/MicrosoftEdgeWebview2Setup.exe')
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$version=(Get-Content -LiteralPath (Join-Path $workspace 'PlaySpaceNext/package.json') -Raw|ConvertFrom-Json).version
if($version -notmatch '^2\.\d+\.\d+$'){throw 'Expected a stable PlaySpace 2 version.'}
if(-not $SkipBuild){& (Join-Path $PSScriptRoot 'Start-PlaySpaceNext.ps1') -BuildOnly}
$folder=Join-Path $workspace "artifacts/releases/$version"
$app=Join-Path $folder 'app'
[void][IO.Directory]::CreateDirectory($app)
$bootstrap=Join-Path $workspace $WebViewBootstrap
$signature=Get-AuthenticodeSignature -LiteralPath $bootstrap
if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Corporation'){throw 'Microsoft WebView2 bootstrapper signature is invalid.'}
Copy-Item -LiteralPath (Join-Path $workspace 'PlaySpaceNext/src-tauri/target/release/playspace-next.exe') -Destination (Join-Path $app 'LauncherPlaySpace.exe') -Force
Copy-Item -LiteralPath (Join-Path $workspace 'LICENSE') -Destination (Join-Path $app 'LICENSE') -Force
Copy-Item -LiteralPath (Join-Path $workspace 'PlaySpaceNext/THIRD_PARTY_NOTICES.md') -Destination (Join-Path $app 'THIRD_PARTY_NOTICES.md') -Force
Copy-Item -LiteralPath (Join-Path $workspace 'PlaySpaceNext/RELEASE-2.0.txt') -Destination (Join-Path $app 'START-HERE.txt') -Force
Copy-Item -LiteralPath (Join-Path $workspace 'PlaySpaceNext/src-tauri/update-public-key.pub') -Destination (Join-Path $app 'update-public-key.pub') -Force
if(-not(Test-Path -LiteralPath (Join-Path $workspace 'artifacts/playspace-next-native/app/licenses/DEPENDENCIES.json'))){& (Join-Path $PSScriptRoot 'Collect-PlaySpaceNextLicenses.ps1')}
Copy-Item -LiteralPath (Join-Path $workspace 'artifacts/playspace-next-native/app/licenses') -Destination $app -Recurse -Force
$compiler=Join-Path $workspace '.tools/InnoSetup/ISCC.exe'
& $compiler ('/DAppVersion='+$version) ('/DPayloadDir='+$app) ('/DBuildOutput='+$folder) ('/DWebViewBootstrap='+$bootstrap) (Join-Path $workspace 'installer/PlaySpace2.iss') *> (Join-Path $folder 'installer-build.log')
if($LASTEXITCODE -ne 0){throw 'Native installer compilation failed. Inspect installer-build.log.'}
$installer=Join-Path $folder "LauncherPlaySpace-$version-Setup.exe"
$archive=Join-Path $folder "LauncherPlaySpace-$version-win-x64.zip"
foreach($file in Get-ChildItem -LiteralPath $app -File -Recurse){if($file.LastWriteTime.Year -lt 1980 -or $file.LastWriteTime.Year -gt 2107){$file.LastWriteTimeUtc=[DateTime]::new(2000,1,1,0,0,0,[DateTimeKind]::Utc)}}
Compress-Archive -Path (Join-Path $app '*') -DestinationPath $archive -CompressionLevel Optimal -Force
Add-Type -AssemblyName System.Security
$protected=Join-Path $workspace '.secrets/playspace-updater-v2.key.dpapi'
try{
    $env:TAURI_SIGNING_PRIVATE_KEY=[Text.Encoding]::UTF8.GetString([Security.Cryptography.ProtectedData]::Unprotect([IO.File]::ReadAllBytes($protected),$null,[Security.Cryptography.DataProtectionScope]::CurrentUser))
    Push-Location (Join-Path $workspace 'PlaySpaceNext')
    # Avoid Windows PowerShell's native stderr promotion; errors are checked by exit code.
    $previous=$ErrorActionPreference;$ErrorActionPreference='Continue'
    try{& node.exe node_modules/@tauri-apps/cli/tauri.js signer sign '--password=' --app-version $version $installer *> (Join-Path $folder 'signing.log');$signExit=$LASTEXITCODE}finally{$ErrorActionPreference=$previous;Pop-Location}
    if($signExit -ne 0){throw 'Update signing failed. Private signing material was not printed.'}
}finally{[Environment]::SetEnvironmentVariable('TAURI_SIGNING_PRIVATE_KEY',$null,'Process')}
$notes=[IO.File]::ReadAllText((Join-Path $workspace "docs/releases/$version.md"))
$notes=$notes.Replace('(../2.0/','(https://github.com/d3a0g/LauncherPlaySpace/blob/main/docs/2.0/')
[IO.File]::WriteAllText((Join-Path $folder 'release-notes.md'),$notes,[Text.UTF8Encoding]::new($false))
$base="https://github.com/d3a0g/playspace-releases/releases/download/v$version/"
$installerHash=(Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
$native=@{version=$version;notes=$notes;pub_date=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ');platforms=@{'windows-x86_64'=@{signature=[IO.File]::ReadAllText($installer+'.sig').Trim();url=$base+(Split-Path -Leaf $installer)}}}
$legacy=@{version=$version;downloadUrl=$base+(Split-Path -Leaf $archive);installerUrl=$base+(Split-Path -Leaf $installer);installerSha256=$installerHash;notes=$notes}
$utf8=[Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText((Join-Path $folder 'latest-v2.json'),($native|ConvertTo-Json -Depth 8),$utf8)
[IO.File]::WriteAllText((Join-Path $folder 'latest.json'),($legacy|ConvertTo-Json -Depth 5),$utf8)
Copy-Item -LiteralPath (Join-Path $workspace 'PlaySpaceNext/src-tauri/update-public-key.pub') -Destination $folder -Force
$assetNames=@((Split-Path -Leaf $installer),(Split-Path -Leaf $archive),((Split-Path -Leaf $installer)+'.sig'),'latest.json','latest-v2.json','update-public-key.pub')
$lines=foreach($name in $assetNames){((Get-FileHash -LiteralPath (Join-Path $folder $name) -Algorithm SHA256).Hash.ToLowerInvariant())+'  '+$name}
[IO.File]::WriteAllLines((Join-Path $folder 'SHA256SUMS.txt'),$lines,$utf8)
Write-Output "Prepared signed native release $version at $folder. Nothing published."
