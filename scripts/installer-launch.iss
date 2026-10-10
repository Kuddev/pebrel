{ Isolate the installed app from Setup's compatibility layer. PrivilegesRequired
  is lowest, so Setup does not create an elevated/original-user spawn server. }

var
  PebrelLaunchCompatibilityLayer: String;
  PebrelLaunchEnvironmentChanged: Boolean;

function RemovePebrelLaunchEnvironmentValue(Name: String; NullValue: LongWord): Boolean;
  external 'SetEnvironmentVariableW@kernel32.dll stdcall';
function SetPebrelLaunchEnvironmentValue(Name, Value: String): Boolean;
  external 'SetEnvironmentVariableW@kernel32.dll stdcall';

procedure LaunchEnvironmentError;
var
  ErrorCode: LongWord;
begin
  ErrorCode := DLLGetLastError;
  RaiseException(FmtMessage(CustomMessage('LaunchEnvironmentFailed'), [SysErrorMessage(ErrorCode)]));
end;

function PebrelLaunchParameters(Param: String): String;
begin
  if not PebrelLaunchEnvironmentChanged then begin
    PebrelLaunchCompatibilityLayer := GetEnv('__COMPAT_LAYER');
    if PebrelLaunchCompatibilityLayer <> '' then begin
      { Zero is a NULL value pointer: delete the process variable, not a
        registry setting. The already-running installer's DPI mode is unchanged. }
      if not RemovePebrelLaunchEnvironmentValue('__COMPAT_LAYER', 0) then
        LaunchEnvironmentError;
      PebrelLaunchEnvironmentChanged := True;
    end;
  end;
  { Expand this code constant inside ProcessRunEntry. Inno catches exceptions
    from BeforeInstall and would otherwise still execute the unsanitized app. }
  Result := '--gpui';
end;

procedure RestorePebrelLaunchEnvironment;
begin
  if not PebrelLaunchEnvironmentChanged then
    Exit;
  if not SetPebrelLaunchEnvironmentValue('__COMPAT_LAYER', PebrelLaunchCompatibilityLayer) then
    LaunchEnvironmentError;
  PebrelLaunchEnvironmentChanged := False;
  PebrelLaunchCompatibilityLayer := '';
end;

<event('DeinitializeSetup')>
procedure RestorePebrelLaunchEnvironmentOnExit;
begin
  { Backstop if Setup exits before the Run entry's AfterInstall callback. }
  RestorePebrelLaunchEnvironment;
end;
