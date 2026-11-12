#ifndef FixtureRoot
  #error FixtureRoot must point to the disposable runner test directory.
#endif
#define MigrationFixture

[Setup]
AppId=PebrelExplorerReview456
AppName=Pebrel Explorer Menu Review
AppVersion=0.0.0
DefaultDirName={#FixtureRoot}
CreateAppDir=no
Uninstallable=no
PrivilegesRequired=lowest
OutputBaseFilename=installer-explorer-review-456
Compression=none

[CustomMessages]
OpenInPebrel=Open in Pebrel
OpenInPebrelWsl=Open in Pebrel (WSL)
WslMenuConflict=Preserved an edited or foreign submenu: %1.
WslMenuRegistrationFailed=Unable to register the WSL context submenu.
ExplorerMenuTitle=Explorer context menu
ExplorerMenuDescription=Choose Explorer entries.
ExplorerMenuHelp=Review fixture.
ExplorerMenuEnabled=Enable Explorer integration
ExplorerMenuConflict=Preserved an edited or foreign context menu: %1.
ExplorerMenuFailed=Unable to update Explorer entries or save choices.

[Code]
#include "..\installer-migration.iss"

var
  RegistryRoot, Report: string;
  Failures: Integer;

procedure RecordResult(Name: string; Passed: Boolean; Detail: string);
begin
  if Passed then
    Report := Report + 'PASS: '
  else begin
    Report := Report + 'FAIL: ';
    Failures := Failures + 1;
  end;
  Report := Report + Name + ' | ' + Detail + #13#10;
end;

procedure SelectEntries(Wsl: Boolean);
begin
  ExplorerMenuPage.Values[0] := True;
  ExplorerMenuPage.Values[1] := True;
  ExplorerMenuPage.Values[2] := Wsl;
  ExplorerMenuPage.Values[3] := Wsl;
end;

procedure CheckRelocation(Root, Argument, LabelText: string; Wsl: Boolean);
var
  OldExe, NewExe, Existing, Detail: string;
  Updated: Boolean;
begin
  PreviousInstallDir := ExpandConstant('{#FixtureRoot}') + '\Previous';
  OldExe := PreviousInstallDir + '\pebrel.exe';
  NewExe := ExpandConstant('{#FixtureRoot}') + '\Moved\pebrel.exe';
  SelectEntries(Wsl);
  UpdateExplorerMenusAt(Root, OldExe, Argument);
  Detail := 'existing registration now points to the new directory';
  try
    UpdateExplorerMenusAt(Root, NewExe, Argument);
    Updated := RegQueryStringValue(HKCU, Root + '\Pebrel\command', '', Existing) and
      (Existing = DefaultExplorerMenuCommand(NewExe, Argument));
    if Wsl then
      Updated := Updated and IsOwnedWslMenu(Root + '\PebrelWslMenu', NewExe);
  except
    Updated := False;
    Detail := GetExceptionMessage;
  end;
  if not Updated and RegQueryStringValue(HKCU, Root + '\Pebrel\command', '', Existing) and
    (Existing = DefaultExplorerMenuCommand(OldExe, Argument)) then
    Detail := Detail + ' (previous ordinary command remains)';
  RecordResult(LabelText, Updated, Detail);
end;

procedure CheckConflict(Root, Argument, LabelText: string);
var
  Exe, Existing: string;
  Children: TArrayOfString;
  Rejected, Preserved: Boolean;
  Remaining: Integer;
begin
  Exe := ExpandConstant('{#FixtureRoot}') + '\Current\pebrel.exe';
  SelectEntries(True);
  UpdateExplorerMenusAt(Root, Exe, Argument);
  if not RegWriteStringValue(HKCU, Root + '\Pebrel\command', '', 'fixture foreign command') then
    RaiseException('unable to seed the isolated foreign ordinary verb');
  ExplorerMenuPage.Values[3] := False;
  Rejected := False;
  try
    UpdateExplorerMenusAt(Root, Exe, Argument);
  except
    Rejected := True;
  end;
  Remaining := -1;
  if RegGetSubkeyNames(HKCU, Root + '\PebrelWslMenu\shell', Children) then
    Remaining := GetArrayLength(Children);
  Preserved := RegQueryStringValue(HKCU, Root + '\Pebrel\command', '', Existing) and
    (Existing = 'fixture foreign command');
  RecordResult(LabelText, Rejected and Preserved and (Remaining = 2),
    'WSL entries after conflict: ' + IntToStr(Remaining) + ' (expected 2)');
end;

procedure InitializeWizard;
var
  Distros: TArrayOfString;
begin
  RegistryRoot := 'Software\PebrelTestFixtures\' + ExtractFileName(ExpandConstant('{#FixtureRoot}'));
  SetArrayLength(Distros, 2);
  Distros[0] := 'Fixture Linux A';
  Distros[1] := 'Fixture Linux B';
  try
    try
      CreateExplorerMenuPage(RegistryRoot + '\Choices', Distros);
      CheckRelocation(RegistryRoot + '\selected-local', '%1', 'selected folder, moved ordinary entry', False);
      CheckRelocation(RegistryRoot + '\background-local', '%V', 'background, moved ordinary entry', False);
      CheckRelocation(RegistryRoot + '\selected-wsl', '%1', 'selected folder, moved WSL entry', True);
      CheckRelocation(RegistryRoot + '\background-wsl', '%V', 'background, moved WSL entry', True);
      CheckConflict(RegistryRoot + '\selected-conflict', '%1', 'selected folder, preserve WSL entries on conflict');
      CheckConflict(RegistryRoot + '\background-conflict', '%V', 'background, preserve WSL entries on conflict');
    except
      RecordResult('fixture setup', False, GetExceptionMessage);
    end;
    if not SaveStringToFile(ExpandConstant('{#FixtureRoot}') + '\independent-result.txt', Report, False) then
      RaiseException('unable to write independent-result.txt');
  finally
    RegDeleteKeyIncludingSubkeys(HKCU, RegistryRoot);
  end;
end;

function GetCustomSetupExitCode: Integer;
begin
  Result := 0;
  if Failures > 0 then
    Result := 1;
end;
