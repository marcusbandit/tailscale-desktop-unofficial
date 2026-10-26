; Per-user installer: no admin prompt, a Start menu entry and an uninstaller.
; Built by the release workflow: iscc /DVersion=0.1.0 /DSource=path\to\exe installer.iss

#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId={{7D8C8BA7-DA0C-4614-BB2D-F51FE63B5EEB}
AppName=Tailscale Desktop (unofficial)
AppVersion={#Version}
AppPublisher=Marcus Rosado
AppPublisherURL=https://github.com/marcusbandit/tailscale-desktop-unofficial
AppSupportURL=https://github.com/marcusbandit/tailscale-desktop-unofficial/issues
DefaultDirName={localappdata}\Programs\Tailscale Desktop (unofficial)
DefaultGroupName=Tailscale Desktop (unofficial)
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputBaseFilename=tailscale-desktop-unofficial-{#Version}-x86_64-windows-setup
OutputDir=.
SetupIconFile=icon.ico
UninstallDisplayIcon={app}\tailscale-desktop-unofficial.exe
LicenseFile=..\..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Files]
Source: "{#Source}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\TRADEMARKS.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Tailscale Desktop (unofficial)"; Filename: "{app}\tailscale-desktop-unofficial.exe"
Name: "{autodesktop}\Tailscale Desktop (unofficial)"; Filename: "{app}\tailscale-desktop-unofficial.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Run]
Filename: "{app}\tailscale-desktop-unofficial.exe"; Description: "Open Tailscale Desktop (unofficial)"; Flags: nowait postinstall skipifsilent
