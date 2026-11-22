[CmdletBinding()]
param([string]$Version='2.0.0')
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$release=Join-Path $workspace "artifacts/releases/$Version"
$payload=Join-Path $release 'app'
$area=Join-Path $workspace ('artifacts/playspace2-installer-test-'+[Guid]::NewGuid().ToString('N'))
$install=Join-Path $area 'installed'
if(-not([IO.Path]::GetFullPath($install)).StartsWith($workspace+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Unsafe test path'}
$registry='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\LauncherPlaySpace.PlaySpace2.LocalSmoke_is1'
if(Test-Path -LiteralPath $registry){throw 'An isolated smoke installation already exists; inspect it before continuing.'}
$compiler=Join-Path $workspace '.tools/InnoSetup/ISCC.exe'
$setupDir=Join-Path $area 'setup'
$profile=Join-Path $area 'profile'
[void][IO.Directory]::CreateDirectory($setupDir)
[void][IO.Directory]::CreateDirectory($profile)
[IO.File]::WriteAllText((Join-Path $profile 'preferences.json'),'{"Language":"ru","OfflineMode":true,"CloseBehavior":2}')
$marker=Join-Path $profile 'must-survive.txt';[IO.File]::WriteAllText($marker,'profile fixture must survive install and uninstall')
$old=Join-Path $workspace 'artifacts/releases/1.0.6/app/LauncherPlaySpace.exe'
if(-not(Test-Path -LiteralPath $old)){throw 'Published 1.0.6 EXE fixture is required.'}
& $compiler /DSmokeTest ('/DAppVersion='+$Version) ('/DPayloadDir='+$payload) ('/DBuildOutput='+$setupDir) ('/DWebViewBootstrap='+(Join-Path $workspace 'artifacts/release-publication-2.0.0/MicrosoftEdgeWebview2Setup.exe')) (Join-Path $workspace 'installer/PlaySpace2.iss') *> (Join-Path $area 'compile-native.log')
if($LASTEXITCODE -ne 0){throw 'Native smoke installer compilation failed.'}
$setup=Join-Path $setupDir "LauncherPlaySpace-$Version-Setup.exe"
$exe=Join-Path $install 'LauncherPlaySpace.exe'
$expected=(Get-FileHash -LiteralPath (Join-Path $payload 'LauncherPlaySpace.exe')).Hash
function Install([string]$File,[switch]$Restart){
    $args=@('/VERYSILENT','/SUPPRESSMSGBOXES','/SP-','/NORESTART','/NOCLOSEAPPLICATIONS','/NORESTARTAPPLICATIONS',('/DIR="'+$install+'"'),'/GROUP="PlaySpace 2 Local Smoke"','/TASKS=""',('/PLAYSPACEPROGRESS="'+(Join-Path $area 'progress.txt')+'"'),('/LOG="'+(Join-Path $area 'install.log')+'"'))
    if($Restart){$args+='/PLAYSPACERESTART=1'}
    # Wait only for Setup itself; -Wait also waits for the restarted application.
    $process=Start-Process -FilePath $File -ArgumentList $args -WindowStyle Hidden -PassThru
    $process.WaitForExit()
    if($process.ExitCode -ne 0){throw ('Isolated installation failed: '+$process.ExitCode)}
}
function StopFixture(){
    foreach($process in Get-CimInstance Win32_Process -Filter "Name='LauncherPlaySpace.exe'"){if($process.ExecutablePath -eq $exe){Stop-Process -Id $process.ProcessId -ErrorAction SilentlyContinue}}
}
function Uninstall(){
    StopFixture
    $process=Start-Process -FilePath (Join-Path $install 'unins000.exe') -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -WindowStyle Hidden -Wait -PassThru
    if($process.ExitCode -ne 0){throw 'Smoke uninstall failed.'}
}
$savedProfile=$env:PLAYSPACE_NEXT_PROFILE
$savedWebView=$env:WEBVIEW2_USER_DATA_FOLDER
try{
    $env:PLAYSPACE_NEXT_PROFILE=$profile
    $env:WEBVIEW2_USER_DATA_FOLDER=Join-Path $area 'webview'
    Install $setup
    if((Get-FileHash -LiteralPath $exe).Hash -ne $expected){throw 'Fresh install EXE differs.'}
    if([int](Get-Content -LiteralPath (Join-Path $area 'progress.txt') -Raw) -lt 100){throw 'Legacy progress did not complete.'}
    $unmanaged=Join-Path $install 'unmanaged-user-file.txt';[IO.File]::WriteAllText($unmanaged,'must survive')
    Uninstall
    if((Test-Path -LiteralPath $exe) -or (Test-Path -LiteralPath $registry) -or -not(Test-Path -LiteralPath $unmanaged)){throw 'Fresh uninstall did not preserve unrelated files or remove owned registration.'}
    # Compile the original 1.0.6 installer against its actual EXE, with ONLY an
    # isolated AppId and absolute resource paths; production install is never touched.
    $oldPayload=Join-Path $area 'old-payload';[void][IO.Directory]::CreateDirectory($oldPayload)
    Copy-Item -LiteralPath $old -Destination (Join-Path $oldPayload 'LauncherPlaySpace.exe')
    foreach($name in @('LICENSE','THIRD_PARTY_NOTICES.md','START-HERE.txt')){Copy-Item -LiteralPath (Join-Path $payload $name) -Destination $oldPayload}
    $template=[IO.File]::ReadAllText((Join-Path $workspace 'installer/LauncherPlaySpace.iss'))
    $template=$template.Replace('AppId=LauncherPlaySpace.LocalSmoke','AppId=LauncherPlaySpace.PlaySpace2.LocalSmoke')
    $template=$template.Replace('SetupIconFile=..\GameLauncherGUI\Assets\playspace.ico','SetupIconFile='+(Join-Path $workspace 'PlaySpaceNext/src-tauri/icons/playspace.ico'))
    $template=$template.Replace('LicenseFile=..\LICENSE','LicenseFile='+(Join-Path $workspace 'LICENSE'))
    $template=$template.Replace('InfoBeforeFile=INSTALL-NOTICE.txt','InfoBeforeFile='+(Join-Path $workspace 'installer/INSTALL-NOTICE.txt'))
    $oldScript=Join-Path $area 'legacy-smoke.iss';[IO.File]::WriteAllText($oldScript,$template,[Text.UTF8Encoding]::new($false))
    $oldOutput=Join-Path $area 'old-setup';[void][IO.Directory]::CreateDirectory($oldOutput)
    & $compiler /DSmokeTest ('/DPayloadDir='+$oldPayload) ('/DBuildOutput='+$oldOutput) $oldScript *> (Join-Path $area 'compile-legacy.log')
    if($LASTEXITCODE -ne 0){throw 'Legacy smoke compilation failed.'}
    Install (Join-Path $oldOutput 'LauncherPlaySpace-Setup-preview.exe')
    if((Get-ItemProperty -LiteralPath $registry).DisplayVersion -notlike '1.0.6*'){throw 'Legacy fixture registry version incorrect.'}
    Install $setup -Restart
    if((Get-FileHash -LiteralPath $exe).Hash -ne $expected -or -not(Test-Path -LiteralPath $unmanaged)){throw 'Legacy-to-native upgrade damaged payload or unrelated file.'}
    if((Get-ItemProperty -LiteralPath $registry).DisplayVersion -ne $Version){throw 'Upgrade registration did not update.'}
    $started=$false
    for($attempt=0;$attempt -lt 60;$attempt++){
        $started=@(Get-CimInstance Win32_Process -Filter "Name='LauncherPlaySpace.exe'" | Where-Object ExecutablePath -EQ $exe).Count -gt 0
        if($started){break};Start-Sleep -Milliseconds 200
    }
    if(-not $started){throw 'Native update restart flag did not launch the installed EXE.'}
    StopFixture
    Uninstall
    if((Test-Path -LiteralPath $exe) -or (Test-Path -LiteralPath $registry) -or -not(Test-Path -LiteralPath $unmanaged) -or -not(Test-Path -LiteralPath $marker)){throw 'Final uninstall did not preserve profile/unrelated data.'}
    $nativeUpgrade=$false
    if($Version -ne '2.0.0'){
        $nativeOldPayload=Join-Path $workspace 'artifacts/releases/2.0.0/app'
        $nativeOldSetup=Join-Path $area 'native-old-setup'
        [void][IO.Directory]::CreateDirectory($nativeOldSetup)
        & $compiler /DSmokeTest /DAppVersion=2.0.0 ('/DPayloadDir='+$nativeOldPayload) ('/DBuildOutput='+$nativeOldSetup) ('/DWebViewBootstrap='+(Join-Path $workspace 'artifacts/release-publication-2.0.0/MicrosoftEdgeWebview2Setup.exe')) (Join-Path $workspace 'installer/PlaySpace2.iss') *> (Join-Path $area 'compile-native-old.log')
        if($LASTEXITCODE -ne 0){throw 'Previous native smoke installer compilation failed.'}
        Install (Join-Path $nativeOldSetup 'LauncherPlaySpace-2.0.0-Setup.exe')
        if((Get-ItemProperty -LiteralPath $registry).DisplayVersion -ne '2.0.0'){throw 'Previous version registration mismatch.'}
        Install $setup
        if((Get-ItemProperty -LiteralPath $registry).DisplayVersion -ne $Version -or (Get-FileHash -LiteralPath $exe).Hash -ne $expected -or -not(Test-Path -LiteralPath $marker)){throw 'Native upgrade verification failed.'}
        Uninstall
        $nativeUpgrade=$true
    }
    $report=@{passed=$true;freshInstall=$true;uninstall=$true;legacyVersion='1.0.6';nativeVersion=$Version;legacyInPlaceUpgrade=$true;nativeInPlaceUpgrade=$nativeUpgrade;nativeRestartFlag=$true;legacyProgress=$true;profilePreserved=$true;unrelatedFilePreserved=$true;webviewRuntime='already present; missing-runtime bootstrap not executed';realInstallationUntouched=$true}
    [IO.File]::WriteAllText((Join-Path $area 'report.json'),($report|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    Copy-Item -LiteralPath (Join-Path $area 'report.json') -Destination (Join-Path $release 'installer-verification.json') -Force
    "PASS: isolated fresh install/uninstall, registered 1.0.6 -> $Version upgrade, native restart and data preservation."
}finally{
    StopFixture
    if((Test-Path -LiteralPath $registry) -and (Test-Path -LiteralPath (Join-Path $install 'unins000.exe'))){Uninstall}
    [Environment]::SetEnvironmentVariable('PLAYSPACE_NEXT_PROFILE',$savedProfile,'Process')
    [Environment]::SetEnvironmentVariable('WEBVIEW2_USER_DATA_FOLDER',$savedWebView,'Process')
}
