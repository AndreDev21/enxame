# ADR 0010 — DNS automático com fallback para servidores públicos

Status: aceito (Fase 6, item de aceite do princípio "setup simples"; ADR 0008).

## Contexto

Em campo (Fase 3, duas vezes) o resolvedor do sistema respondia mal às consultas com EDNS: o
MagicDNS do Tailscale e o roteador de um usuário. O relay ficava inalcançável sem nenhum erro útil e
a saída era o usuário definir `ENXAME_DNS_SERVERS=1.1.1.1,8.8.8.8`. O usuário básico não pode ter de
saber isso.

O `DnsResolver::new()` do iroh só recorre a servidores públicos quando a configuração do sistema não
pode ser lida ou os servidores "não respondem". Nos casos acima o servidor **responde** (com erro ou
vazio), então o fallback do iroh não dispara.

## Decisão

Na subida (`Enxame::start`), antes de criar o endpoint:

1. `dns_servers` definido ⇒ vale (`DnsChoice::Manual`).
2. Sem relay (`relay = disabled`) ou com relay por IP ⇒ nada a testar (`System`).
3. Caso contrário, testa se o **resolvedor do sistema** (o mesmo `DnsResolver::new()` que o endpoint
   usaria) resolve o host do relay (`use1-1.relay.n0.iroh.link` ou o host do relay próprio), com
   timeout de 4 s. Em paralelo testa **1.1.1.1 e 8.8.8.8**.
   - Sistema resolve ⇒ `System` (não espera o teste público).
   - Sistema falha e o público resolve ⇒ `Fallback`: o endpoint usa 1.1.1.1/8.8.8.8 e o motivo vai
     para o log e para o `diag` ("DNS: automático, servidores públicos (...)").
   - Os dois falham ⇒ `System` (sem rede; nada a corrigir).

Sem rede, o custo é um só timeout de 4 s na subida; com DNS saudável, a latência é a de uma consulta.

## Consequências e limites

- Quem usa o app não define variável alguma. `dns_servers` continua existindo para quem quiser
  servidores próprios (Pi-hole, DoT na rede) e tem prioridade.
- Os servidores públicos veem os nomes dos relays e do discovery do iroh (não o conteúdo). Quem não
  aceita isso define `dns_servers` ou desliga o fallback (sem opção dedicada ainda; fica para a tela
  de configurações avançadas).
- Só testamos o relay: um DNS que falha só para outros nomes (ex.: `dns.iroh.link`) não é detectado.
- O fallback é decidido uma vez por subida; trocar de rede com o app aberto não reavalia.
  Reavaliar em mudança de rede fica para o monitor de rede da Fase 6.

## Validação

Cobertura da lógica por testes (`net::tests`). Em campo, nesta máquina (resolvedor do Tailscale), o
`diag` sem `ENXAME_DNS_SERVERS` detectou o problema, usou o fallback e alcançou um contato pelo relay.
