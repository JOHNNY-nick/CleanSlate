; CleanSlate installer (Inno Setup 6). Built by scripts/package.ps1, which passes
; AppVersion and PublishDir (output of a self-contained dotnet publish, spec § 10.4).

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef PublishDir
  #define PublishDir "..\artifacts\publish"
#endif

[Setup]
; Fixed AppId: lets later versions upgrade this installation in place.
AppId={{6F1C2E8A-3B4D-4E5F-9A0B-C1D2E3F4A5B6}
AppName=CleanSlate
AppVersion={#AppVersion}
AppVerName=CleanSlate {#AppVersion}
AppPublisher=CleanSlate
AppComments=Désinstallation complète et nettoyage des restes d'applications, avec quarantaine restaurable.
DefaultDirName={autopf}\CleanSlate
DefaultGroupName=CleanSlate
DisableProgramGroupPage=yes
; The application itself requires administrator rights (app.manifest).
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
SetupIconFile=..\assets\branding\cleanslate.ico
UninstallDisplayIcon={app}\CleanSlate.App.exe
UninstallDisplayName=CleanSlate
LicenseFile=..\LICENSE
OutputDir=..\artifacts\installer
OutputBaseFilename=CleanSlate-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#PublishDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\CleanSlate"; Filename: "{app}\CleanSlate.App.exe"
Name: "{autodesktop}\CleanSlate"; Filename: "{app}\CleanSlate.App.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\CleanSlate.App.exe"; Description: "{cm:LaunchProgram,CleanSlate}"; Flags: nowait postinstall skipifsilent shellexec

; The quarantine, history, settings and logs (%LOCALAPPDATA%\CleanSlate) are kept
; on uninstall: they may hold files the user still wants to restore.
