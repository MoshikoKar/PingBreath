; Inno Setup script. Build via build.ps1 (passes /DAppVersion=x.y.z).
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; AppId GUID must stay unchanged across renames so upgrades replace the prior install in place.
AppId={{6F1D2B8E-4C7A-4E55-9A31-0A5E7B1C2D03}
AppName=PingBreath
AppVersion={#AppVersion}
AppPublisher=PingBreath
DefaultDirName={autopf}\PingBreath
DefaultGroupName=PingBreath
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=PingBreath-Setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\PingBreath.exe
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible

[Tasks]
Name: "startup"; Description: "Start PingBreath when I sign in to Windows"; Flags: checkedonce

[Files]
Source: "..\dist\PingBreath.exe"; DestDir: "{app}"; Flags: ignoreversion

; LEGACY(NetPulse): remove in v0.3.1 - clean up old exe and Start Menu entry on upgrade.
[InstallDelete]
Type: files; Name: "{app}\NetPulse.exe"
Type: files; Name: "{userprograms}\NetPulse\NetPulse.lnk"
Type: dirifempty; Name: "{userprograms}\NetPulse"

[Icons]
Name: "{group}\PingBreath"; Filename: "{app}\PingBreath.exe"

[Registry]
; Same Run-key value the in-app "Start with Windows" toggle reads and writes.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "PingBreath"; ValueData: """{app}\PingBreath.exe"""; Tasks: startup; Flags: uninsdeletevalue
; LEGACY(NetPulse): remove in v0.3.1 - drop the old Run value so an upgraded install has no dead entry.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "NetPulse"; Flags: deletevalue noerror

[Run]
; Not skipifsilent: after a silent self-update the app comes back up on its own.
Filename: "{app}\PingBreath.exe"; Description: "Launch PingBreath"; Flags: nowait postinstall runasoriginaluser
