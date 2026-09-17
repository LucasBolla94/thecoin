# Operação de nós — The Coin

Guia para quem roda `thecoind`: participantes que mineram numa VPS, operadores
dos nós seed (`seed1`/`seed2.the-coin.cloud`) e quem hospeda o site.

## Seja um validador em 1 minuto

Em qualquer VPS Linux com systemd (x86_64 ou ARM64), conectado por SSH, **um único comando**:

```bash
curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes
```

* `curl -fsSL …/install.sh` baixa o instalador (`installer/install.sh`);
* `sudo bash -s --` executa como root lendo o script da entrada padrão e repassa as opções seguintes;
* `--yes` = modo não interativo: nenhuma pergunta, aceita os padrões (mainnet, minerando, carteira criada automaticamente).

Outras opções vêm depois de `--yes` (ex.: `--network testnet`, `--miner-address tc1…`,
`--no-mine`, `--threads 2`; lista completa na §2.1). Ao final:

- o nó `thecoind` está rodando como serviço, sincronizando e **minerando**;
- existe uma carteira de recompensas em `~/.thecoin/wallet-mainnet.json`, protegida por uma senha
  aleatória forte guardada em `~/.thecoin/wallet-mainnet.password` (dono `root`, permissão `0600`);
- as **24 palavras de recuperação** aparecem num quadro amarelo — anote-as no papel.
  Para vê-las de novo: `sudo thecoin mnemonic`;
- os binários `thecoind` (nó), `thecoin-wallet` (carteira) e `tccl` (ferramenta de
  contratos inteligentes) estão em `/usr/local/bin`;
- o comando `thecoin` está instalado:

| Comando | O que faz |
|---|---|
| `thecoin status` | serviço, versão, altura, sincronização, peers, mempool, hashrate, mineração e saldo do endereço de mineração (inclusive recompensas em cooldown) |
| `thecoin logs` | segue os logs do serviço (`journalctl -u thecoind -f`) |
| `thecoin balance` | saldo da carteira (sem sudo, usa a API do nó; com sudo, usa a carteira) |
| `thecoin address` | endereço de mineração/carteira |
| `sudo thecoin mnemonic` | mostra as palavras de recuperação |
| `sudo thecoin restart` | reinicia o nó |
| `thecoin signals` | propostas de governança que seus blocos apoiam e propostas abertas |
| `sudo thecoin signal <id>` | passa a apoiar a proposta `<id>` (grava em `[mining] signal` e reinicia o nó) |
| `sudo thecoin unsignal <id>` | deixa de apoiar a proposta |
| `sudo thecoin update` | reinstala a versão mais recente mantendo carteira e configuração |
| `sudo thecoin uninstall [--purge]` | remove o nó (a carteira nunca é apagada) |
| `thecoin version` | versões instaladas do nó e da carteira |
| `thecoin help` | ajuda |

Comandos que precisam de root se reexecutam sozinhos com `sudo`.

Checklist rápido depois de instalar:

1. **Porta P2P** (7333/TCP; 17333 na testnet) liberada no firewall do servidor **e** no painel do
   provedor (AWS/GCP/Azure/Oracle/OVH/Hetzner: *security group* ou firewall de nuvem). Com `ufw`
   ativo o instalador libera sozinho; com `ufw` inativo, rode `sudo ufw allow 7333/tcp` antes de ativá-lo.
2. **Relógio sincronizado**: `timedatectl show -p NTPSynchronized` deve dizer `yes`
   (senão: `sudo timedatectl set-ntp true`). Blocos com horário muito adiantado são rejeitados.
3. **Backup das 24 palavras** feito em papel.
4. `thecoin status` mostra `Service: active` e `Mining: on`.

Guia ilustrado para novos usuários: <https://the-coin.cloud/validator.html>.

## 1. Requisitos

| Recurso | Mínimo | Recomendado |
|---|---|---|
| CPU | 1 vCPU x86_64 ou ARM64 | 2 vCPU |
| RAM | 1 GB (+ swap de 1 GB) | 2 GB |
| Disco | 5 GB livres | 20 GB SSD |
| SO | Linux com systemd | Ubuntu/Debian LTS |
| Rede | porta TCP 7333 aberta para entrada | IP público fixo |
| Relógio | NTP ativo (`timedatectl`) | — |

> **A memória mudou depois da v0.2.0:** a prova de trabalho é **RandomX** (§1.1). Verificar blocos
> exige um cache de **256 MiB**, compartilhado por todas as threads — por isso o mínimo
> prático passou a ser ~1 GB de RAM e o recomendado 2 GB. Minerar em **modo rápido**
> (dataset de 2 GiB) só faz sentido em máquinas com ~3 GiB livres.

Consumo medido (VPS 2 vCPU Haswell, 3,7 GB RAM):

| Métrica | Valor |
|---|---|
| Memória do nó minerando com 1 thread (modo leve) | ≈ 280 MB RSS (256 MiB são o cache RandomX) |
| CoinHash (RandomX, modo leve) | ≈ 30 ms por hash/bloco |
| Verificação de assinaturas | ≈ 14 400 transações/s por núcleo |
| Aplicar bloco com 5 000 tx (790 kB) | 0,39 s + 62 ms de LtHash |
| Contratos TCCL no pior caso | bloco cheio (50 M de combustível) ≈ 1 s de CPU |
| Disco por bloco vazio (banco + índices) | ≈ 436 bytes (≈ 0,92 GB/ano com blocos de 15 s) |
| Disco por transação — nó arquivo **com** índice de endereços | ≈ 430 bytes |
| Disco por transação — nó arquivo **sem** índice de endereços | ≈ 348 bytes |
| Transferência simples (sem memo) | 159 bytes |

Projeções completas de disco (blocos, transações, poda) estão em
[ESCALA.md §4](ESCALA.md). Com a poda ligada por padrão (§10) um nó comum fica na
casa de poucos GB para sempre.

Os números de disco vêm de 100 000 transferências na regtest e o custo do
CoinHash do benchmark de prova de trabalho:

```bash
GROWTH_BLOCKS=500 cargo test -p thecoin-node --release --test storage_growth -- --ignored --nocapture
cargo test -p thecoin-core --release --test pow_bench -- --ignored --nocapture
```

*Dados* é o que o banco realmente usa (blocos comprimidos, estado, undo, índices
e recibos). O arquivo `chain.redb` ocupa mais que os dados: o redb cresce em
dobras e reserva espaço à frente. Esse excesso é recuperável com
`thecoind compact` (nó parado, §10).

### 1.1 Memória da prova de trabalho (RandomX)

O CoinHash é o **RandomX** com uma chave que gira a cada época
(`epoch = altura / 2048` na mainnet e na testnet, 64 na regtest; ver
[PROTOCOL.md](PROTOCOL.md)). A memória não é por thread — é compartilhada:

| Modo | Memória | Para quê |
|---|---|---|
| **Leve** (`light`) | cache de **256 MiB**, compartilhado por todas as threads | verificar blocos — é o que **todo** nó faz, inclusive VPS pequenas |
| **Rápido** (`fast`) | dataset de **2 GiB** | minerar bem mais rápido, só se sobrar memória |

* Todo nó que valida blocos aloca o cache de 256 MiB, mesmo sem minerar.
* O nó mantém **até dois caches** ao mesmo tempo (o da época atual e o anterior),
  para não reconstruir tudo na virada de época; nesse instante o pico chega a
  512 MiB. Construir um cache leva cerca de 1 s.
* Threads de mineração **não** multiplicam esse custo: todas usam o mesmo cache
  (ou o mesmo dataset).
* O modo é escolhido em `[mining] mode` (§4). Em `auto` (padrão) o nó usa o modo
  rápido só quando `MemAvailable` do `/proc/meminfo` for **≥ 3 072 MiB**; caso
  contrário fica no modo leve. Se o dataset não couber na memória, o nó cai
  sozinho para o modo leve e registra
  `not enough memory for the 2 GiB RandomX dataset: mining in light mode`.

Por que RandomX: ele executa um programa aleatório feito das operações em que a
CPU é boa, então uma placa de vídeo ganha pouco ou nada — mineração continua nos
computadores comuns. Ver [ESCALA.md §5](ESCALA.md).

## 2. Instalação

### 2.1 Instalador (recomendado)

```bash
curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes   # sem perguntas
curl -fsSL https://the-coin.cloud/install.sh | sudo bash               # interativo
```

O instalador (`installer/install.sh`):

1. **pré-verificações**: Linux + systemd, arquitetura, `curl`/`tar`/`sha256sum`, RAM, disco,
   sincronização do relógio (NTP) e situação do firewall;
2. oferece criar 1 GB de swap em máquinas com menos de 2 GB de RAM sem swap (aceito automaticamente com `--yes`);
3. baixa `thecoind` + `thecoin-wallet` + `tccl` de `https://the-coin.cloud/releases/` (ou do GitHub Releases),
   verificando o SHA-256 — ou compila do código-fonte se não houver binário;
4. cria o usuário de sistema `thecoin` e `/var/lib/thecoin`;
5. define o endereço de recompensa:
   - `--miner-address` informado → usa esse endereço;
   - reinstalação/atualização → mantém o endereço e o estado de mineração do `thecoind.toml` existente;
   - carteira do instalador já existente → lê o endereço (usando o arquivo de senha, se houver);
   - **modo não interativo** (`--yes` ou sem terminal) → cria a carteira automaticamente para o
     usuário que chamou o `sudo` (ou root), com senha aleatória de 32 bytes guardada em
     `~/.thecoin/wallet-<rede>.password` (dono `root`, `0600`), e mostra as palavras de recuperação no final;
   - **modo interativo** → menu: criar carteira com senha escolhida (padrão), criar automaticamente,
     informar um endereço ou não minerar;
6. grava `/etc/thecoin/thecoind.toml`, `/etc/thecoin/install.env` (onde está a carteira; sem segredos)
   e o serviço systemd `thecoind` (com *hardening*, `Nice=5`, `MemoryHigh` = 70 % da RAM);
7. instala o comando `thecoin` e o desinstalador em `/usr/local/lib/thecoin/uninstall.sh`;
8. abre a porta P2P no `ufw` (se ativo) e inicia (ou reinicia, numa atualização) o nó.

Rodar o instalador de novo **atualiza** os binários e mantém carteira, senha e configuração
(é o que `sudo thecoin update` faz).

Opções (também por variável de ambiente):

| Opção | Variável | Descrição |
|---|---|---|
| `--yes`, `-y` | `THECOIN_YES=1` | não interativo: nunca pergunta |
| `--network mainnet\|testnet` | `THECOIN_NETWORK` | rede (padrão mainnet) |
| `--miner-address tc1…` | `THECOIN_MINER_ADDRESS` | endereço de recompensa (também reativa a mineração numa reinstalação) |
| `--no-mine` | `THECOIN_NO_MINE=1` | só valida e retransmite (fica gravado na configuração) |
| `--threads N` | `THECOIN_THREADS` | threads de mineração (0 = núcleos − 1) |
| `--version x.y.z` | `THECOIN_VERSION` | versão específica |
| `--from-source` | `THECOIN_FROM_SOURCE=1` | compila localmente |
| `--archive` | `THECOIN_ARCHIVE=1` | **nó arquivo**: grava `prune = false` e guarda todo o histórico (necessário para exploradores e para servir histórico por endereço; §10) |
| `--public-api` | `THECOIN_PUBLIC_API=1` | API em `0.0.0.0` (só para seeds/explorador) |
| — | `THECOIN_RELEASES_URL` | outra origem dos binários (espelho/testes) |
| — | `THECOIN_SITE_URL` | outra origem do comando `thecoin` e do desinstalador |

Exemplos:

```bash
# minerar para um endereço existente
curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes --miner-address tc1seuendereco...
# nó seed que serve a API ao site (arquivo: guarda todo o histórico)
curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes --no-mine --public-api --archive
# sem acesso ao site
curl -fsSL https://raw.githubusercontent.com/LucasBolla94/thecoin/main/installer/install.sh | sudo bash -s -- --yes
```

Desinstalar (mantém dados; `--purge` apaga dados, configuração e o usuário `thecoin`;
os arquivos da carteira **nunca** são apagados):

```bash
sudo thecoin uninstall [--purge]
curl -fsSL https://the-coin.cloud/uninstall.sh | sudo bash      # alternativa
```

> **Segurança da senha automática:** no modo `--yes` a carteira fica cifrada com uma senha que está
> no próprio servidor (legível só pelo root). Isso protege contra acesso de outros usuários, mas não
> contra quem tem root na máquina. Para valores altos, anote as palavras e transfira as recompensas
> periodicamente para uma carteira que só você controla — ou instale no modo interativo escolhendo a
> senha.

### 2.2 Manual (código-fonte)

```bash
sudo apt install -y build-essential pkg-config git cmake g++   # cmake e g++: o RandomX é C++
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://github.com/LucasBolla94/thecoin && cd thecoin
cargo build --release -p thecoin-node -p thecoin-wallet
sudo install -m755 target/release/thecoind target/release/thecoin-wallet /usr/local/bin/
# opcional — ferramenta de contratos, do repositório da linguagem (tag fixada no Cargo.toml):
cargo install --locked --git https://github.com/LucasBolla94/tccl --tag v0.3.0 tccl-cli
thecoin-wallet create                         # anote a frase de recuperação!
thecoind --miner-address $(thecoin-wallet address)
```

Em máquinas com pouca RAM, compile com `CARGO_BUILD_JOBS=1` e swap ativo.

## 3. Linha de comando do `thecoind`

```
thecoind [OPÇÕES] [run|init|params|compact]
```

| Opção | Descrição |
|---|---|
| `-c, --config <arquivo>` | TOML (padrão: `<data-dir>/thecoind.toml` se existir) — env `THECOIN_CONFIG` |
| `--network <rede>` | `mainnet`, `testnet`, `regtest` — env `THECOIN_NETWORK` |
| `--data-dir <dir>` | env `THECOIN_DATA_DIR` |
| `--miner-address <addr>` | liga a mineração para esse endereço — env `THECOIN_MINER_ADDRESS` |
| `--threads <n>` | threads de mineração |
| `--no-mine` | desliga a mineração |
| `--signal <id>` | id (hex) de proposta de governança que os blocos minerados apoiam; repetível; soma-se a `[mining] signal` |
| `--listen <ip:porta>` | P2P |
| `--rpc <ip:porta>` | API |
| `--peer <host:porta>` | peer extra (repetível) |
| `--connect-only` | conecta **somente** aos `--peer` |
| `--prune` | poda corpos antigos de blocos — **redundante depois da v0.2.0**, a poda já vem ligada por padrão (§10) |
| `--log-level <nível>` | `error`…`trace` (padrão `info`) — env `THECOIN_LOG` |

> Não existe opção de linha de comando para **desligar** a poda: um nó arquivo
> precisa de `[storage] prune = false` no `thecoind.toml` (ou da instalação com
> `--archive`, §2.1).

| Subcomando | Função |
|---|---|
| `run` | roda o nó (padrão) |
| `init [--output arq] [--force]` | grava um arquivo de configuração padrão |
| `params` | mostra parâmetros da rede (gênese, emissão, portas) |
| `compact` | compacta o banco `chain.redb` para liberar espaço — **pare o nó antes** |

## 4. Referência do arquivo de configuração

Instalação via instalador: `/etc/thecoin/thecoind.toml`. Gerar um modelo:
`thecoind init --output thecoind.toml`. Campos ausentes assumem o padrão **da
rede configurada**; campos desconhecidos são erro.

```toml
network = "mainnet"            # mainnet | testnet | regtest
data_dir = "/var/lib/thecoin"  # padrão: ~/.thecoin (mainnet) ou ~/.thecoin/<rede>

[p2p]
enabled = true
listen = "0.0.0.0:7333"        # "" desliga conexões de entrada
max_inbound = 32
max_outbound = 8
seeds = []                     # seeds extras "host:porta" (somam às embutidas)
connect = []                   # se não vazio: conecta SOMENTE a estes
allow_private = false          # aceita IPs privados via gossip (true em regtest)

[rpc]
enabled = true
listen = "127.0.0.1:7334"      # mantenha local, exceto seeds/explorador
cors_origins = ["*"]           # ex.: ["https://the-coin.cloud"]
max_concurrency = 64

[mining]
enabled = true
address = ""                   # vazio = sem mineração
threads = 0                    # 0 = núcleos − 1 (mínimo 1)
signal = []                    # ids (hex) de propostas de governança apoiadas
mode = "auto"                  # RandomX: auto | fast | light (§1.1)

[storage]
cache_mb = 64                  # cache de páginas do banco
prune = true                   # padrão: guarda só os últimos prune_keep blocos
prune_keep = 40320             # uma semana de blocos de 15 s (mínimo efetivo 1000)
address_index = true           # histórico por endereço (necessário para /address/{a}/txs; forçado a false em nós podados)

[mempool]
max_mb = 32
```

| Campo | Padrão | Notas |
|---|---|---|
| `network` | `mainnet` | define portas e diretório padrão |
| `data_dir` | `~/.thecoin[/rede]` | contém `chain.redb`, `peers.json` e, com o nó parado, `mempool.dat` (transações pendentes salvas ao desligar) |
| `p2p.enabled` | `true` | `false` = nó isolado (só testes) |
| `p2p.listen` | `0.0.0.0:<porta P2P>` | 7333 / 17333 / 27333 |
| `p2p.max_inbound` | 32 | limite extra de 4 por IP público |
| `p2p.max_outbound` | 8 | |
| `p2p.seeds` | `[]` | |
| `p2p.connect` | `[]` | útil para redes privadas |
| `p2p.allow_private` | `false` (`true` em regtest) | |
| `rpc.enabled` | `true` | |
| `rpc.listen` | `127.0.0.1:<porta API>` | 7334 / 17334 / 27334 |
| `rpc.cors_origins` | `["*"]` | |
| `rpc.max_concurrency` | 64 | requisições simultâneas |
| `mining.enabled` | `true` | só minera se `address` estiver preenchido |
| `mining.address` | `""` | bech32m da rede |
| `mining.threads` | 0 | as threads compartilham a memória do RandomX (§1.1) |
| `mining.signal` | `[]` | ids de propostas apoiadas; também `--signal` e `thecoin signal <id>`; ver [GOVERNANCE.md](GOVERNANCE.md#44-sinalizar-mineradores) |
| `mining.mode` | `"auto"` | `auto` = rápido quando houver ≥ 3 072 MiB de memória livre; `fast` = dataset de 2 GiB; `light`/`off` = só o cache de 256 MiB (§1.1) |
| `storage.cache_mb` | 64 | reduza para 16–32 em máquinas de 1 GB |
| `storage.prune` | `true` | **ligada por padrão**; nós seed e explorador devem ser **arquivo** (`false`) |
| `storage.prune_keep` | 40 320 | uma semana de blocos de 15 s; valores abaixo de 1 000 são elevados a 1 000 |
| `storage.address_index` | `true` | pode desligar em mineradores para economizar disco (≈ 82 bytes por transação); em nós **podados** o nó o desliga sozinho ao abrir o banco |
| `mempool.max_mb` | 32 | |

## 5. Portas e firewall

| Porta | Uso | Exposição |
|---|---|---|
| 7333/tcp | P2P mainnet | **aberta** para a internet |
| 7334/tcp | API REST | somente local, ou somente para o IP do servidor do site |
| 17333/17334 | testnet | idem |

```bash
sudo ufw allow OpenSSH
sudo ufw allow 7333/tcp
# apenas em seeds que alimentam o site:
sudo ufw allow from <IP_DO_SERVIDOR_WEB> to any port 7334 proto tcp
sudo ufw enable
```

Em provedores com firewall próprio (OVH, AWS, etc.), libere também a 7333 no painel.

## 6. Nós seed (`seed1` e `seed2.the-coin.cloud`)

Os seeds são o ponto de entrada de novos nós e a fonte de dados do site.

1. **DNS:** registros `A` (e `AAAA` se houver IPv6) para `seed1.the-coin.cloud`
   e `seed2.the-coin.cloud` apontando para as duas VPS. Para testnet:
   `testnet-seed1/2.the-coin.cloud`. TTL baixo (300 s) facilita trocas.
2. **Instalação:** instalador com `--public-api --archive` (ou edite
   `rpc.listen = "0.0.0.0:7334"` e `prune = false`).
3. **Configuração recomendada:**
   ```toml
   [p2p]
   max_inbound = 64
   seeds = ["seed2.the-coin.cloud:7333"]   # em seed1 (e vice-versa)
   [rpc]
   listen = "0.0.0.0:7334"
   cors_origins = ["https://the-coin.cloud"]
   [storage]
   prune = false          # obrigatório: com a poda ligada o índice de endereços é desligado
   address_index = true
   ```
4. **Firewall:** 7333 aberta; 7334 somente para o servidor web (§5).
5. **Relógio:** `timedatectl set-ntp true`.
6. Mantenha os dois seeds em provedores/regiões diferentes quando possível.

## 7. Site e explorador

O site (`website/`) roda em outra máquina com nginx, TLS (Let's Encrypt) e
proxy de `/api/` para os dois seeds com *rate limit* e cache — configuração
pronta em `website/nginx/the-coin.cloud.conf`. Detalhes em
[API.md](API.md#expondo-a-api-publicamente) e `website/README.md`.

Para montar tudo o que o site serve (páginas, `install.sh`, whitepaper e
binários no layout `releases/latest/thecoin-<target>.tar.gz` + `.sha256` que o
instalador espera):

```bash
scripts/package.sh x86_64-unknown-linux-gnu      # ou baixe os arquivos do GitHub Release para dist/
scripts/publish-site.sh                          # gera dist/site/
rsync -av --delete dist/site/ usuario@maquina-do-site:/var/www/the-coin.cloud/
```

## 8. Monitoramento

```bash
systemctl status thecoind
journalctl -u thecoind -f                          # logs ao vivo
journalctl -u thecoind --since "1 hour ago" | grep -E "WARN|ERROR"
curl -s http://127.0.0.1:7334/api/v1/status | jq '{height, finalized_height, peers, syncing, mempool_txs, congestion_bp, software_upgrade_required}'
curl -s http://127.0.0.1:7334/api/v1/mining  | jq '{hashrate, blocks_found, signal_proposals}'
curl -s http://127.0.0.1:7334/api/v1/peers   | jq length
curl -s http://127.0.0.1:7334/api/v1/alerts  | jq length                # tentativas de gasto duplo vistas
```

Mensagens de log importantes:

| Log | Significado |
|---|---|
| `new tip height=…` | novo bloco na cadeia principal |
| `block found!` | seu minerador achou um bloco |
| `starting block sync` / `block sync finished` | sincronização |
| `chain reorganization depth=…` | troca de ramo (normal com profundidade 1–2) |
| `refusing reorganization deeper than the maximum` | ramo alternativo com > 2 880 blocos (720 na regtest) — investigar |
| `block finalised by the miners' votes height=…` | a altura ficou **irreversível** pela finalidade assinada (§9.1) |
| `not enough memory for the 2 GiB RandomX dataset: mining in light mode` | o modo rápido não coube na memória; a mineração continua no modo leve (§1.1) |
| `rejected invalid block` | peer enviou bloco inválido (é banido) |
| `peer misbehaving` | pontuação de mau comportamento |
| `double spend attempt` / `double spend attempt reported by peer` | duas transações do mesmo remetente e nonce (alerta listado em `/api/v1/alerts`) |
| `saved pending transactions txs=…` | ao desligar, o mempool foi gravado em `mempool.dat` |
| `restored pending transactions kept=… dropped=…` | ao iniciar, transações de `mempool.dat` revalidadas (as que ficaram inválidas são descartadas) |

Alertas sugeridos: `peers == 0` por mais de 10 min; altura parada por mais de
15 min; `software_upgrade_required != null`; disco > 80 %; `congestion_bp` alto
por muito tempo (blocos cheios: considere propor aumento de limites);
`height − finalized_height` crescendo sem parar (a finalidade travou — veja §9.1;
a cadeia continua funcionando pelo trabalho, só sem o carimbo de finalidade).

### 8.1 Governança: sinalizar como minerador

```bash
thecoin signals              # o que seus blocos apoiam hoje + propostas abertas
sudo thecoin signal <id>     # apoiar (reinicia o nó)
sudo thecoin unsignal <id>   # deixar de apoiar
```

Sem o comando `thecoin`: `thecoind --signal <id>` (repetível) ou
`[mining] signal = ["<id>"]` no `thecoind.toml`. Detalhes em
[GOVERNANCE.md](GOVERNANCE.md#44-sinalizar-mineradores).

## 9. Backups

* **Carteira:** o que importa é a **frase de recuperação** (24 palavras) —
  guarde em papel, offline. O arquivo `~/.thecoin/wallet-<rede>.json` é
  criptografado e pode ser copiado, mas sem a senha não serve.
  `thecoin-wallet show-mnemonic` mostra a frase (cuidado com o terminal).
* **Blockchain:** não precisa de backup — é re-sincronizada da rede. Para
  acelerar a recuperação de um seed, copie `/var/lib/thecoin/chain.redb` com o nó **parado**.
* **Transações pendentes:** ao desligar normalmente (`systemctl stop`, `thecoin restart`,
  atualização) o nó grava o mempool em `/var/lib/thecoin/mempool.dat` e o recarrega ao
  iniciar, revalidando cada transação; o arquivo é apagado depois de lido. Um
  desligamento abrupto (queda de energia, `kill -9`) perde o mempool, que é
  reconstruído pela rede.
* **Configuração:** `/etc/thecoin/thecoind.toml`.
* **Chave de finalidade:** `<data-dir>/finality.key` (§9.1).

### 9.1 Chave de finalidade (`finality.key`)

Desde a v0.2 os blocos carregam a **chave pública de assinatura** do minerador, e
os mineradores dos últimos `finality_window` blocos (200 na mainnet e na testnet,
20 na regtest) assinam o bloco **anterior ao topo**. Quando as assinaturas somam
2/3 do peso da janela — e a janela tem pelo menos 4 mineradores diferentes — o
bloco vira **final** e nenhum nó aceita um ramo que o remova
([ESCALA.md §3.3](ESCALA.md)).

* O nó cria `<data-dir>/finality.key` (32 bytes em hexadecimal, permissão `0600`)
  na primeira inicialização, e a reusa depois. Nada a configurar.
* A chave pública vai no campo `signer` de cada bloco que este nó minera. Um nó
  que **não** minerou nenhum bloco da janela não emite votos — eles não teriam
  peso. Nós que só validam continuam recebendo e retransmitindo os votos dos
  outros e contam a finalidade normalmente.
* **Nunca copie o mesmo `finality.key` para dois nós.** Assinar dois blocos
  diferentes na mesma altura é prova pública de má-fé: a chave é banida por todos
  os nós e aquele minerador perde o direito de votar.
* Pelo mesmo motivo, **evite reiniciar o nó em laço durante uma reorganização**:
  o controle de "um voto por altura" fica em memória e é zerado no boot, então um
  nó reiniciado pode votar de novo numa altura em que já votou. Um reinício
  normal (atualização, `thecoin restart`) é seguro.
* Ao migrar um nó de máquina, leve o `finality.key` junto **ou** deixe o novo nó
  criar outro — mas nunca deixe os dois rodando.
* Acompanhe `finalized_height` em `/api/v1/status` e o campo `finalized` de cada
  bloco em `/api/v1/block/{id}`.

## 10. Poda (padrão) e nós arquivo

A poda vem **ligada por padrão** depois da v0.2.0:

```toml
[storage]
prune = true            # padrão
prune_keep = 40320      # uma semana de blocos de 15 s
```

O nó continua validando tudo, mas apaga corpos de blocos mais antigos que
`prune_keep` (valores abaixo de 1 000 são elevados a 1 000) e, junto com cada
corpo, as entradas do índice de transações e os **recibos** daquele bloco. Nós
podados não servem blocos antigos a outros peers (não anunciam o bit `ARCHIVE`),
não respondem `/api/v1/tx/{txid}` nem `/api/v1/block/{id}` para blocos antigos e
**não mantêm o índice de endereços**: ao abrir o banco com poda ligada o nó força
`address_index = false`, porque o índice apontaria para corpos apagados. Histórico
por endereço é papel de nós arquivo/exploradores.

Cabeçalhos e estado completo são sempre mantidos. Dados de *undo* (para
reorganizações) são guardados para os últimos `max_reorg_depth + 16` blocos em
qualquer modo — **2 896** na mainnet e na testnet, 736 na regtest.

**Nó arquivo** (explorador, seed que serve o site, quem precisa de histórico):

```bash
curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes --archive
```

ou, manualmente, `prune = false` no `thecoind.toml` — não há opção de linha de
comando para desligar a poda. Um nó que já rodou podado **não recupera** os
corpos apagados: para virar arquivo é preciso ressincronizar do zero.

Quanto isso economiza, e por que a poda foi ligada por padrão:
[ESCALA.md §4](ESCALA.md).

Após ativar a poda (ou apagar muitos dados), recupere espaço do arquivo:

```bash
sudo systemctl stop thecoind
sudo -u thecoin thecoind --config /etc/thecoin/thecoind.toml compact
sudo systemctl start thecoind
```

## 11. Atualização

```bash
curl -fsSL https://the-coin.cloud/install.sh | sudo bash    # mantém config e dados
thecoind --version
```

* Acompanhe propostas `SoftwareUpgrade` na governança: após ativação, nós
  antigos exibem `software_upgrade_required` e podem ficar incompatíveis.
* Atualize durante o `activation_delay` (2 880 blocos ≈ **12 h** na mainnet, com
  blocos de 15 s).
* Se uma versão mudar o esquema do banco, o nó informa
  `unsupported database schema version; resync required`: pare, apague
  `chain.redb` e reinicie para sincronizar de novo. **A v0.2 usa o esquema 2 e
  outro gênese**: dados de um nó v0.1 precisam ser apagados (a carteira continua válida).
* `sudo thecoin update` (ou o instalador de novo) também instala a versão nova do `tccl`.

## 12. Solução de problemas

| Sintoma | Causa provável / solução |
|---|---|
| `cannot bind P2P port` | outra instância rodando ou porta ocupada: `ss -ltnp \| grep 7333` |
| `database at … belongs to a different network/genesis` | `data_dir` de outra rede — use outro diretório |
| `state commitment does not match tip header` | banco corrompido (disco cheio/falha) — pare, apague `chain.redb`, re-sincronize |
| `peers: 0` | porta 7333 bloqueada, DNS dos seeds, ou `p2p.connect` errado; teste `nc -vz seed1.the-coin.cloud 7333` |
| altura não sobe, `syncing: true` | aguarde; o sync troca de peer após 90 s sem progresso |
| blocos rejeitados por `timestamp … too far in the future` | relógio errado: `timedatectl set-ntp true` |
| `mining enabled but no mining.address configured` | preencha `[mining] address` |
| hashrate 0 | nó sincronizando (mineração pausada) ou `threads` = 0 com 1 núcleo? veja `/api/v1/mining` |
| máquina lenta | reduza `mining.threads`; o minerador já roda com prioridade mínima (nice 19) |
| nó morre por falta de memória (OOM) ao minerar | o modo rápido do RandomX pede 2 GiB; force `[mining] mode = "light"` (§1.1) |
| `not enough memory for the 2 GiB RandomX dataset` | esperado em máquinas pequenas: o nó já caiu sozinho para o modo leve |
| `finalized_height` parado em 0 | a janela ainda não tem 4 mineradores diferentes, ou este nó nunca minerou; é normal em redes pequenas (§9.1) |
| compilação morre (OOM) | ative swap e use `CARGO_BUILD_JOBS=1` |
| `invalid mining.address: address belongs to another network` | endereço `tct1…` na mainnet ou vice-versa |

## 13. Checklist de lançamento da mainnet

1. **Congelar parâmetros de consenso** (`crates/core/src/params.rs`): mensagem e
   `genesis_timestamp` do gênese, alvos iniciais, seeds. Rodar
   `thecoind params` e publicar o hash do gênese.
2. **Testnet** rodando por pelo menos algumas semanas com os dois seeds e
   mineradores externos; testar contratos, governança e reorganizações.
3. **Build reprodutível** da versão 0.2.0 (`scripts/package.sh`, que empacota `thecoind`, `thecoin-wallet` e `tccl`), publicar
   `thecoin-<target>.tar.gz` + `.sha256` no GitHub Releases e em
   `https://the-coin.cloud/releases/`.
4. **DNS** de `seed1`/`seed2.the-coin.cloud` e firewall conforme §5–6.
5. **Subir seed1 e seed2** (minerando, e como **arquivo**: `--archive`) antes do
   anúncio; confirmar que se conectam (`/api/v1/peers`) e produzem blocos.
   Com apenas dois mineradores **não existe finalidade assinada** — ela só começa
   quando a janela de 200 blocos tiver 4 mineradores diferentes (§9.1), o que é
   esperado: até lá `finalized_height` fica em 0 e a cadeia vale pelo trabalho.
6. **Site** no ar com proxy para os seeds e o instalador em `/install.sh`.
7. **Anúncio** com o hash do gênese e instruções de instalação.
8. Nos blocos 1 a 6 a dificuldade fica no valor de gênese; a partir do bloco 7
   o LWMA ajusta a cada bloco (janela crescente até 120 blocos, no máximo 2× por
   bloco). Acompanhe o tempo médio de bloco (**alvo 15 s**). Lembre que
   recompensas só ficam 25 % gastáveis após **400 blocos** (≈ 100 min) e 100 %
   após **4 000 blocos** (≈ 16,7 h).
9. **Checkpoints:** após algumas semanas, adicionar em `checkpoints` alturas
   profundamente confirmadas numa versão nova (protege contra reescritas longas).
10. Documentar responsáveis por segurança (`security@the-coin.cloud`) e
    processo de divulgação responsável.
