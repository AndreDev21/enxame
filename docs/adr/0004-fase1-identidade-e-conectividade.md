# 0004 — Decisões da Fase 1 (identidade e conectividade)

Status: aceito

## Contexto

A Fase 1 usa `iroh` 1.4.1 (ADR 0002). Algumas decisões não estavam na spec ou divergem dela por
mudanças de API.

## Decisões

1. **Nomes do iroh 1.x.** A spec fala `NodeId`/`NodeAddr`; no iroh 1.x são `EndpointId` e
   `EndpointAddr`. O código usa os nomes novos; o campo do ticket continua `node_addr`.
2. **Classificação de NAT** vem do `net_report` do iroh, que está atrás do feature
   `unstable-net-report` (sem garantia de semver). Fica isolado em `net.rs`. Regras: sem UDP →
   `Unknown`; `mapping_varies_by_dest` verdadeiro → `Symmetric`; falso e IP público igual a um IP
   local → `Open`; falso → `Cone`. **Com um só relay o iroh não compara destinos** e o resultado é
   `Unknown`; configure dois ou mais relays (`relay = "https://a,https://b"`) para distinguir.
3. **Contatos em arquivo** (`contacts.bin`, `postcard` versionado, escrita atômica) até o SQLite
   chegar na Fase 2, que migra esses dados.
4. **Fluxo de contato** em ALPN próprio (`enxame/contact/1`), separado do `sync`, para que o
   único tráfego aceito de desconhecidos seja o pedido (ver `docs/protocol.md`). O consentimento
   do lado que gerou o ticket é explícito (evento + `respond_contact`), exceto quando esse lado
   já havia pedido contato ao mesmo dispositivo.
5. **`Role` no `Hello` é `Leaf`** até a Fase 3 (valor conservador).
6. **`relay_ca_file`**: relays self-hosted com CA privada exigem confiar nela; a opção substitui as
   raízes públicas só para o endpoint configurado. Não existe opção de pular a verificação TLS.
7. **`dns_discovery`** (padrão ligado): com `false` o endpoint não publica nem resolve no DNS do
   n0, para redes isoladas e para o netsim. Relay próprio sem DNS próprio ainda depende dos
   endereços do ticket.
8. **Keystore do SO atrás do feature `os-keyring`** (ligado na CLI e no nó), para o core compilar
   no Android sem arrastar dependências de desktop (spec 17.1).
9. **Netsim sem root**: `unshare --user --map-root-user --net --mount` + `ip netns` + nftables.
   O "roteador doméstico" descarta tráfego novo da WAN (`ct state new drop` no `input`), sem o que o
   conntrack do Linux rouba o mapeamento de porta e impede o hole punching (relatório da fase 1).
10. **mDNS** usa `iroh-mdns-address-lookup` 0.6.1 (compatível com `iroh ^1.0.0`). Ligado por
    padrão; a validação em LAN fica para a Fase 6.

## Consequências

- Atualizar o iroh exige rever `net.rs` (net_report, paths) e o netsim.
- Os testes de integração rodam sem internet: relay local do `iroh::test_utils` e nós só-IP.
