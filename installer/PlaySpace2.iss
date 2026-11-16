; Production and portable payloads must contain the native binary as LauncherPlaySpace.exe.
; The build coordinator verifies Microsoft's Authenticode signature on WebViewBootstrap
; before invoking ISCC. This file never downloads or executes PowerShell scripts.
#ifndef PayloadDir
  #error PayloadDir must point to the native production payload
#endif
#ifndef BuildOutput
  #error BuildOutput must point to the installer output directory
#endif
#ifndef WebViewBootstrap
  #error WebViewBootstrap must point to the verified Microsoft Evergreen bootstrapper
#endif
#define AppExe "LauncherPlaySpace.exe"
#define AppVersion "2.0.0"
#define AppFileVersion "2.0.0.0"
#if GetVersionNumbersString(AddBackslash(PayloadDir) + AppExe) != AppFileVersion
  #error Native LauncherPlaySpace.exe must have file version 2.0.0.0
#endif

[Setup]
#ifdef SmokeTest
AppId=LauncherPlaySpace.PlaySpace2.LocalSmoke
DefaultDirName={localappdata}\Programs\PlaySpace2-LocalSmoke
DefaultGroupName=PlaySpace 2 Local Smoke
#else
AppId={{9DB01324-66DD-4DA3-B7B0-042139280B20}
DefaultDirName={localappdata}\Programs\LauncherPlaySpace
DefaultGroupName=LauncherPlaySpace
AppMutex=Local\LauncherPlaySpace.Launcher
#endif
AppName=PlaySpace
AppVersion={#AppVersion}
AppVerName=PlaySpace {#AppVersion}
VersionInfoVersion={#AppFileVersion}
AppPublisher=d3a0g
AppPublisherURL=https://github.com/d3a0g/LauncherPlaySpace
AppSupportURL=https://t.me/PlaySpaceSupport_bot
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
UsePreviousAppDir=yes
UsePreviousTasks=yes
DisableProgramGroupPage=yes
WizardStyle=modern
SetupIconFile=..\PlaySpaceNext\src-tauri\icons\playspace.ico
UninstallDisplayIcon={app}\{#AppExe}
CloseApplications=yes
CloseApplicationsFilter=LauncherPlaySpace.exe
RestartApplications=no
Compression=lzma2
SolidCompression=yes
OutputDir={#BuildOutput}
#ifdef InstallerName
OutputBaseFilename={#InstallerName}
#else
OutputBaseFilename=LauncherPlaySpace-2.0.0-Setup
#endif
LicenseFile=..\LICENSE
InfoBeforeFile=PLAYSPACE2-NOTICE.txt
Uninstallable=yes
UninstallLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut / Создать ярлык на рабочем столе"; Flags: unchecked

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#WebViewBootstrap}"; DestDir: "{tmp}"; DestName: "PlaySpace-WebView2Setup.exe"; Flags: dontcopy

[Icons]
Name: "{group}\LauncherPlaySpace"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"
Name: "{autodesktop}\LauncherPlaySpace"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Tasks: desktopicon

[InstallDelete]
; Remove only the obsolete shortcut owned by the legacy installer.
Type: files; Name: "{group}\LauncherPlaySpace - Safe Mode.lnk"

[Run]
; The legacy 1.0.6 worker restarts the EXE itself, so its unflagged run must not launch twice.
Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Flags: nowait; Check: UpdateRestartRequested

[Code]
const
  WebViewClient = 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';

function UpdateRestartRequested: Boolean;
begin
  Result := ExpandConstant('{param:PLAYSPACERESTART|0}') = '1';
end;

function RuntimeAt(Root: Integer): Boolean;
var
  Version: String;
  PackedVersion: Int64;
begin
  Result := False;
  if RegQueryStringValue(Root, WebViewClient, 'pv', Version) then
    if StrToVersion(Version, PackedVersion) then
      Result := PackedVersion > 0;
end;

function WebViewInstalled: Boolean;
begin
  Result := RuntimeAt(HKCU32) or RuntimeAt(HKCU64) or
            RuntimeAt(HKLM32) or RuntimeAt(HKLM64);
end;

procedure CurInstallProgressChanged(CurProgress, MaxProgress: Integer);
var
  ProgressFile: String;
begin
  ProgressFile := ExpandConstant('{param:PLAYSPACEPROGRESS|}');
  if (ProgressFile <> '') and (MaxProgress > 0) then
    SaveStringToFile(ProgressFile, IntToStr(Round(100.0 * CurProgress / MaxProgress)), False);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  InstalledVersion, NewVersion: Int64;
  ExitCode: Integer;
begin
  Result := '';
  if GetPackedVersion(ExpandConstant('{app}\{#AppExe}'), InstalledVersion) and
     StrToVersion('{#AppVersion}', NewVersion) then
    if ComparePackedVersion(InstalledVersion, NewVersion) > 0 then
    begin
      Result := 'A newer version is installed. Downgrade is not supported. / Уже установлена более новая версия. Понижение версии запрещено.';
      Exit;
    end;
  if not WebViewInstalled then
  begin
    ExtractTemporaryFile('PlaySpace-WebView2Setup.exe');
    if not Exec(ExpandConstant('{tmp}\PlaySpace-WebView2Setup.exe'), '/silent /install',
                '', SW_HIDE, ewWaitUntilTerminated, ExitCode) then
    begin
      Result := 'Could not start Microsoft WebView2 installation. / Не удалось запустить установку Microsoft WebView2.';
      Exit;
    end;
    if not WebViewInstalled then
      Result := 'Microsoft WebView2 Runtime is required but was not installed (code ' +
        IntToStr(ExitCode) + '). Check your Internet connection and install Microsoft WebView2 Runtime, then retry. / Требуется Microsoft WebView2 Runtime. Проверьте подключение к Интернету, установите Runtime и повторите установку.';
  end;
end;

// No UninstallDelete: profiles, games and unrelated files are preserved.
