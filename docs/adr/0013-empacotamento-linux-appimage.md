# ADR 0013 — Empacotamento para Linux: AppImage e tarball, build em Ubuntu 22.04

Status: aceito (Fase 6, bloco 6.5). Responde à decisão aberta 7 da spec para o Linux (o Windows vem
no bloco 6.6).

## Contexto

O princípio de produto (spec 1.1, ADR 0008) é instalar e usar **sem terminal e sem compilar nada**.
Em campo, para testar chamadas, o bob precisou instalar à mão um Flutter 3.47.7 ao lado do pacote do
Arch, que era antigo. Um usuário comum não faz isso.

## Decisões

1. **AppImage** para o app (`Enxame-<versão>-x86_64.AppImage`): um único arquivo, sem instalação nem
   root; o usuário baixa, marca como executável e abre. Foi escolhido em vez de Flatpak/Snap porque
   não exige loja, runtime nem portal para o que o app precisa (rede, câmera/microfone/tela, bandeja).
   Flatpak e pacote AUR ficam como possibilidades futuras (decisão aberta 7 continua para elas).
2. **Tarball do CLI e do nó** (`enxame-cli-<versão>-x86_64-linux.tar.gz`, com `enxame-cli`,
   `enxame-node`, licença e README) para quem prefere a linha de comando ou roda um nó 24/7.
3. **Build em Ubuntu 22.04 dentro de um contêiner** (`packaging/linux/Dockerfile.build`, glibc 2.35):
   o binário roda em distribuições dessa idade ou mais novas. Um bundle compilado num Arch atual
   exigiria uma glibc muito nova e não abriria em distros estáveis. O mesmo script
   (`scripts/package/build-linux-release.sh`) roda na máquina de quem desenvolve e na CI; só precisa
   do Docker (sem BuildKit).
4. **O AppImage depende do sistema** para GTK 3, D-Bus e PulseAudio (todo desktop com GTK os tem) e
   leva consigo o runtime do Flutter, o WebRTC e o núcleo Rust. Não empacotamos o GTK: deixaria o
   arquivo muito maior e quebraria temas e portais. Requisitos: GTK 3.24, libdbus-1, libpulse.
5. **Cadeia de suprimentos:** o `appimagetool` (1.9.1) e o runtime do AppImage (tag 20251108) são
   baixados com **versão fixada e SHA-256 verificado** (`scripts/package/appimage.sh`); o Flutter é
   clonado pela tag `3.47.7`. Cada artefato sai com `.sha256`.
6. **Autostart** dentro do AppImage registra o caminho do arquivo `.AppImage` (`$APPIMAGE`), não o do
   ponto de montagem, que muda a cada execução.
7. **Releases:** ao criar uma tag `vX.Y.Z` igual à versão do workspace, `.github/workflows/release.yml`
   gera os artefatos e publica no GitHub Releases com `gh release create` (sem ação de terceiros).
   Sem a tag, o workflow manual só gera os artefatos.

## Consequências e limites

- Não há atualização automática (o usuário baixa o novo arquivo); `AppImageUpdate`/zsync ficam para
  depois, junto com assinatura dos artefatos.
- Os artefatos **não são assinados** (só `.sha256`). O SHA-256 protege contra download corrompido, não
  contra quem controla o repositório.
- Só x86_64. ARM e macOS não têm build.
- O aviso "sem host de bandeja" depende do desktop: no GNOME é preciso a extensão AppIndicator; sem ela,
  fechar a janela encerra o app (ADR 0011).
