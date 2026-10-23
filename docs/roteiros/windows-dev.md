# Ambiente de desenvolvimento no Windows (rodar o app direto, sem esperar a CI)

Objetivo: no PC Windows, rodar `flutter run -d windows` com recarga a quente e ver os logs no
terminal. Só a primeira compilação é demorada (o núcleo em Rust); as seguintes são incrementais.

Tudo no PowerShell. Marque com ✅ cada passo concluído e pare no primeiro erro, mostrando a saída.

## 1. Pré-requisitos (uma vez)

1. **Modo de desenvolvedor** (os plugins do Flutter precisam de links simbólicos):
   `start ms-settings:developers` e ligar "Modo de Desenvolvedor".
2. **Git**: `winget install --id Git.Git -e`
3. **Visual Studio 2022 Build Tools com C++** (compilador, CMake e Windows SDK):
   `winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`
   (leva um tempo e pode pedir reinício).
4. **Runtime do VC++ atualizado** (um `msvcp140.dll` antigo, 14.29, derruba o WebRTC):
   `winget install --id Microsoft.VCRedist.2015+.x64 -e`
5. **Rust**: `winget install --id Rustlang.Rustup -e`; depois abrir um **novo** PowerShell e rodar
   `rustup default stable`.
6. **Flutter 3.47.7** (versão fixada do projeto):
   `git clone https://github.com/flutter/flutter.git -b 3.47.7 C:\dev\flutter`
   e acrescentar `C:\dev\flutter\bin` ao PATH do usuário:
   `[Environment]::SetEnvironmentVariable("Path", $env:Path + ";C:\dev\flutter\bin", "User")`
   (abrir um novo PowerShell depois).
7. Conferir: `flutter doctor -v`. Precisa estar ✅ em Flutter, Windows e Visual Studio (Chrome e
   Android não importam). Se a Visual Studio reclamar de componentes, instalar o que ele pedir.

## 2. Código

```powershell
mkdir C:\dev -ErrorAction SilentlyContinue
cd C:\dev
git clone https://github.com/AndreDev21/enxame.git
cd enxame\app
flutter pub get
```

Atualizar depois: `git pull` na raiz do repositório.

## 3. Rodar

```powershell
cd C:\dev\enxame\app
flutter run -d windows
```

- A primeira compilação demora (Rust + libwebrtc); as próximas são rápidas.
- No terminal do `flutter run`: `r` recarrega a quente, `R` reinicia, `q` sai. Os logs de chamada
  (`[call ...]`) aparecem ali e também em `%APPDATA%\enxame\calls.log`.
- Para usar uma identidade separada da instalada (sem mexer nos contatos do instalador):
  `$env:ENXAME_DATA_DIR = "C:\dev\enxame-dados"` antes do `flutter run`.
- Se der erro de DLL ou o app cair: `Get-WinEvent -FilterHashtable @{LogName='Application'; Id=1000} -MaxEvents 2 | Format-List TimeCreated, Message`
  (o "Nome do módulo com falha" diz qual DLL derrubou).

## 4. Fluxo de trabalho

1. No Linux, o André corrige e dá `git push`.
2. No Windows: `git pull` e `flutter run -d windows` (ou `R` no terminal se já estiver rodando e
   a mudança for só de Dart). Mudanças em Rust (`crates/`) exigem fechar e rodar de novo.
3. Teste a chamada, copie o final do `calls.log` e mande de volta.

Para testar o build final (como o instalador), `flutter build windows --release` gera em
`app\build\windows\x64\runner\Release\`.
