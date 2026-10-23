# ADR 0014 — Instalador para Windows: Inno Setup, runtime do VC++ junto do app

Status: aceito (Fase 6, bloco 6.6). Complementa o ADR 0013 (Linux).

## Contexto

O princípio de produto (spec 1.1, ADR 0008) é instalar e usar sem terminal, sem compilar e sem
instalar dependências à mão. No Windows isso inclui não exigir o Visual C++ Redistributable.

## Decisões

1. **Inno Setup 6** gera `Enxame-Setup-<versão>-x64.exe` (`packaging/windows/enxame.iss`), pela CI
   (`release.yml`, job Windows). Instala **por usuário, sem administrador** (`PrivilegesRequired=lowest`),
   com atalho no menu Iniciar, ícone opcional na área de trabalho, português e inglês. Os dados
   (`%APPDATA%\enxame`) não são apagados ao desinstalar: reinstalar mantém a identidade.
2. **Pré-lançamentos** saem por `workflow_dispatch` com `prerelease_tag` (ex.: `v0.0.1-rc4`); a
   release oficial vem de uma tag `vX.Y.Z` (a CI confere tag × versão do `Cargo.toml`).
3. **O runtime do Visual C++ vai dentro do pacote** (DLLs ao lado do `.exe`), escolhido pela **maior
   versão** de `msvcp140.dll` do runner. O WebRTC é compilado com um VC++ recente: com o 14.29 (de um
   Visual Studio 2019 do runner) o app caía com `0xc0000005` ao iniciar qualquer chamada. Confirmado
   no campo: com o 14.44 voz, vídeo e tela funcionam.
4. **Artefatos sem assinatura** por enquanto: o SmartScreen avisa na primeira execução. Assinar
   (certificado de código) fica como pendência, com custo e processo próprios.
5. **Ambiente de desenvolvimento no Windows** (`docs/roteiros/windows-dev.md`): `flutter run -d windows`
   é muito mais rápido que esperar a CI; o `windows-dev.yml` (zip de teste) não ganhou tempo com cache.

## Consequências

- Quedas nativas no Windows aparecem no Visualizador de Eventos (Application, Id 1000): o módulo com
  falha indica a DLL. Isso achou três defeitos reais (ver `docs/reports/fase6-windows.md`).
- Instalar por usuário evita o UAC, mas o instalador não registra nada em `Program Files` compartilhado.
