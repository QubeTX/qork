; qork — Global Edition installer (perMachine, requires admin).
;
; Built by .github/workflows/windows-installers.yml on tag push. Companion to
; inno/corporate.iss (perUser sibling).
;
; Builds with Inno Setup 6 (`iscc` from JRSoftware) on a Windows runner. CI
; passes the version via `iscc /DMyAppVersion=1.0.1` so the same script
; rebuilds at every release without editing.
;
; The MSI sibling lives at wix/main.wxs. Both installers target the SAME
; install path (C:\Program Files\qork\bin\qork.exe) and write the SAME
; registry marker (HKCU\Software\Qork) — only the InstallSource value
; differs (msi-global vs exe-global). README documents "pick one format per
; edition" since coexistence creates duplicate Add/Remove Programs entries.
;
; qork installs ONLY the single binary on PATH plus the install-source
; registry marker. No shell alias, no auto-run, no migrate-cleanup. (The
; tr300 project this was adapted from adds those; they were deliberately
; stripped here.)
;
; If install path changes here, update src/update.rs::classify_install_path()
; in lockstep — that logic matches on the install path to choose which
; installer to fetch during `qork update`.

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0-dev"
#endif

#define MyAppName "qork"
#define MyAppPublisher "Emmett S"
#define MyAppURL "https://qork.me/install"
#define MyAppExeName "qork.exe"

[Setup]
; AppId is the immutable identity of the Global EXE installer.
; Different from the MSI Global's UpgradeCode (Windows treats MSI products and
; Inno Setup products as separate even when they target the same install path)
; and different from the Corporate EXE's AppId.
AppId={{67481507-3640-45A6-ACB8-7EEB87C48E7E}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
; perMachine install: %ProgramFiles%\qork — same path as MSI Global by design.
DefaultDirName={commonpf}\{#MyAppName}
DefaultGroupName={#MyAppName}
; CLI tool — no start menu group, no desktop shortcut.
DisableProgramGroupPage=yes
DisableDirPage=auto
; Require admin (perMachine scope). Triggers UAC prompt at install start.
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
OutputBaseFilename=qork-x86_64-pc-windows-msvc-setup
OutputDir=Output
Compression=lzma
SolidCompression=yes
WizardStyle=modern
; Tell Windows we touched env vars so File Explorer broadcasts WM_SETTINGCHANGE.
; New cmd / PowerShell sessions then pick up the PATH addition without reboot.
ChangesEnvironment=yes
; ARP display name. Matches the MSI Global's Product Name so users see the
; same label regardless of which installer they used.
UninstallDisplayName={#MyAppName}
; Embed the LICENSE file so the installer wizard shows it.
LicenseFile=..\LICENSE
SetupLogging=yes
; Close any running qork before we replace files so the in-place upgrade isn't
; blocked. CloseApplications uses Windows' Restart Manager; AppMutex lets Setup
; detect a running instance. (qork is a short-lived CLI tool, so this is almost
; always a no-op.)
AppMutex=QORK_Running
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
; Bundles qork.exe from target/release/. The CI workflow runs cargo build
; --release before invoking iscc so this path is populated.
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}\bin"; Flags: ignoreversion

[Registry]
; Install-source marker. qork update reads HKCU\Software\Qork\InstallSource
; and picks the matching installer to download for in-place upgrades. Value
; must match the `exe-global` arm in src/update.rs.
Root: HKCU; Subkey: "Software\Qork"; ValueType: string; ValueName: "InstallSource"; ValueData: "exe-global"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Qork"; Flags: uninsdeletekeyifempty

[Code]
{
  PATH management — system PATH (HKLM) for the Global perMachine edition.
  Inno Setup's [Registry] section can't safely append-without-duplicates +
  reliably remove-on-uninstall, so we do it explicitly in [Code].
  The canonical pattern, adapted from the Inno Setup community knowledge base.
}
const
  EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

procedure EnvAddPath(Path: string);
var
  Paths: string;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths) then
    Paths := '';

  // Skip if already in PATH (case-insensitive substring match with
  // ;-padding so we don't match a prefix of a different directory).
  if Pos(';' + Uppercase(Path) + ';', ';' + Uppercase(Paths) + ';') > 0 then exit;

  if Length(Paths) > 0 then
    Paths := Paths + ';' + Path
  else
    Paths := Path;

  RegWriteExpandStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths);
end;

procedure EnvRemovePath(Path: string);
var
  Paths: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths) then
    exit;

  P := Pos(';' + Uppercase(Path) + ';', ';' + Uppercase(Paths) + ';');
  if P = 0 then exit;

  if P = 1 then
    // First-entry case. Most likely on fresh corporate workstations where
    // SYSTEM Path is empty before install, so qork's bin lands at index 1.
    //   Paths = "X;Y"  -> "Y"    (eats "X;")
    //   Paths = "X;"   -> ""     (eats "X;")
    //   Paths = "X"    -> ""     (eats "X", count clamps to remaining)
    Delete(Paths, 1, Length(Path) + 1)
  else
    // Middle/end entry: consume the leading `;` plus the path.
    //   Paths = "A;X;B" -> "A;B" (eats ";X")
    //   Paths = "A;X"   -> "A"   (eats ";X")
    Delete(Paths, P - 1, Length(Path) + 1);

  RegWriteExpandStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    EnvAddPath(ExpandConstant('{app}') + '\bin');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    EnvRemovePath(ExpandConstant('{app}') + '\bin');
end;
