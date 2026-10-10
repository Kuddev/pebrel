#ifndef FixtureRoot
  #error FixtureRoot must point at an isolated workspace directory.
#endif

[Setup]
AppId=PebrelInstallerLaunchFixture
AppName=Pebrel Installer Launch Fixture
AppVersion=0.0.0
DefaultDirName={#FixtureRoot}
CreateAppDir=no
Uninstallable=no
PrivilegesRequired=lowest
OutputBaseFilename=installer-launch-fixture
Compression=none
SetupLogging=no

[CustomMessages]
LaunchEnvironmentFailed=Unable to update the launch environment: %1.

[Run]
Filename: "{#FixtureRoot}\dpi-child.exe"; Parameters: "{code:PebrelLaunchParameters}"; Flags: runhidden nowait; Check: IsRunEntryFixture; AfterInstall: RestorePebrelLaunchEnvironment

[Code]
#include "..\installer-launch.iss"

var
  Checks: Integer;

procedure Check(Condition: Boolean; Name: String);
begin
  if not Condition then
    RaiseException(Name);
  Checks := Checks + 1;
end;

function IsRunEntryFixture: Boolean;
begin
  Result := ExpandConstant('{param:runentry|0}') = '1';
end;

function ChildReport(Root, Parameters: String): String;
var
  ErrorCode: Integer;
  Report: AnsiString;
begin
  Check(SetPebrelLaunchEnvironmentValue('PEBREL_LAUNCH_FIXTURE_REPORT', Root + '\child.txt'),
    'set child report path');
  Check(Exec(Root + '\dpi-child.exe', Parameters, Root, SW_HIDE,
    ewWaitUntilTerminated, ErrorCode), 'start native DPI reporter');
  Check(ErrorCode = 0, 'DPI reporter succeeds');
  Check(LoadStringFromFile(Root + '\child.txt', Report), 'load child report');
  Result := String(Report);
end;

function InitializeSetup: Boolean;
var
  Root, OriginalLayer, Parameters, OriginalChild, CleanChild: String;
  ErrorCode: Integer;
begin
  Root := ExpandConstant('{#FixtureRoot}');
  OriginalLayer := GetEnv('__COMPAT_LAYER');
  if ExpandConstant('{param:deinit|0}') = '1' then begin
    Check(SetPebrelLaunchEnvironmentValue('__COMPAT_LAYER', 'HighDpiAware'), 'set shutdown layer');
    Parameters := PebrelLaunchParameters('');
    SaveStringToFile(Root + '\result.txt', 'PENDING: shutdown restore', False);
    Result := False;
    Exit;
  end;
  if IsRunEntryFixture then begin
    Check(SetPebrelLaunchEnvironmentValue('__COMPAT_LAYER', 'HighDpiAware'), 'set Run entry layer');
    Check(SetPebrelLaunchEnvironmentValue('PEBREL_LAUNCH_FIXTURE_REPORT', Root + '\child.txt'), 'set Run entry report');
    if FileExists(Root + '\child.txt') then
      Check(DeleteFile(Root + '\child.txt'), 'remove stale child report');
    Result := True;
    Exit;
  end;
  try
    Check(SetPebrelLaunchEnvironmentValue('__COMPAT_LAYER', 'HighDpiAware'), 'set inherited layer');
    OriginalChild := ChildReport(Root, '--gpui');
    Check(Pos('v1=1', OriginalChild) > 0, 'old launch reproduces per-monitor V1');

    if ExpandConstant('{param:oldbehavior|0}') <> '1' then begin
      Parameters := PebrelLaunchParameters('');
      Check(Parameters = '--gpui', 'preserve GPUI arguments');
      Check(GetEnv('__COMPAT_LAYER') = '', 'remove inherited layer');
      Check(PebrelLaunchParameters('') = '--gpui', 'repeated expansion retains saved layer');
    end else
      Parameters := '--gpui';
    try
      CleanChild := ChildReport(Root, Parameters);
      Check(Pos('v2=1', CleanChild) > 0, 'clean launch restores per-monitor V2');
      Check(Pos('args=--gpui', CleanChild) > 0, 'child receives original arguments');
    finally
      RestorePebrelLaunchEnvironment;
    end;
    Check(GetEnv('__COMPAT_LAYER') = 'HighDpiAware', 'restore installer layer after success');

    Check(SetPebrelLaunchEnvironmentValue('__COMPAT_LAYER', 'HighDpiAware Win8RTM'), 'set combined layer');
    Parameters := PebrelLaunchParameters('');
    try
      Check(not Exec(Root + '\missing.exe', Parameters, Root, SW_HIDE,
        ewWaitUntilTerminated, ErrorCode), 'failed launch is observed');
    finally
      RestorePebrelLaunchEnvironment;
    end;
    Check(GetEnv('__COMPAT_LAYER') = 'HighDpiAware Win8RTM', 'restore exact layer after failure');

    Check(RemovePebrelLaunchEnvironmentValue('__COMPAT_LAYER', 0), 'unset layer');
    Check(PebrelLaunchParameters('') = '--gpui', 'ordinary launch keeps arguments');
    RestorePebrelLaunchEnvironment;
    Check(GetEnv('__COMPAT_LAYER') = '', 'ordinary launch remains without a layer');
    SaveStringToFile(Root + '\result.txt', 'PASS: ' + IntToStr(Checks) + ' checks', False);
  except
    SaveStringToFile(Root + '\result.txt', 'FAIL: ' + GetExceptionMessage, False);
  end;
  RestorePebrelLaunchEnvironment;
  if OriginalLayer = '' then
    RemovePebrelLaunchEnvironmentValue('__COMPAT_LAYER', 0)
  else
    SetPebrelLaunchEnvironmentValue('__COMPAT_LAYER', OriginalLayer);
  { Exit before installation, registry, shortcut or PATH actions. }
  Result := False;
end;

<event('DeinitializeSetup')>
procedure VerifyLaunchEnvironmentOnExit;
begin
  if ExpandConstant('{param:deinit|0}') = '1' then begin
    if GetEnv('__COMPAT_LAYER') = 'HighDpiAware' then
      SaveStringToFile(ExpandConstant('{#FixtureRoot}\result.txt'), 'PASS: shutdown restore', False)
    else
      SaveStringToFile(ExpandConstant('{#FixtureRoot}\result.txt'), 'FAIL: shutdown restore', False);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if IsRunEntryFixture and (CurStep = ssDone) then begin
    if GetEnv('__COMPAT_LAYER') = 'HighDpiAware' then
      SaveStringToFile(ExpandConstant('{#FixtureRoot}\result.txt'), 'PASS: Run entry restored layer', False)
    else
      SaveStringToFile(ExpandConstant('{#FixtureRoot}\result.txt'), 'FAIL: Run entry lost layer', False);
  end;
end;
