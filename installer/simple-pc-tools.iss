; Inno Setup script for Simple PC Tools. Built by .github/workflows/release.yml:
;   iscc /DAppVersion=0.1.0 installer\simple-pc-tools.iss
; Expects target\release\simple-pc-tools.exe to exist. Output: target\installer\.

#define AppName "Simple PC Tools"
#define AppExe "simple-pc-tools.exe"
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Never change AppId: it is how Windows recognises an update of the same app.
AppId={{8C4A6F2E-5B1D-4E7A-9C3F-2D6B8E1A4F70}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Simple PC Tools contributors
AppPublisherURL=https://github.com/Dri-water/simple-pc-tools
AppSupportURL=https://github.com/Dri-water/simple-pc-tools/issues
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
OutputDir=..\target\installer
OutputBaseFilename=simple-pc-tools-setup-{#AppVersion}
; We quit the running copy ourselves (PrepareToInstall), so no Restart Manager prompt.
CloseApplications=no

[Types]
Name: "full"; Description: "All tools (recommended)"
Name: "custom"; Description: "Choose the tools"; Flags: iscustom

[Components]
Name: "apps"; Description: "Apps: start programs when you sign in and keep them running"; Types: full custom
Name: "disk"; Description: "Disk usage: see which folders take up the most space"; Types: full custom

[Tasks]
Name: "autostart"; Description: "Start Simple PC Tools when I sign in (recommended)"
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
; Save the tool choices and set up (or remove) the sign-in task. This is the
; "permissions" step: the task runs the app as administrator without a prompt.
Filename: "{app}\{#AppExe}"; Parameters: "--setup --tools {code:ToolList} --autostart {code:AutoStart}"; Flags: runhidden waituntilterminated; StatusMsg: "Setting up Simple PC Tools..."
Filename: "{app}\{#AppExe}"; Description: "Open Simple PC Tools"; Flags: nowait postinstall skipifsilent
; After a silent update (started from the app), bring the app back.
Filename: "{app}\{#AppExe}"; Parameters: "--background"; Flags: nowait; Check: WizardSilent

[UninstallRun]
Filename: "{app}\{#AppExe}"; Parameters: "--uninstall"; Flags: runhidden waituntilterminated; RunOnceId: "RemoveSignInTask"

[Code]
procedure QuitRunningCopy();
var
  Code: Integer;
begin
  if FileExists(ExpandConstant('{app}\{#AppExe}')) then
    Exec(ExpandConstant('{app}\{#AppExe}'), '--quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(1500);
  { If it didn't answer, end it. The apps it started keep running either way. }
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  QuitRunningCopy();
  Result := '';
end;

function ToolList(Param: String): String;
begin
  Result := '';
  if WizardIsComponentSelected('apps') then Result := 'apps';
  if WizardIsComponentSelected('disk') then
  begin
    if Result <> '' then Result := Result + ',';
    Result := Result + 'disk';
  end;
  if Result = '' then Result := 'none';
end;

function AutoStart(Param: String): String;
begin
  if WizardIsTaskSelected('autostart') then Result := 'on' else Result := 'off';
end;
