// Included by the product installer and the isolated migration fixture.
const
#ifdef AcceptanceFixture
  ProductUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{76B778B5-76C6-4F60-9431-9E67C2A351AF}_is1';
  ProductSettingsKey = 'Software\PebrelUpdateAcceptance';
  LegacySettingsKey = 'Software\PebrelUpdateAcceptanceLegacy';
#else
  ProductUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{61022144-7D0A-4E54-94F2-C329A8F58656}_is1';
  ProductSettingsKey = 'Software\Pebrel';
  LegacySettingsKey = 'Software\Nebula Terminal';
#endif

var
  PreviousInstallDir: string;
  LegacyInstallDir: string;
  LegacyUninstaller: string;
  MigrationFailed: Boolean;

function MigrationFileAttributes(FileName: string): LongWord;
  external 'GetFileAttributesW@kernel32.dll stdcall';

function NormalizedDirectory(Value: string): string;
begin
  Result := RemoveBackslashUnlessRoot(ExpandFileName(RemoveQuotes(Trim(Value))));
end;

function SameDirectory(Left, Right: string): Boolean;
begin
  Result := CompareText(NormalizedDirectory(Left), NormalizedDirectory(Right)) = 0;
end;

function SuggestedInstallDir(Previous, Fallback: string): string;
begin
  Result := Fallback;
  if Previous = '' then
    Exit;
  Result := NormalizedDirectory(Previous);
  if CompareText(ExtractFileName(Result), 'Nebula Terminal') = 0 then
    Result := AddBackslash(ExtractFileDir(Result)) + 'Pebrel';
end;

function PathContainsDirectory(Value, Directory: string): Boolean;
var
  Entry: string;
  Separator: Integer;
begin
  Result := False;
  repeat
    Separator := Pos(';', Value);
    if Separator = 0 then
      Separator := Length(Value) + 1;
    Entry := Copy(Value, 1, Separator - 1);
    if (Trim(Entry) <> '') and SameDirectory(Entry, Directory) then begin
      Result := True;
      Exit;
    end;
    Delete(Value, 1, Separator);
  until Value = '';
end;

function RemovePathDirectory(Value, Directory: string): string;
var
  Entry: string;
  Separator: Integer;
  FirstEntry: Boolean;
begin
  Result := '';
  FirstEntry := True;
  repeat
    Separator := Pos(';', Value);
    if Separator = 0 then
      Separator := Length(Value) + 1;
    Entry := Copy(Value, 1, Separator - 1);
    if (Trim(Entry) = '') or not SameDirectory(Entry, Directory) then begin
      if not FirstEntry then
        Result := Result + ';';
      Result := Result + Entry;
      FirstEntry := False;
    end;
    Delete(Value, 1, Separator);
  until Value = '';
end;

function IsLegacyUninstaller(Directory, FileName: string): Boolean;
var
  Name: string;
  Index: Integer;
begin
  Name := Lowercase(ExtractFileName(FileName));
  Result := SameDirectory(ExtractFileDir(FileName), Directory) and
    (Length(Name) = 12) and (Copy(Name, 1, 5) = 'unins') and
    (Copy(Name, 9, 4) = '.exe');
  if Result then
    for Index := 6 to 8 do
      if (Name[Index] < '0') or (Name[Index] > '9') then
        Result := False;
end;

function DefaultInstallDir(Param: string): string;
begin
  Result := SuggestedInstallDir(PreviousInstallDir,
    ExpandConstant('{localappdata}\Programs\Pebrel'));
end;

procedure InitializeWizard;
begin
  { Reuse the registered directory for normal upgrades, so Inno recognizes it
    as an existing installation. Keep the old-brand relocation suggestion and
    an explicit /DIR selection intact. }
  if (PreviousInstallDir <> '') and
    (CompareText(ExtractFileName(NormalizedDirectory(PreviousInstallDir)), 'Nebula Terminal') = 0) and
    (ExpandConstant('{param:DIR|}') = '') then
    WizardForm.DirEdit.Text := SuggestedInstallDir(PreviousInstallDir,
      ExpandConstant('{localappdata}\Programs\Pebrel'));
end;

procedure DiscoverLegacyInstallation;
var
  Pending, UninstallCommand: string;
begin
  RegQueryStringValue(HKCU, ProductUninstallKey, 'Inno Setup: App Path', PreviousInstallDir);
  if (PreviousInstallDir <> '') and
    FileExists(AddBackslash(PreviousInstallDir) + 'nebula.exe') then begin
    LegacyInstallDir := NormalizedDirectory(PreviousInstallDir);
    if RegQueryStringValue(HKCU, ProductUninstallKey, 'UninstallString', UninstallCommand) then begin
      UninstallCommand := RemoveQuotes(Trim(UninstallCommand));
      if IsLegacyUninstaller(LegacyInstallDir, UninstallCommand) then
        LegacyUninstaller := UninstallCommand;
    end;
  end;
  if RegQueryStringValue(HKCU, ProductSettingsKey, 'PendingLegacyInstallDir', Pending) then begin
    if Trim(Pending) = '' then
      RaiseException('The pending legacy installation directory is empty.');
    if (LegacyInstallDir <> '') and not SameDirectory(LegacyInstallDir, Pending) then
      RaiseException('Another legacy installation is awaiting migration: ' + Pending);
    LegacyInstallDir := NormalizedDirectory(Pending);
    RegQueryStringValue(HKCU, ProductSettingsKey, 'PendingLegacyUninstaller', LegacyUninstaller);
    if not IsLegacyUninstaller(LegacyInstallDir, LegacyUninstaller) then
      LegacyUninstaller := '';
  end;
end;

function IsExecutableRunning(FileName: string): Boolean;
var
  Locator, Service, Processes, Process: Variant;
  Index: Integer;
  Name: string;
begin
  Result := False;
  if not FileExists(FileName) then
    Exit;
  Name := ExtractFileName(FileName);
  StringChangeEx(Name, '''', '''''', True);
  Locator := CreateOleObject('WbemScripting.SWbemLocator');
  Service := Locator.ConnectServer('', 'root\CIMV2');
  Processes := Service.ExecQuery('SELECT ExecutablePath FROM Win32_Process WHERE Name = ''' + Name + '''');
  for Index := 0 to Processes.Count - 1 do begin
    Process := Processes.ItemIndex(Index);
    if not VarIsNull(Process.ExecutablePath) then
      if SameDirectory(Process.ExecutablePath, FileName) then begin
        Result := True;
        Exit;
      end;
  end;
end;

function SafePayloadPath(Root, Relative: string): string;
var
  Part, Parent: string;
  Attributes: LongWord;
begin
  Root := NormalizedDirectory(Root);
  Result := ExpandFileName(AddBackslash(Root) + Relative);
  if Pos(Lowercase(AddBackslash(Root)), Lowercase(Result)) <> 1 then
    RaiseException('Invalid legacy payload path: ' + Result);
  Part := Result;
  repeat
    Attributes := MigrationFileAttributes(Part);
    if (Attributes <> $FFFFFFFF) and ((Attributes and $400) <> 0) then
      RaiseException('Linked legacy installation path must be migrated manually: ' + Part);
    Parent := ExtractFileDir(Part);
    if SameDirectory(Parent, Part) then
      Break;
    Part := Parent;
  until Part = '';
end;

procedure RemoveLegacyFile(Root, Relative: string);
var
  FileName: string;
begin
  FileName := SafePayloadPath(Root, Relative);
  if FileExists(FileName) and not DeleteFile(FileName) then
    RaiseException('Unable to remove old installation file: ' + FileName);
end;

procedure RemoveEmptyLegacyDir(Root, Relative: string);
var
  Directory: string;
begin
  Directory := SafePayloadPath(Root, Relative);
  if DirExists(Directory) then
    RemoveDir(Directory);
end;

procedure RemoveLegacyPayload(OldDir, NewDir, Uninstaller: string);
begin
  if OldDir = '' then
    Exit;
  RemoveLegacyFile(OldDir, 'runtime\nebula-hook.exe');
  RemoveLegacyFile(OldDir, 'nebula-hook.exe');
  RemoveLegacyFile(OldDir, 'skills\nebula-runtime\agents\openai.yaml');
  RemoveLegacyFile(OldDir, 'skills\nebula-runtime\SKILL.md');
  RemoveEmptyLegacyDir(OldDir, 'skills\nebula-runtime\agents');
  RemoveEmptyLegacyDir(OldDir, 'skills\nebula-runtime');
  if not SameDirectory(OldDir, NewDir) then begin
    RemoveLegacyFile(OldDir, 'runtime\conpty.dll');
    RemoveLegacyFile(OldDir, 'runtime\OpenConsole.exe');
    RemoveLegacyFile(OldDir, 'conpty.dll');
    RemoveLegacyFile(OldDir, 'OpenConsole.exe');
    RemoveLegacyFile(OldDir, 'README.md');
    RemoveLegacyFile(OldDir, 'docs\CHANGELOG.md');
    RemoveLegacyFile(OldDir, 'docs\INSTALL.md');
    RemoveLegacyFile(OldDir, 'docs\lua-configuration.md');
    RemoveLegacyFile(OldDir, 'docs\runtime-control-api.md');
    RemoveLegacyFile(OldDir, 'docs\runtime-api-v1.schema.json');
    RemoveLegacyFile(OldDir, 'fonts\MapleMonoNormal-NF-CN-Regular.ttf');
    RemoveLegacyFile(OldDir, 'licenses\LICENSE');
    RemoveLegacyFile(OldDir, 'licenses\LICENSE-LUA');
    RemoveLegacyFile(OldDir, 'licenses\LICENSE-MLUA');
    RemoveLegacyFile(OldDir, 'licenses\THIRD-PARTY-NOTICES');
    if IsLegacyUninstaller(OldDir, Uninstaller) then begin
      RemoveLegacyFile(OldDir, ExtractFileName(Uninstaller));
      RemoveLegacyFile(OldDir, ChangeFileExt(ExtractFileName(Uninstaller), '.dat'));
      RemoveLegacyFile(OldDir, ChangeFileExt(ExtractFileName(Uninstaller), '.msg'));
    end;
    RemoveEmptyLegacyDir(OldDir, 'runtime');
    RemoveEmptyLegacyDir(OldDir, 'docs');
    RemoveEmptyLegacyDir(OldDir, 'fonts');
    RemoveEmptyLegacyDir(OldDir, 'licenses');
    RemoveEmptyLegacyDir(OldDir, 'skills');
  end;
  RemoveLegacyFile(OldDir, 'nebula.exe');
  if not SameDirectory(OldDir, NewDir) then
    RemoveDir(OldDir);
end;

procedure RemoveOwnedShortcut(FileName, Target: string);
var
  Shell, Shortcut: Variant;
begin
  if not FileExists(FileName) then
    Exit;
  Shell := CreateOleObject('WScript.Shell');
  Shortcut := Shell.CreateShortcut(FileName);
  if SameDirectory(Shortcut.TargetPath, Target) and not DeleteFile(FileName) then
    RaiseException('Unable to remove old shortcut: ' + FileName);
end;

procedure RemoveOwnedRegistryKey(Key, ValueName, Expected: string);
var
  Value: string;
begin
  if RegQueryStringValue(HKCU, Key, ValueName, Value) and
    (CompareText(Value, Expected) = 0) and not RegDeleteKeyIncludingSubkeys(HKCU, Key) then
    RaiseException('Unable to remove old registration: ' + Key);
end;

procedure RemoveOwnedContextMenu(Key, ExpectedCommand: string);
var
  Command: string;
begin
  if RegQueryStringValue(HKCU, Key + '\command', '', Command) and
    (CompareText(Command, ExpectedCommand) = 0) and not RegDeleteKeyIncludingSubkeys(HKCU, Key) then
    RaiseException('Unable to remove old context menu: ' + Key);
end;

{ —— 按 WSL 发行版注册的右键项 ——

  WSL 发行版子菜单按**安装那一刻**机器上的发行版动态生成：
  `Lxss` 里的 `DistributionName` 是唯一真相，枚举口径与 Rust 侧
  `shell_detect::find_wsl_distros` 一致（跳过 `docker-desktop*` 这类 plumbing 发行版）。

  走 `[Code]` 而不是 `[Registry]` 的原因只有一个：键的条数取决于机器上有几个发行版。
  代价是这些键没有 `uninsdeletekey`，卸载由 `RemoveOwnedWslContextMenus` 按名字前缀
  加精确命令与级联菜单归属标记认领后删除。 }

const
  LxssRoot = 'Software\Microsoft\Windows\CurrentVersion\Lxss';

function WslDistroNames: TArrayOfString;
var
  Names: TArrayOfString;
  Index, Count: Integer;
  Name: string;
begin
  SetArrayLength(Result, 0);
  if not RegGetSubkeyNames(HKCU, LxssRoot, Names) then
    Exit;
  SetArrayLength(Result, GetArrayLength(Names));
  Count := 0;
  for Index := 0 to GetArrayLength(Names) - 1 do
    if RegQueryStringValue(HKCU, LxssRoot + '\' + Names[Index], 'DistributionName', Name) then
      if (Name <> '') and (Pos('docker-desktop', Lowercase(Name)) <> 1) then begin
        Result[Count] := Name;
        Count := Count + 1;
      end;
  SetArrayLength(Result, Count);
end;

{ WSL 右键项按精确可执行文件前缀归属；级联菜单还校验所有子项，保留用户编辑。
  发行版移除后，下次安装重建菜单时不留下旧项。 }
function IsOwnedWslCommand(Command, Executable: string): Boolean;
var
  Prefix: string;
begin
  Prefix := '"' + Executable + '" --gpui --shell "wsl:';
  Result := CompareText(Copy(Command, 1, Length(Prefix)), Prefix) = 0;
end;

function IsOwnedWslMenu(Key, Executable: string): Boolean;
var
  Owner, Command: string;
  Children, Subkeys: TArrayOfString;
  Index: Integer;
begin
  Result := False;
  if not RegQueryStringValue(HKCU, Key, 'PebrelOwner', Owner) or
    (CompareText(Owner, Executable) <> 0) then
    Exit;
  if not RegGetSubkeyNames(HKCU, Key, Subkeys) then
    Exit;
  for Index := 0 to GetArrayLength(Subkeys) - 1 do
    if CompareText(Subkeys[Index], 'shell') <> 0 then
      Exit;
  if GetArrayLength(Subkeys) > 0 then begin
    if not RegGetSubkeyNames(HKCU, Key + '\shell', Children) then
      Exit;
    for Index := 0 to GetArrayLength(Children) - 1 do begin
      if not RegQueryStringValue(HKCU, Key + '\shell\' + Children[Index] + '\command', '', Command) or
        not IsOwnedWslCommand(Command, Executable) then
        Exit;
    end;
  end;
  Result := True;
end;

procedure RemoveOwnedWslContextMenusAt(Root, Executable: string);
var
  Key, Command: string;
  Names: TArrayOfString;
  NameIndex: Integer;
begin
  if RegGetSubkeyNames(HKCU, Root, Names) then
    for NameIndex := 0 to GetArrayLength(Names) - 1 do
      if Pos('PebrelWsl', Names[NameIndex]) = 1 then begin
        Key := Root + '\' + Names[NameIndex];
        Command := '';
        if IsOwnedWslMenu(Key, Executable) or
          (RegQueryStringValue(HKCU, Key + '\command', '', Command) and
            IsOwnedWslCommand(Command, Executable)) then
          if not RegDeleteKeyIncludingSubkeys(HKCU, Key) then
            RaiseException('Unable to remove an owned WSL context menu: ' + Key);
      end;
end;

procedure RemoveOwnedWslContextMenus;
var
  Roots: array[0..1] of string;
  Index: Integer;
begin
  Roots[0] := 'Software\Classes\Directory\shell';
  Roots[1] := 'Software\Classes\Directory\Background\shell';
  for Index := 0 to 1 do
    RemoveOwnedWslContextMenusAt(Roots[Index], ExpandConstant('{app}\pebrel.exe'));
end;

procedure RegisterWslContextMenuAt(Root, Executable, DirectoryArgument: string;
  Distros: TArrayOfString);
var
  Index: Integer;
  Distro, Verb, Command, Key, Menu: string;
begin
  Menu := Root + '\PebrelWslMenu';
  { Never overwrite an unknown installation or an edited submenu. }
  if RegKeyExists(HKCU, Menu) and not IsOwnedWslMenu(Menu, Executable) then
    RaiseException(FmtMessage(CustomMessage('WslMenuConflict'), [Menu]));
  RemoveOwnedWslContextMenusAt(Root, Executable);
  if GetArrayLength(Distros) = 0 then
    Exit;
  if not RegWriteStringValue(HKCU, Menu, 'MUIVerb', CustomMessage('OpenInPebrelWsl')) or
    not RegWriteStringValue(HKCU, Menu, 'Icon', Executable + ',0') or
    not RegWriteStringValue(HKCU, Menu, 'SubCommands', '') or
    not RegWriteStringValue(HKCU, Menu, 'PebrelOwner', Executable) then
    RaiseException(CustomMessage('WslMenuRegistrationFailed'));
  for Index := 0 to GetArrayLength(Distros) - 1 do begin
    Distro := Distros[Index];
    { 键名用序号而不是发行版名：注册表键名里带空格与非 ASCII 只会给自己找麻烦，
      何况我们靠前缀认领。 }
    Verb := 'PebrelWsl' + IntToStr(Index);
    { `--shell` 必须排在 `--working-directory` 之前：盘根（`D:\`）时后者的收尾
      反斜杠会吃掉它的收尾引号，并把后面整段并进同一个参数（issue #36 的另一面），
      顺序写反会静默开出一个既没有 cwd、也没用上指定发行版的标签。 }
    Command := '"' + Executable + '" --gpui --shell "wsl:' + Distro + '" --working-directory ';
    Key := Menu + '\shell\' + Verb;
    if not RegWriteStringValue(HKCU, Key, 'MUIVerb', Distro) or
      not RegWriteStringValue(HKCU, Key, 'Icon', Executable + ',0') or
      not RegWriteStringValue(HKCU, Key + '\command', '', Command + '"' + DirectoryArgument + '"') then
      RaiseException('Unable to register the WSL context menu for ' + Distro + '.');
  end;
end;

procedure RegisterWslContextMenus;
var
  Distros: TArrayOfString;
  Executable: string;
begin
  Distros := WslDistroNames;
  Executable := ExpandConstant('{app}\pebrel.exe');
  RegisterWslContextMenuAt('Software\Classes\Directory\shell', Executable, '%1', Distros);
  RegisterWslContextMenuAt('Software\Classes\Directory\Background\shell', Executable, '%V', Distros);
end;

procedure MigrateLegacyIntegrations;
var
  Executable, ExistingPath, Key: string;
begin
  Executable := AddBackslash(LegacyInstallDir) + 'nebula.exe';
  RemoveOwnedShortcut(ExpandConstant('{autodesktop}\Nebula Terminal.lnk'), Executable);
  RemoveOwnedShortcut(ExpandConstant('{autodesktop}\Pebrel.lnk'), Executable);
  RemoveOwnedShortcut(ExpandConstant('{userstartup}\Nebula Terminal.lnk'), Executable);
  RemoveOwnedShortcut(ExpandConstant('{userstartup}\Pebrel.lnk'), Executable);
  RemoveOwnedShortcut(ExpandConstant('{userprograms}\Nebula Terminal\Nebula Terminal.lnk'), Executable);
  RemoveOwnedShortcut(ExpandConstant('{userprograms}\Pebrel\Pebrel.lnk'), Executable);
  if LegacyUninstaller <> '' then begin
    RemoveOwnedShortcut(ExpandConstant('{userprograms}\Nebula Terminal\Uninstall Nebula Terminal.lnk'), LegacyUninstaller);
    RemoveOwnedShortcut(ExpandConstant('{userprograms}\Nebula Terminal\卸载 Nebula Terminal.lnk'), LegacyUninstaller);
  end;
  RemoveDir(ExpandConstant('{userprograms}\Nebula Terminal'));
  RemoveOwnedRegistryKey('Software\Microsoft\Windows\CurrentVersion\App Paths\nebula.exe', '', Executable);
  Key := 'Software\Classes\Directory\Background\shell\NebulaTerminal';
  RemoveOwnedContextMenu(Key, '"' + Executable + '" --gpui --working-directory "%V"');
  Key := 'Software\Classes\Directory\shell\NebulaTerminal';
  RemoveOwnedContextMenu(Key, '"' + Executable + '" --gpui --working-directory "%1"');
  if RegValueExists(HKCU, LegacySettingsKey, 'InstallerAddedToPath') then begin
    if SameDirectory(LegacyInstallDir, ExpandConstant('{app}')) then begin
      if not RegWriteDWordValue(HKCU, ProductSettingsKey, 'InstallerAddedToPath', 1) then
        RaiseException('Unable to transfer PATH ownership.');
    end else if RegQueryStringValue(HKCU, 'Environment', 'Path', ExistingPath) then begin
      if not RegWriteExpandStringValue(HKCU, 'Environment', 'Path',
        RemovePathDirectory(ExistingPath, LegacyInstallDir)) then
        RaiseException('Unable to remove old installation directory from PATH.');
    end;
    if not RegDeleteValue(HKCU, LegacySettingsKey, 'InstallerAddedToPath') then
      RaiseException('Unable to clear previous PATH ownership.');
    RegDeleteKeyIfEmpty(HKCU, LegacySettingsKey);
  end;
end;

#ifndef MigrationFixture
function InitializeSetup: Boolean;
begin
  Result := True;
  try
    DiscoverLegacyInstallation;
  except
    Result := False;
    SuppressibleMsgBox(FmtMessage(CustomMessage('MigrationPreflightFailed'), [GetExceptionMessage]),
      mbError, MB_OK, IDOK);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): string;
var
  Executable: string;
begin
  Result := '';
  if LegacyInstallDir = '' then
    Exit;
  try
    Executable := SafePayloadPath(LegacyInstallDir, 'nebula.exe');
    if IsExecutableRunning(Executable) then
      Result := FmtMessage(CustomMessage('CloseLegacyProgram'), [Executable]);
  except
    Result := FmtMessage(CustomMessage('MigrationPreflightFailed'), [GetExceptionMessage]);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep <> ssPostInstall then
    Exit;
  { 与旧版迁移无关，装完就要做：菜单项取决于本机装了哪些 WSL 发行版。 }
  RegisterWslContextMenus;
  if LegacyInstallDir = '' then
    Exit;
  try
    if not RegWriteStringValue(HKCU, ProductSettingsKey, 'PendingLegacyInstallDir', LegacyInstallDir) or
      not RegWriteStringValue(HKCU, ProductSettingsKey, 'PendingLegacyUninstaller', LegacyUninstaller) then
      RaiseException('Unable to record the previous installation for migration.');
    MigrateLegacyIntegrations;
    RemoveLegacyPayload(LegacyInstallDir, ExpandConstant('{app}'), LegacyUninstaller);
    if not RegDeleteValue(HKCU, ProductSettingsKey, 'PendingLegacyInstallDir') or
      not RegDeleteValue(HKCU, ProductSettingsKey, 'PendingLegacyUninstaller') then
      RaiseException('Unable to clear the completed migration record.');
  except
    MigrationFailed := True;
    Log('Legacy migration failed: ' + GetExceptionMessage);
    SuppressibleMsgBox(FmtMessage(CustomMessage('MigrationFailed'), [GetExceptionMessage]),
      mbError, MB_OK, IDOK);
  end;
end;

function GetCustomSetupExitCode: Integer;
begin
  Result := 0;
  if MigrationFailed then
    Result := 1;
end;
#endif
