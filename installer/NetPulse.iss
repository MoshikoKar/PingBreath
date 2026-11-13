; Inno Setup script. Build via build.ps1 (passes /DAppVersion=x.y.z).
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F1D2B8E-4C7A-4E55-9A31-0A5E7B1C2D03}
AppName=NetPulse
AppVersion={#AppVersion}
AppPublisher=NetPulse
DefaultDirName={autopf}\NetPulse
DefaultGroupName=NetPulse
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=NetPulse-Setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\NetPulse.exe
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible

[Tasks]
Name: "startup"; Description: "Start NetPulse when I sign in to Windows"; Flags: checkedonce

[Files]
Source: "..\dist\NetPulse.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\NetPulse"; Filename: "{app}\NetPulse.exe"
Name: "{userstartup}\NetPulse"; Filename: "{app}\NetPulse.exe"; Tasks: startup

[Run]
; Not skipifsilent: after a silent self-update the app comes back up on its own.
Filename: "{app}\NetPulse.exe"; Description: "Launch NetPulse"; Flags: nowait postinstall runasoriginaluser
