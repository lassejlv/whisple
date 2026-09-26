; Inno Setup installer for Whisple. Built by scripts/windows/package-windows.ps1,
; which passes the version, the executable and the output location:
;
;   ISCC /DVersion=0.2.1 /DNumericVersion=0.2.1.0 /DSourceExe=... /DOutputDir=... /DOutputBase=... whisple.iss
;
; Whisple installs per user into %LOCALAPPDATA%\Programs\Whisple without
; administrator rights. The updater expects exactly that folder, so the
; directory page is hidden.

#ifndef Version
  #error Pass /DVersion=<semantic version>
#endif

[Setup]
AppId={{5E0F9D9B-7C1A-4E5B-9E54-3A0C6B3F2D71}
AppName=Whisple
AppVersion={#Version}
AppVerName=Whisple {#Version}
AppPublisher=Whisple
AppPublisherURL=https://whisple.app
AppSupportURL=https://github.com/lassejlv/whisple/issues
AppUpdatesURL=https://github.com/lassejlv/whisple/releases
VersionInfoVersion={#NumericVersion}
VersionInfoProductVersion={#NumericVersion}
VersionInfoProductTextVersion={#Version}
PrivilegesRequired=lowest
DefaultDirName={userpf}\Whisple
DisableDirPage=yes
DisableProgramGroupPage=yes
UsePreviousAppDir=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
SetupIconFile=..\..\assets\whisple.ico
UninstallDisplayIcon={app}\whisple.exe
UninstallDisplayName=Whisple
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBase}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "danish"; MessagesFile: "compiler:Languages\Danish.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "norwegian"; MessagesFile: "compiler:Languages\Norwegian.isl"
Name: "swedish"; MessagesFile: "compiler:Languages\Swedish.isl"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "whisple.exe"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Whisple"; Filename: "{app}\whisple.exe"

[Run]
Filename: "{app}\whisple.exe"; Description: "{cm:LaunchProgram,Whisple}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; The updater keeps the previous executable here until an update finishes.
Type: files; Name: "{app}\whisple.exe.previous"

[Code]
// "Open at login" writes this value. Remove it with the app so Windows does
// not try to start a program that is gone.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', 'Whisple');
end;
