# Deploy: relay + nó headless

Sobe, num VPS ou homelab, o **iroh-relay** próprio (fallback de conexão, mesmo papel do hbbs/hbbr
do RustDesk) e o **enxame-node**, um SuperPeer 24/7 que guarda mensagens offline dos seus contatos.

## Requisitos

- Docker com Compose v2.
- Um nome DNS apontando para o servidor e as portas **80/tcp, 443/tcp e 7842/udp** liberadas
  (80 e 443 para o certificado e o relay; 7842/udp para descoberta de endereço QUIC).

## Passo a passo

```bash
cd deploy
cp .env.example .env     # edite RELAY_HOSTNAME, ACME_EMAIL e ALLOWED_FINGERPRINTS
docker compose up -d --build
docker compose ps        # relay e node devem ficar "healthy"
docker compose logs node | head
```

O nó escreve o ticket em `/data/node-ticket.txt` (renovado a cada hora; vale 24 h):

```bash
docker compose exec node cat /data/node-ticket.txt
```

Em cada cliente que deve usar o nó:

```bash
# 1. a fingerprint do cliente (enxame-cli id) vai em ALLOWED_FINGERPRINTS no .env do servidor
#    (depois: docker compose up -d para recriar o nó);
# 2. o cliente aponta para o relay e adiciona o nó:
ENXAME_RELAY=https://relay.example.org enxame-cli add '<ticket do nó>'
```

O nó **não pergunta nada**: aceita só as fingerprints listadas e recusa (com aviso no log) as
demais. Depois de contato mútuo, ele passa a ser carrier preferido (SuperPeer) entre vocês.

## Certificado manual

`RELAY_CERT_MODE=Manual` usa `./certs/relay.crt` e `./certs/relay.key` (monte seus arquivos
nesse diretório). Clientes com CA privada configuram `relay_ca_file`/`ENXAME_RELAY_CA_FILE`.

## Operação

- **Dados:** volume `node-data` (`identity.key`, `enxame.db`). Faça backup do volume: a chave é a
  identidade do nó.
- **Saúde:** o nó atualiza `/data/health` a cada 10 s; o healthcheck falha se ficar 60 s sem sinal.
- **Limites:** CPU e memória em `docker-compose.yml`; banda e volume mensal por
  `MAX_CONTRIB_KBPS` e `MAX_CONTRIB_GB_MONTH`; guarda de terceiros por `CARRY_QUOTA_MB_TOTAL`.
- **Atualizar:** `git pull && docker compose up -d --build`.
- **Logs:** `docker compose logs -f node` (nunca contêm texto de mensagem nem chaves).

## Verificar a imagem do nó

Com a imagem construída (`docker build -f deploy/Dockerfile.node -t enxame/node:local .`) e o CLI
compilado (`cargo build --release -p enxame-cli`), o script abaixo sobe o contêiner em localhost e
confere ponta a ponta: A manda texto e um arquivo a B (offline), o contêiner guarda o aviso e
replica o arquivo, A sai da rede e B baixa o arquivo **só do contêiner** (hash confere).

```bash
scripts/e2e/node-image.sh
```

O healthcheck do Docker (`enxame-node health`) só olha a idade do arquivo `/data/health`, que tem
o resumo (`role`, `contacts`, `carry_items`, `bytes_served_month`): `docker exec <contêiner> cat /data/health`.
Em máquinas com pouca RAM, limite a compilação: `docker build --build-arg CARGO_BUILD_JOBS=2 ...`.
