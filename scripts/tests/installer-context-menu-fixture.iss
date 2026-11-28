#ifndef FixtureRoot
  #error FixtureRoot is required
#endif
[Setup]
AppId={#FixtureIdentity}
AppName=Pebrel Context Menu Acceptance
AppVersion=0.0.0
DefaultDirName={#FixtureRoot}\installed
PrivilegesRequired=lowest
DisableProgramGroupPage=yes
CreateAppDir=yes
OutputBaseFilename=installer-context-menu-fixture
Compression=none
UninstallDisplayName=Pebrel Context Menu Acceptance

[CustomMessages]
ModernContextMenuFailed=Context menu registration failed.
ModernContextMenuRemoveFailed=Context menu unregistration failed.

[Files]
Source: "{#FixtureRoot}\payload\*"; DestDir: "{app}\context-menu"; Flags: ignoreversion recursesubdirs
Source: "{#FixtureRoot}\payload\pebrel-context-menu-setup.exe"; DestDir: "{app}"; DestName: "pebrel.exe"; Flags: ignoreversion

[Code]
#include "..\installer-context-menu.iss"
function InitializeUninstall: Boolean;
begin
  Result := ExpandConstant('{param:CancelBeforeConfirmation|0}') <> '1';
end;
procedure CurStepChanged(Step: TSetupStep);
begin
  if Step = ssPostInstall then
    RegisterModernContextMenu;
end;
function GetCustomSetupExitCode: Integer;
begin
  Result := 0;
  if ModernContextMenuFailed then
    Result := 1;
end;
