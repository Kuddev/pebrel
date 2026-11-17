{ Native helper owns per-user package registration and installation ownership. }
var
  ModernContextMenuFailed: Boolean;

function HasModernExplorer: Boolean;
var
  Version: TWindowsVersion;
begin
  GetWindowsVersionEx(Version);
  Result := (Version.Major > 10) or
    ((Version.Major = 10) and (Version.Build >= 22000));
end;

function RunContextMenuHelper(const Operation: string): Boolean;
var
  Helper: string;
  ResultCode: Integer;
begin
  Helper := ExpandConstant('{app}\context-menu\pebrel-context-menu-setup.exe');
  ResultCode := -1;
  Result := Exec(Helper, Operation, ExpandConstant('{app}'), SW_HIDE,
    ewWaitUntilTerminated, ResultCode);
  Result := Result and (ResultCode = 0);
  if not Result then
    Log(Format('Context-menu %s failed (%d); see context-menu\setup.log', [Operation, ResultCode]));
end;

procedure RegisterModernContextMenu;
begin
  if not HasModernExplorer then
    Exit;
  ModernContextMenuFailed := not RunContextMenuHelper('--register');
  if ModernContextMenuFailed then
    SuppressibleMsgBox(CustomMessage('ModernContextMenuFailed'), mbError, MB_OK, IDOK);
end;

<event('CurUninstallStepChanged')>
procedure UnregisterModernContextMenu(Step: TUninstallStep);
begin
  if (Step <> usUninstall) or not HasModernExplorer or
    not FileExists(ExpandConstant('{app}\context-menu\pebrel-context-menu-setup.exe')) then
    Exit;
  { usUninstall runs after confirmation and before file removal. An exception
    here is fatal, preserving files if the owned registration cannot be removed. }
  if not RunContextMenuHelper('--unregister') then
    RaiseException(CustomMessage('ModernContextMenuRemoveFailed'));
end;
