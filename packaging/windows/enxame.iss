; Instalador do Enxame para Windows (Inno Setup 6). Instala por usuário, sem administrador.
; Uso: iscc /DBundleDir=<pasta do build\windows\x64\runner\Release> /DAppVersion=0.0.1 enxame.iss
; Saída: dist\Enxame-Setup-<versão>-x64.exe

#ifndef AppVersion
  #define AppVersion "0.0.1"
#endif
#ifndef BundleDir
  #define BundleDir "..\..\app\build\windows\x64\runner\Release"
#endif

[Setup]
AppId={{6E7A2B1F-4C3D-4B8E-9F10-2D5A7C9E1B34}
AppName=Enxame
AppVersion={#AppVersion}
AppPublisher=Enxame
DefaultDirName={autopf}\Enxame
DefaultGroupName=Enxame
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile=enxame.ico
UninstallDisplayIcon={app}\enxame_app.exe
OutputDir=..\..\dist
OutputBaseFilename=Enxame-Setup-{#AppVersion}-x64
DisableProgramGroupPage=yes

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BundleDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{autoprograms}\Enxame"; Filename: "{app}\enxame_app.exe"
Name: "{autodesktop}\Enxame"; Filename: "{app}\enxame_app.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\enxame_app.exe"; Description: "{cm:LaunchProgram,Enxame}"; Flags: nowait postinstall skipifsilent

; Os dados do usuário (identidade, mensagens) ficam em %APPDATA%\enxame e NÃO são apagados ao
; desinstalar: reinstalar mantém a identidade. Para apagar tudo, remova essa pasta.
