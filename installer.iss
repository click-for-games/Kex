[Setup]
AppName=Kex Programming Language
AppVersion=1.0.0
DefaultDirName={autopf}\KexEngine
DefaultGroupName=Kex Language
OutputBaseFilename=KexSetup
Compression=lzma
SolidCompression=yes
PrivilegesRequired=admin

[Files]
; Core Executable
Source: "kex\target\release\kex.exe"; DestDir: "{app}"; Flags: ignoreversion
; VS Code Extension Package
Source: "vscode-extension\kex-lang-1.0.0.vsix"; DestDir: "{app}"; Flags: ignoreversion

[Tasks]
Name: "addtopath"; Description: "Add Kex to System PATH (Required to run 'kex' anywhere)"; Flags: checkedonce

[Code]
// Helper to modify System PATH environment variable and install extension
procedure CurStepChanged(CurStep: TSetupStep);
var
  Path: String;
  ResultCode: Integer;
begin
  if (CurStep = ssPostInstall) then
  begin
    // 1. Add Install Dir to System PATH
    if IsTaskSelected('addtopath') then
    begin
      if RegQueryStringValue(HKEY_LOCAL_MACHINE, 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment', 'Path', Path) then
      begin
        if Pos(ExpandConstant('{app}'), Path) = 0 then
        begin
          Path := Path + ';' + ExpandConstant('{app}');
          RegWriteStringValue(HKEY_LOCAL_MACHINE, 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment', 'Path', Path);
        end;
      end;
    end;

    // 2. Install VS Code Extension automatically if VS Code CLI is available
    Exec('cmd.exe', '/c code --install-extension "' + ExpandConstant('{app}\kex-lang-1.0.0.vsix') + '" --force', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  end;
end;