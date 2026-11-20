[CmdletBinding()]
param(
    [switch]$Initialize,
    [switch]$Publish,
    [long]$Revision=0,
    [ValidateRange(1,90)][int]$ValidDays=90,
    [string]$MinimumClientVersion='2.0.0',
    [string]$MaximumClientVersion='',
    [string]$SourceDirectory='PlaySpaceNext/remote',
    [string]$OutputDirectory='artifacts/remote-config/stable',
    [string]$VerifierExecutable='',
    [string]$GitHubCliPath=''
)
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$keyPath=Join-Path $workspace '.secrets/remote-config-authority.dpapi'
$publicPath=Join-Path $workspace 'PlaySpaceNext/src-tauri/remote-public-key.xml'
$source=if([IO.Path]::IsPathRooted($SourceDirectory)){$SourceDirectory}else{Join-Path $workspace $SourceDirectory}
$output=if([IO.Path]::IsPathRooted($OutputDirectory)){$OutputDirectory}else{Join-Path $workspace $OutputDirectory}
$utf8=[Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName System.Security
$rsa=New-Object System.Security.Cryptography.RSACryptoServiceProvider(3072)
$rsa.PersistKeyInCsp=$false
try {
    if($Initialize){
        if((Test-Path -LiteralPath $keyPath) -or (Test-Path -LiteralPath $publicPath)){throw 'Remote signing identity already exists. Do not rotate it accidentally.'}
        [void][IO.Directory]::CreateDirectory((Split-Path -Parent $keyPath))
        $secret=$utf8.GetBytes($rsa.ToXmlString($true))
        try{[IO.File]::WriteAllBytes($keyPath,[Security.Cryptography.ProtectedData]::Protect($secret,$null,[Security.Cryptography.DataProtectionScope]::CurrentUser))}finally{[Array]::Clear($secret,0,$secret.Length)}
        [IO.File]::WriteAllText($publicPath,$rsa.ToXmlString($false),$utf8)
        Write-Output 'Created separate remote configuration authority. Private key protected with current-user Windows DPAPI.'
    }
    if(-not(Test-Path -LiteralPath $keyPath)){throw 'Initialize the remote signing identity first with -Initialize.'}
    $secret=[Security.Cryptography.ProtectedData]::Unprotect([IO.File]::ReadAllBytes($keyPath),$null,[Security.Cryptography.DataProtectionScope]::CurrentUser)
    try{$rsa.FromXmlString($utf8.GetString($secret))}finally{[Array]::Clear($secret,0,$secret.Length)}
    if($rsa.ToXmlString($false) -ne [IO.File]::ReadAllText($publicPath).Trim()){throw 'Signing identity does not match the embedded public key.'}
    if($MinimumClientVersion -notmatch '^\d+\.\d+\.\d+$' -or ($MaximumClientVersion -and $MaximumClientVersion -notmatch '^\d+\.\d+\.\d+$')){throw 'Use three-part client versions.'}
    function Read-Json([string]$relative){Get-Content -LiteralPath (Join-Path $source $relative) -Raw -Encoding UTF8|ConvertFrom-Json}
    $previousRevision=0L
    $manifestPath=Join-Path $output 'manifest.json'
    if(Test-Path -LiteralPath $manifestPath){
        $previous=Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8|ConvertFrom-Json
        $previousPayload=$utf8.GetString([Convert]::FromBase64String($previous.payload))|ConvertFrom-Json
        $previousRevision=[long]$previousPayload.revision
    }
    $ghCommand=Get-Command gh -ErrorAction SilentlyContinue
    $gh=if($GitHubCliPath){$GitHubCliPath}elseif($ghCommand){$ghCommand.Source}else{Join-Path $workspace '.tools/github-cli-2.103.3/bin/gh.exe'}
    $remoteEntry=$null
    if($Publish){
        $inventory=& $gh api 'repos/d3a0g/playspace-releases/git/trees/main?recursive=1'
        if($LASTEXITCODE -ne 0){throw 'Cannot inspect the remote branch.'}
        $tree=$inventory|ConvertFrom-Json
        if($tree.truncated){throw 'Remote inventory is truncated.'}
        $remoteEntry=$tree.tree|Where-Object {$_.path -eq 'remote/stable/manifest.json'}|Select-Object -First 1
        if($remoteEntry){
            if($remoteEntry.type -ne 'blob' -or $remoteEntry.size -gt 4MB){throw 'Published envelope exceeds its size limit or is not a file.'}
            $blob=& $gh api ('repos/d3a0g/playspace-releases/git/blobs/'+$remoteEntry.sha)
            if($LASTEXITCODE -ne 0){throw 'Cannot read the published configuration.'}
            $remoteEnvelope=$utf8.GetString([Convert]::FromBase64String(($blob|ConvertFrom-Json).content))|ConvertFrom-Json
            $remotePayload=$utf8.GetString([Convert]::FromBase64String($remoteEnvelope.payload))|ConvertFrom-Json
            $remoteBytes=[Convert]::FromBase64String($remoteEnvelope.payload)
            $domain=$utf8.GetBytes("PlaySpaceRemoteConfig/v1`n")
            $remoteSigned=New-Object byte[] ($domain.Length+$remoteBytes.Length)
            [Array]::Copy($domain,0,$remoteSigned,0,$domain.Length);[Array]::Copy($remoteBytes,0,$remoteSigned,$domain.Length,$remoteBytes.Length)
            if(-not $rsa.VerifyData($remoteSigned,'SHA256',[Convert]::FromBase64String($remoteEnvelope.signature))){throw 'Published configuration has an invalid signature. Inspect it before publishing.'}
            $previousRevision=[Math]::Max($previousRevision,[long]$remotePayload.revision)
        }
    }
    if($Revision -eq 0){$Revision=$previousRevision+1}
    if($Revision -le $previousRevision){throw 'A published revision must increase. To roll back content, publish the old content with a new revision.'}
    $issued=[DateTimeOffset]::UtcNow
    $payload=[ordered]@{
        schemaVersion=1;revision=$Revision;minClientVersion=$MinimumClientVersion;maxClientVersion=$(if($MaximumClientVersion){$MaximumClientVersion}else{$null})
        issuedAt=$issued.ToString('o');expiresAt=$issued.AddDays($ValidDays).ToString('o')
        config=(Read-Json 'config.json');announcements=[object[]](Read-Json 'announcements.json')
        translations=@{ru=(Read-Json 'translations/ru.json');en=(Read-Json 'translations/en.json')}
        catalogs=@{recommendations=[object[]](Read-Json 'catalogs/recommendations.json');modAdapters=(Read-Json 'catalogs/mod-adapters.json')}
    }
    $bytes=$utf8.GetBytes(($payload|ConvertTo-Json -Depth 60 -Compress))
    if($bytes.Length -gt 2MB){throw 'Remote payload exceeds 2 MiB.'}
    $prefix=$utf8.GetBytes("PlaySpaceRemoteConfig/v1`n")
    $signed=New-Object byte[] ($prefix.Length+$bytes.Length)
    [Array]::Copy($prefix,0,$signed,0,$prefix.Length);[Array]::Copy($bytes,0,$signed,$prefix.Length,$bytes.Length)
    $signature=$rsa.SignData($signed,'SHA256')
    if(-not $rsa.VerifyData($signed,'SHA256',$signature)){throw 'Signature self-check failed.'}
    [void][IO.Directory]::CreateDirectory($output)
    $envelope=@{payload=[Convert]::ToBase64String($bytes);signature=[Convert]::ToBase64String($signature)}|ConvertTo-Json -Compress
    [IO.File]::WriteAllText($manifestPath,$envelope,$utf8)
    [IO.File]::WriteAllText((Join-Path $output 'payload-preview.json'),($payload|ConvertTo-Json -Depth 60),$utf8)
    Write-Output "Signed remote configuration revision $Revision, expires $($issued.AddDays($ValidDays).ToString('u'))."
    if($Publish){
        if(-not $VerifierExecutable -or -not(Test-Path -LiteralPath $VerifierExecutable)){throw 'Supply -VerifierExecutable with the newly built launcher. Publication requires native signature and schema validation.'}
        $validationPath=Join-Path $output ('validation-'+[Guid]::NewGuid().ToString('N'))
        $startInfo=New-Object Diagnostics.ProcessStartInfo
        $startInfo.FileName=[IO.Path]::GetFullPath($VerifierExecutable)
        $startInfo.Arguments='--verify-remote-config "'+$manifestPath+'"'
        $startInfo.UseShellExecute=$false;$startInfo.CreateNoWindow=$true;$startInfo.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden
        $startInfo.RedirectStandardOutput=$true;$startInfo.RedirectStandardError=$true
        $validator=[Diagnostics.Process]::Start($startInfo)
        try{
            if(-not $validator.WaitForExit(15000)){$validator.Kill();throw 'Native configuration validation timed out. Nothing published.'}
            $stdout=$validator.StandardOutput.ReadToEnd();$stderr=$validator.StandardError.ReadToEnd()
            [IO.File]::WriteAllText(($validationPath+'.json'),$stdout,$utf8);[IO.File]::WriteAllText(($validationPath+'.txt'),$stderr,$utf8)
            $validation=$stdout|ConvertFrom-Json
            if($validator.ExitCode -ne 0 -or -not $validation.valid){throw 'Native configuration validation failed. Nothing published.'}
        }finally{$validator.Dispose()}
        # Publish exactly one file through the contents API with a blob-SHA lease.
        $update=@{message="Publish signed remote configuration revision $Revision";branch='main';content=[Convert]::ToBase64String([IO.File]::ReadAllBytes($manifestPath))}
        if($remoteEntry){$update.sha=$remoteEntry.sha}
        $requestPath=Join-Path $output 'publication-request.json'
        [IO.File]::WriteAllText($requestPath,($update|ConvertTo-Json -Compress),$utf8)
        & $gh api --method PUT 'repos/d3a0g/playspace-releases/contents/remote/stable/manifest.json' --input $requestPath --jq .commit.sha
        if($LASTEXITCODE -ne 0){throw 'Configuration publication rejected; inspect the remote file and publish a new revision.'}
        Write-Output 'Published https://raw.githubusercontent.com/d3a0g/playspace-releases/main/remote/stable/manifest.json'
    }
} finally {$rsa.Dispose()}
