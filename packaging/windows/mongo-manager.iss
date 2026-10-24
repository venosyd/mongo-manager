; Instalador do Windows (Inno Setup 6). Compilar com:
;   iscc /DAppVersion=0.1.0 packaging\windows\mongo-manager.iss
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{8C1E6C1A-4D3B-4F5E-9C2A-6B7D8E9F0A1B}
AppName=Mongo Manager
AppVersion={#AppVersion}
AppPublisher=Venosyd
AppPublisherURL=https://github.com/venosyd/mongo-manager
DefaultDirName={autopf}\Mongo Manager
DefaultGroupName=Mongo Manager
DisableProgramGroupPage=yes
OutputDir=..\..\dist
OutputBaseFilename=mongo-manager-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\mongo-manager.exe
LicenseFile=..\..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequiredOverridesAllowed=dialog

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\x86_64-pc-windows-msvc\release\mongo-manager.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Mongo Manager"; Filename: "{app}\mongo-manager.exe"
Name: "{autodesktop}\Mongo Manager"; Filename: "{app}\mongo-manager.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\mongo-manager.exe"; Description: "{cm:LaunchProgram,Mongo Manager}"; Flags: nowait postinstall skipifsilent
