# 0008 — Simplicidade de setup e relay padrão

Status: aceito

## Contexto

O objetivo do produto é que uma pessoa comum instale o aplicativo e passe a usar, sem terminal,
Docker, variáveis de ambiente ou servidor. Durante os testes de campo das Fases 1–4 apareceram
dois pontos em que o estado atual exige conhecimento técnico: a compilação com Rust (só existe
CLI) e a falha de DNS (roteador e MagicDNS do Tailscale descartando consultas com EDNS) que deixa
o relay inalcançável e exige `ENXAME_DNS_SERVERS`. Também ficou a dúvida de quem opera o relay.

## Decisão

1. **Setup simples é requisito de produto** (princípio na seção 1.1 da spec) e passa a ser parte do
   aceite da Fase 6: instalar, abrir, adicionar contato e conversar sem terminal nem variáveis.
2. **Relay padrão = os relays públicos do iroh.** Zero configuração para o usuário básico. Relay
   próprio (`deploy/`, `relay = "https://..."`, CA própria) é **opcional**, para quem quer
   garantia de disponibilidade ou independência — por exemplo quem roda um nó 24/7. Docker e
   `enxame-node` existem só para esse caso; nenhum usuário básico precisa deles.
3. **DNS**: a correção definitiva (detectar que o resolvedor do sistema não responde e usar
   servidores públicos automaticamente, com o motivo visível no `diag`) é item de aceite da Fase 6.
   Até lá existe `dns_servers` / `ENXAME_DNS_SERVERS`, usado nos testes de campo.
4. **Distribuição**: AppImage e instalador do Windows (Fase 6) e, antes disso, binários prontos pela
   CI (GitHub Releases) para quem usa a CLI.
5. As configurações avançadas (relay próprio, DNS fixo, papel, budget) ficam fora do caminho do
   onboarding.

## Consequências

- Dependemos da disponibilidade e das condições dos relays públicos do n0 enquanto não houver um
  relay do projeto; a documentação do iroh os trata como infraestrutura de desenvolvimento, sem
  garantia. Reabrir esta decisão se isso mudar ou se o projeto decidir operar um relay oficial.
- Quem quer garantia roda o próprio relay e os contatos apontam para ele; isso não afeta quem
  usa o padrão.
- Os testes de campo seguem usando variáveis de ambiente até a Fase 6 entregar o fluxo simples.
