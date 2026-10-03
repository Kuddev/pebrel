// Explorer choices belong to the installer, alongside its owned registrations.
var
  ExplorerMenuPage: TInputOptionWizardPage;
  ExplorerMenuDistros: TArrayOfString;
  ExplorerMenuSettings: string;

function ExplorerMenuChoice(Key, Name: string; Default: Boolean): Boolean;
var
  Value: Cardinal;
begin
  Result := Default;
  if RegQueryDWordValue(HKCU, Key, Name, Value) then
    Result := Value <> 0;
end;

procedure ExplorerMenuSelectionChanged(Sender: TObject);
var
  Index: Integer;
begin
  for Index := 1 to ExplorerMenuPage.CheckListBox.Items.Count - 1 do
    ExplorerMenuPage.CheckListBox.ItemEnabled[Index] := ExplorerMenuPage.Values[0];
end;

procedure CreateExplorerMenuPage(SettingsKey: string; Distros: TArrayOfString);
var
  Index: Integer;
  InitialSelection: Boolean;
begin
  ExplorerMenuSettings := SettingsKey;
  ExplorerMenuDistros := Distros;
  InitialSelection := not RegValueExists(HKCU, SettingsKey, 'Saved');
  ExplorerMenuPage := CreateInputOptionPage(wpSelectDir,
    CustomMessage('ExplorerMenuTitle'), CustomMessage('ExplorerMenuDescription'),
    CustomMessage('ExplorerMenuHelp'), False, True);
  ExplorerMenuPage.Add(CustomMessage('ExplorerMenuEnabled'));
  ExplorerMenuPage.Values[0] := ExplorerMenuChoice(SettingsKey, 'Enabled', True);
  ExplorerMenuPage.Add(CustomMessage('OpenInPebrel'));
  ExplorerMenuPage.Values[1] := ExplorerMenuChoice(SettingsKey, 'Default', True);
  for Index := 0 to GetArrayLength(Distros) - 1 do begin
    ExplorerMenuPage.Add(CustomMessage('OpenInPebrel') + ' (' + Distros[Index] + ')');
    ExplorerMenuPage.Values[Index + 2] :=
      ExplorerMenuChoice(SettingsKey, 'wsl:' + Distros[Index], InitialSelection);
  end;
  ExplorerMenuPage.CheckListBox.OnClickCheck := @ExplorerMenuSelectionChanged;
  ExplorerMenuSelectionChanged(nil);
end;

procedure SaveExplorerMenuChoices;
var
  Index: Integer;
begin
  if not RegWriteDWordValue(HKCU, ExplorerMenuSettings, 'Enabled',
    Ord(ExplorerMenuPage.Values[0])) or
    not RegWriteDWordValue(HKCU, ExplorerMenuSettings, 'Default',
      Ord(ExplorerMenuPage.Values[1])) then
    RaiseException(CustomMessage('ExplorerMenuFailed'));
  for Index := 0 to GetArrayLength(ExplorerMenuDistros) - 1 do
    if not RegWriteDWordValue(HKCU, ExplorerMenuSettings, 'wsl:' + ExplorerMenuDistros[Index],
      Ord(ExplorerMenuPage.Values[Index + 2])) then
      RaiseException(CustomMessage('ExplorerMenuFailed'));
  if not RegWriteDWordValue(HKCU, ExplorerMenuSettings, 'Saved', 1) then
    RaiseException(CustomMessage('ExplorerMenuFailed'));
end;

function SelectedExplorerMenuDistros: TArrayOfString;
var
  Index, Count: Integer;
begin
  SetArrayLength(Result, GetArrayLength(ExplorerMenuDistros));
  Count := 0;
  if ExplorerMenuPage.Values[0] then
    for Index := 0 to GetArrayLength(ExplorerMenuDistros) - 1 do
      if ExplorerMenuPage.Values[Index + 2] then begin
        Result[Count] := ExplorerMenuDistros[Index];
        Count := Count + 1;
      end;
  SetArrayLength(Result, Count);
end;

function DefaultExplorerMenuCommand(Executable, DirectoryArgument: string): string;
begin
  Result := '"' + Executable + '" --gpui --working-directory "' + DirectoryArgument + '"';
end;

function IsOwnedDefaultExplorerMenu(Key, Command: string): Boolean;
var
  Existing: string;
begin
  Result := IsSingleCommandVerb(Key) and
    RegQueryStringValue(HKCU, Key + '\command', '', Existing) and
    (CompareText(Existing, Command) = 0);
end;

procedure UpdateDefaultExplorerMenuAt(Root, Executable, DirectoryArgument: string;
  Enabled: Boolean);
var
  Key, Command: string;
begin
  Key := Root + '\Pebrel';
  Command := DefaultExplorerMenuCommand(Executable, DirectoryArgument);
  if RegKeyExists(HKCU, Key) then begin
    if not IsOwnedDefaultExplorerMenu(Key, Command) then begin
      if Enabled then
        RaiseException(FmtMessage(CustomMessage('ExplorerMenuConflict'), [Key]));
      Exit;
    end;
    if not Enabled then begin
      RemoveOwnedContextMenu(Key, Command);
      Exit;
    end;
  end;
  if not Enabled then
    Exit;
  if not RegWriteStringValue(HKCU, Key, 'MUIVerb', CustomMessage('OpenInPebrel')) or
    not RegWriteStringValue(HKCU, Key, 'Icon', Executable + ',0') or
    not RegWriteStringValue(HKCU, Key + '\command', '', Command) then
    RaiseException(CustomMessage('ExplorerMenuFailed'));
end;

procedure UpdateExplorerMenusAt(Root, Executable, DirectoryArgument: string);
var
  Distros: TArrayOfString;
begin
  Distros := SelectedExplorerMenuDistros;
  if GetArrayLength(Distros) > 0 then
    RegisterWslContextMenuAt(Root, Executable, DirectoryArgument, Distros)
  else
    RemoveOwnedWslContextMenusAt(Root, Executable);
  UpdateDefaultExplorerMenuAt(Root, Executable, DirectoryArgument,
    ExplorerMenuPage.Values[0] and ExplorerMenuPage.Values[1]);
end;

procedure RegisterExplorerContextMenus;
var
  Executable: string;
begin
  Executable := ExpandConstant('{app}\pebrel.exe');
  UpdateExplorerMenusAt('Software\Classes\Directory\shell', Executable, '%1');
  UpdateExplorerMenusAt('Software\Classes\Directory\Background\shell', Executable, '%V');
  SaveExplorerMenuChoices;
end;

procedure RemoveOwnedExplorerContextMenus;
var
  Executable: string;
begin
  Executable := ExpandConstant('{app}\pebrel.exe');
  RemoveOwnedWslContextMenus;
  UpdateDefaultExplorerMenuAt('Software\Classes\Directory\shell', Executable, '%1', False);
  UpdateDefaultExplorerMenuAt('Software\Classes\Directory\Background\shell', Executable, '%V', False);
end;
