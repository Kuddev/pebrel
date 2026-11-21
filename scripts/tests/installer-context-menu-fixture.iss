#ifndef FixtureRoot
  #error FixtureRoot must point at an isolated test directory.
#endif
#define MigrationFixture

[Setup]
AppId=PebrelExplorerMenuFixture
AppName=Pebrel Explorer Menu Fixture
AppVersion=0.0.0
DefaultDirName={#FixtureRoot}
CreateAppDir=no
Uninstallable=no
PrivilegesRequired=lowest
OutputBaseFilename=installer-context-menu-fixture
Compression=none
DisableWelcomePage=yes
DisableDirPage=yes
DisableReadyPage=yes
DisableFinishedPage=yes

[CustomMessages]
OpenInPebrel=Open in Pebrel
OpenInPebrelWsl=Open in Pebrel (WSL)
WslMenuConflict=Preserved an edited or foreign submenu: %1.
WslMenuRegistrationFailed=Unable to register the WSL context submenu.
ExplorerMenuTitle=Explorer context menu
ExplorerMenuDescription=Choose which Pebrel entries appear in Explorer.
ExplorerMenuHelp=Run this installer again to change these choices. Upgrades keep your selection. New WSL distributions start unchecked after you save a selection.
ExplorerMenuEnabled=Enable Explorer context menu integration
ExplorerMenuConflict=Preserved an edited or foreign context menu: %1.
ExplorerMenuFailed=Unable to update Explorer entries or save choices.

[Code]
#include "..\installer-migration.iss"

var
  RegistryRoot, Executable: string;
  Checks: Integer;

procedure Check(Condition: Boolean; Name: string);
begin
  if not Condition then
    RaiseException(Name);
  Checks := Checks + 1;
end;

procedure CheckEditedWslSubtrees;
var
  Root, Argument, Verb, Custom, Value, Report: string;
  RootIndex, Depth, Mode, Failures: Integer;
  Failed, Preserved: Boolean;
begin
  Report := '';
  Failures := 0;
  for RootIndex := 0 to 1 do
    for Depth := 0 to 1 do
      for Mode := 0 to 3 do begin
        Root := RegistryRoot + '\structure' + IntToStr(RootIndex);
        if RootIndex = 0 then Argument := '%1' else Argument := '%V';
        RegDeleteKeyIncludingSubkeys(HKCU, Root);
        ExplorerMenuPage.Values[0] := True;
        ExplorerMenuPage.Values[2] := True;
        ExplorerMenuPage.Values[3] := True;
        if Mode = 2 then begin
          Verb := Root + '\PebrelWsl0';
          Check(RegWriteStringValue(HKCU, Verb + '\command', '',
            '"' + Executable + '" --gpui --shell "wsl:Ubuntu Test" --working-directory "' + Argument + '"'),
            'seed owned legacy flat verb');
        end else begin
          UpdateExplorerMenusAt(Root, Executable, Argument);
          Verb := Root + '\PebrelWslMenu\shell\PebrelWsl0';
        end;
        Custom := Verb;
        if Depth = 1 then Custom := Custom + '\command';
        Custom := Custom + '\Custom';
        Check(RegWriteStringValue(HKCU, Custom, '', 'user data'), 'seed unknown WSL descendant');
        Failed := False;
        if Mode = 2 then
          RemoveOwnedWslContextMenusAt(Root, Executable)
        else begin
          if Mode = 0 then begin
            ExplorerMenuPage.Values[2] := False;
            ExplorerMenuPage.Values[3] := False;
          end else if Mode = 1 then
            ExplorerMenuPage.Values[0] := False
          else
            ExplorerMenuPage.Values[3] := False;
          try
            UpdateExplorerMenusAt(Root, Executable, Argument);
          except
            Failed := True;
          end;
        end;
        Preserved := RegQueryStringValue(HKCU, Custom, '', Value) and (Value = 'user data');
        if Mode = 3 then Preserved := Preserved and Failed
        else Preserved := Preserved and not Failed;
        if Preserved then Report := Report + 'PASS: ' else begin
          Report := Report + 'FAIL: ';
          Failures := Failures + 1;
        end;
        Report := Report + 'root=' + IntToStr(RootIndex) + ' depth=' + IntToStr(Depth) +
          ' mode=' + IntToStr(Mode) + #13#10;
      end;
  SaveStringToFile(ExpandConstant('{#FixtureRoot}') + '\wsl-structure-result.txt', Report, False);
  ExplorerMenuPage.Values[0] := True;
  ExplorerMenuPage.Values[2] := True;
  ExplorerMenuPage.Values[3] := True;
  Check(Failures = 0, 'unknown WSL verb/command descendants were deleted or overwritten');
end;

procedure CheckChoices;
var
  Index: Integer;
  Root, Argument, Value: string;
  Names, Distros: TArrayOfString;
  Failed: Boolean;
begin
  Check(ExplorerMenuPage.Values[0] and ExplorerMenuPage.Values[1] and
    ExplorerMenuPage.Values[2] and ExplorerMenuPage.Values[3], 'old configuration keeps all entries');
  for Index := 0 to 1 do begin
    Root := RegistryRoot + '\root' + IntToStr(Index);
    if Index = 0 then Argument := '%1' else Argument := '%V';
    UpdateExplorerMenusAt(Root, Executable, Argument);
    Check(RegQueryStringValue(HKCU, Root + '\Pebrel\command', '', Value) and
      (Value = DefaultExplorerMenuCommand(Executable, Argument)), 'ordinary entry keeps cwd argv');
    ExplorerMenuPage.Values[3] := False;
    UpdateExplorerMenusAt(Root, Executable, Argument);
    Check(RegGetSubkeyNames(HKCU, Root + '\PebrelWslMenu\shell', Names) and
      (GetArrayLength(Names) = 1), 'deselected distro is removed');
    Check(RegQueryStringValue(HKCU, Root + '\PebrelWslMenu\shell\PebrelWsl0',
      'MUIVerb', Value) and (Value = 'Ubuntu Test'), 'selected distro remains');
    ExplorerMenuPage.Values[2] := False;
    UpdateExplorerMenusAt(Root, Executable, Argument);
    Check(not RegKeyExists(HKCU, Root + '\PebrelWslMenu') and
      RegKeyExists(HKCU, Root + '\Pebrel'), 'zero selected distros removes only the cascade');
    ExplorerMenuPage.Values[2] := True;
    ExplorerMenuPage.Values[1] := False;
    UpdateExplorerMenusAt(Root, Executable, Argument);
    Check(not RegKeyExists(HKCU, Root + '\Pebrel'), 'ordinary entry can be removed independently');
    ExplorerMenuPage.Values[1] := True;
    ExplorerMenuPage.Values[3] := True;
    UpdateExplorerMenusAt(Root, Executable, Argument);
    Check(RegGetSubkeyNames(HKCU, Root + '\PebrelWslMenu\shell', Names) and
      (GetArrayLength(Names) = 2), 'reselected distro is added');
    ExplorerMenuPage.Values[0] := False;
    UpdateExplorerMenusAt(Root, Executable, Argument);
    Check(not RegKeyExists(HKCU, Root + '\Pebrel') and
      not RegKeyExists(HKCU, Root + '\PebrelWslMenu'), 'master removes owned entries');
    Check(ExplorerMenuPage.Values[2] and ExplorerMenuPage.Values[3], 'master preserves individual choices');
    ExplorerMenuPage.Values[0] := True;
    UpdateExplorerMenusAt(Root, Executable, Argument);
    Check(RegWriteStringValue(HKCU, Root + '\Pebrel\Custom', '', 'user data'), 'seed edited ordinary key');
    UpdateDefaultExplorerMenuAt(Root, Executable, Argument, False);
    Check(RegKeyExists(HKCU, Root + '\Pebrel\Custom'), 'edited subtree survives removal');
    Failed := False;
    try
      UpdateDefaultExplorerMenuAt(Root, Executable, Argument, True);
    except
      Failed := True;
    end;
    Check(Failed, 'edited ordinary key fails visibly on registration');
    Check(RegDeleteKeyIncludingSubkeys(HKCU, Root + '\Pebrel'), 'clear fixture ordinary key');
    Check(RegWriteStringValue(HKCU, Root + '\Pebrel\command', '', 'foreign command'), 'seed foreign ordinary key');
    UpdateDefaultExplorerMenuAt(Root, Executable, Argument, False);
    Check(RegQueryStringValue(HKCU, Root + '\Pebrel\command', '', Value) and
      (Value = 'foreign command'), 'foreign ordinary command survives removal');
    Check(RegWriteStringValue(HKCU, Root + '\PebrelWslForeign\command', '',
      '"D:\other\pebrel.exe" --gpui --shell "wsl:Foreign"'), 'seed foreign WSL key');
    ExplorerMenuPage.Values[0] := False;
    UpdateExplorerMenusAt(Root, Executable, Argument);
    Check(RegKeyExists(HKCU, Root + '\PebrelWslForeign'), 'master preserves foreign WSL entry');
    ExplorerMenuPage.Values[0] := True;
  end;
  ExplorerMenuPage.Values[0] := False;
  ExplorerMenuPage.Values[1] := False;
  ExplorerMenuPage.Values[3] := False;
  SaveExplorerMenuChoices;
  Distros := ExplorerMenuDistros;
  SetArrayLength(Distros, 3);
  Distros[0] := '开发环境';
  Distros[1] := 'Ubuntu Test';
  Distros[2] := 'New Distro';
  CreateExplorerMenuPage(ExplorerMenuSettings, Distros);
  Check(not ExplorerMenuPage.Values[0] and not ExplorerMenuPage.Values[1], 'upgrade keeps master and ordinary choices');
  Check(not ExplorerMenuPage.Values[2] and ExplorerMenuPage.Values[3], 'distro names preserve choices across enumeration order changes');
  Check(not ExplorerMenuPage.Values[4], 'new distro starts unchecked after opt-in selection');
  Check(not ExplorerMenuPage.CheckListBox.ItemEnabled[3], 'master visibly disables rows');
  ExplorerMenuPage.Values[0] := True;
  ExplorerMenuSelectionChanged(nil);
  Check(ExplorerMenuPage.CheckListBox.ItemEnabled[3], 'master re-enables rows');
  SaveExplorerMenuChoices;
  Check(ExplorerMenuChoice(ExplorerMenuSettings, 'Enabled', False) and
    ExplorerMenuChoice(ExplorerMenuSettings, 'wsl:Ubuntu Test', False), 'selection roundtrip');
end;

procedure InitializeWizard;
var
  Distros: TArrayOfString;
  Report: string;
begin
  RegistryRoot := 'Software\PebrelTestFixtures\' + ExtractFileName(ExpandConstant('{#FixtureRoot}'));
  Executable := ExpandConstant('{#FixtureRoot}') + '\Program Files\pebrel.exe';
  SetArrayLength(Distros, 2);
  Distros[0] := 'Ubuntu Test';
  Distros[1] := '开发环境';
  CreateExplorerMenuPage(RegistryRoot + '\Choices', Distros);
  if ExpandConstant('{param:ExplorerUi|0}') = '1' then
    Exit;
  try
    try
      CheckEditedWslSubtrees;
      CheckChoices;
      Report := 'PASS: ' + IntToStr(Checks) + ' Explorer selection checks';
    except
      Report := 'FAIL: ' + GetExceptionMessage;
    end;
    SaveStringToFile(ExpandConstant('{#FixtureRoot}') + '\choices-result.txt', Report, False);
  finally
    RegDeleteKeyIncludingSubkeys(HKCU, RegistryRoot);
  end;
end;

procedure CurPageChanged(CurPageID: Integer);
var
  Coordinates: string;
begin
  if (ExpandConstant('{param:ExplorerUi|0}') = '1') and
    (CurPageID = ExplorerMenuPage.ID) then begin
    WizardForm.ActiveControl := ExplorerMenuPage.CheckListBox;
    Coordinates := IntToStr(ExplorerMenuPage.CheckListBox.Handle);
    SaveStringToFile(ExpandConstant('{#FixtureRoot}') + '\ui-handle.txt', Coordinates, False);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (ExpandConstant('{param:ExplorerUi|0}') = '1') and (CurStep = ssPostInstall) then begin
    UpdateExplorerMenusAt(RegistryRoot + '\selected', Executable, '%1');
    UpdateExplorerMenusAt(RegistryRoot + '\background', Executable, '%V');
    SaveExplorerMenuChoices;
    SaveStringToFile(ExpandConstant('{#FixtureRoot}') + '\ui-result.txt', 'PASS', False);
  end;
end;
