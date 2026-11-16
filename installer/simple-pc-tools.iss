; Inno Setup script for Simple PC Tools. Built by CI and .github/workflows/release.yml:
;   iscc /DAppVersion=0.1.0 installer\simple-pc-tools.iss
; Expects target\release\simple-pc-tools.exe to exist. Output: target\installer\.
;
; The wizard follows the usual Windows installer flow:
;   Welcome > Licence > Install location > Select components (every tool ticked)
;   > Additional tasks ("Start when I sign in" ticked) > Ready > Installing
;   > Finish ("Open Simple PC Tools" ticked)
; Updates reuse the previous choices and skip the location page.
; The app's own updater runs it with /SILENT, which skips every page.

#define AppName "Simple PC Tools"
#define AppExe "simple-pc-tools.exe"
#define AppUrl "https://github.com/Dri-water/simple-pc-tools"
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
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
AppComments=Small, no-frills tools for your Windows PC.
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} setup
DefaultDirName={autopf}\{#AppName}
DisableWelcomePage=no
DisableDirPage=auto
DisableProgramGroupPage=yes
DisableReadyPage=no
DisableFinishedPage=no
AlwaysShowComponentsList=yes
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

[Messages]
WelcomeLabel2=This will install [name/ver] on your computer.%n%nSimple PC Tools is a set of small, no-frills tools. You choose which tools to install on the next pages, and you can switch them on or off later in Settings.%n%nIt's free and open source. No ads, no accounts, no tracking.
FinishedLabel=Setup has finished installing [name].%n%nIt lives in the tray by the clock. Open it from there or from the Start menu.

[Types]
Name: "full"; Description: "Full installation (all tools)"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

[Components]
Name: "core"; Description: "Simple PC Tools (required)"; Types: full custom; Flags: fixed
Name: "apps"; Description: "Apps: start programs when you sign in and keep them running"; Types: full custom
Name: "disk"; Description: "Disk usage: see which folders take up the most space"; Types: full custom

[Tasks]
Name: "autostart"; Description: "Start Simple PC Tools when I sign in (recommended)"; GroupDescription: "Startup:"
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion; Components: core
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion; Components: core

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
; Save the tool choices and set up (or remove) the sign-in task. This is the
; "permissions" step: the task runs the app as administrator without a prompt.
Filename: "{app}\{#AppExe}"; Parameters: "--setup --tools {code:ToolList} --autostart {code:AutoStart}"; Flags: runhidden waituntilterminated; StatusMsg: "Setting up Simple PC Tools..."
Filename: "{app}\{#AppExe}"; Description: "Open Simple PC Tools now"; Flags: nowait postinstall skipifsilent
; After a silent update (started from the app), bring the app back.
Filename: "{app}\{#AppExe}"; Parameters: "--background"; Flags: nowait; Check: WizardSilent

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

{ Uninstall: remove the sign-in task, and ask whether to keep the settings and
  app list (kept by default, and always kept by a silent uninstall). }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Code: Integer;
  Params: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    Params := '--uninstall';
    if not UninstallSilent() then
      if MsgBox('Also delete your Simple PC Tools settings and app list?' + #13#10#13#10 +
                'Choose No to keep them in case you install it again.',
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        Params := Params + ' --remove-data';
    Exec(ExpandConstant('{app}\{#AppExe}'), Params, '', SW_HIDE, ewWaitUntilTerminated, Code);
  end;
end;
