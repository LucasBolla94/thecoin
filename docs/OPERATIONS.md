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
| RAM | 1 GB (+ swap de 1 GB) | 2–4 GB |
| Disco | 5 GB livres | 20 GB SSD |
| SO | Linux com systemd | Ubuntu/Debian LTS |
| Rede | porta TCP 7333 aberta para entrada | IP público fixo |
| Relógio | NTP ativo (`timedatectl`) | — |

Consumo medido (VPS 2 vCPU Haswell, 3,7 GB RAM):

| Métrica | Valor |
|---|---|
| Memória do nó minerando com 1 thread | ≈ 42 MB RSS |
| CoinHash | ≈ 12 ms/hash → 83–87 H/s por thread |
| Verificação de assinaturas | ≈ 14 400 transações/s por núcleo |
| Aplicar bloco com 5 000 tx (790 kB) | 0,39 s + 62 ms de LtHash |
| Contratos TCCL no pior caso | bloco cheio (50 M de combustível) ≈ 1 s de CPU |
| Disco por bloco vazio | ≈ 765 bytes (~400 MB/ano) |
| Disco por transação — nó arquivo **com** índice de endereços | ≈ 430 bytes de dados (≈ 792 bytes alocados no arquivo) |
| Disco por transação — nó arquivo **sem** índice de endereços | ≈ 348 bytes de dados (≈ 633 bytes alocados) |
| Transferência simples (sem memo) | 159 bytes |

Os números de disco vêm de 100 000 transferências na regtest (v0.2):

```bash
GROWTH_BLOCKS=500 cargo test -p thecoin-node --release --test storage_growth -- --ignored --nocapture
```

*Dados* é o que o banco realmente usa (blocos comprimidos, estado, undo, índices
e recibos). *Alocados* são os blocos de disco do arquivo `chain.redb`: o redb
cresce o arquivo em dobras e reserva espaço à frente, então o arquivo ocupa mais
que os dados. Esse excesso é recuperável com `thecoind compact` (nó parado, §10).

Cada thread de mineração usa 16 MiB para o CoinHash.

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
| `--public-api` | `THECOIN_PUBLIC_API=1` | API em `0.0.0.0` (só para seeds/explorador) |
| — | `THECOIN_RELEASES_URL` | outra origem dos binários (espelho/testes) |
| — | `THECOIN_SITE_URL` | outra origem do comando `thecoin` e do desinstalador |

Exemplos:

```bash
# minerar para um endereço existente
curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes --miner-address tc1seuendereco...
# nó seed que serve a API ao site
curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes --no-mine --public-api
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
sudo apt install -y build-essential pkg-config git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://github.com/LucasBolla94/thecoin && cd thecoin
cargo build --release -p thecoin-node -p thecoin-wallet -p tccl
sudo install -m755 target/release/thecoind target/release/thecoin-wallet target/release/tccl /usr/local/bin/
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
| `--prune` | poda corpos antigos de blocos |
| `--log-level <nível>` | `error`…`trace` (padrão `info`) — env `THECOIN_LOG` |

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

[storage]
cache_mb = 64                  # cache de páginas do banco
prune = false                  # true = guarda só os últimos prune_keep blocos
prune_keep = 10000             # mínimo efetivo 1000
address_index = true           # histórico por endereço (necessário para /address/{a}/txs; ignorado em nós podados)

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
| `mining.threads` | 0 | |
| `mining.signal` | `[]` | ids de propostas apoiadas; também `--signal` e `thecoin signal <id>`; ver [GOVERNANCE.md](GOVERNANCE.md#44-sinalizar-mineradores) |
| `storage.cache_mb` | 64 | reduza para 16–32 em máquinas de 1 GB |
| `storage.prune` | `false` | nós seed e explorador devem ser **arquivo** (sem poda) |
| `storage.prune_keep` | 10 000 | |
| `storage.address_index` | `true` | pode desligar em mineradores para economizar disco (≈ 82 bytes por transação); nós podados nunca mantêm o índice |
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
2. **Instalação:** instalador com `--public-api` (ou edite `rpc.listen = "0.0.0.0:7334"`).
3. **Configuração recomendada:**
   ```toml
   [p2p]
   max_inbound = 64
   seeds = ["seed2.the-coin.cloud:7333"]   # em seed1 (e vice-versa)
   [rpc]
   listen = "0.0.0.0:7334"
   cors_origins = ["https://the-coin.cloud"]
   [storage]
   prune = false
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
scripts/package.sh x86_64-unknown-linux-musl     # ou baixe os arquivos do GitHub Release para dist/
scripts/publish-site.sh                          # gera dist/site/
rsync -av --delete dist/site/ usuario@maquina-do-site:/var/www/the-coin.cloud/
```

## 8. Monitoramento

```bash
systemctl status thecoind
journalctl -u thecoind -f                          # logs ao vivo
journalctl -u thecoind --since "1 hour ago" | grep -E "WARN|ERROR"
curl -s http://127.0.0.1:7334/api/v1/status | jq '{height, peers, syncing, mempool_txs, congestion_bp, software_upgrade_required}'
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
| `refusing reorganization deeper than the maximum` | ramo alternativo com > 720 blocos — investigar |
| `rejected invalid block` | peer enviou bloco inválido (é banido) |
| `peer misbehaving` | pontuação de mau comportamento |
| `double spend attempt` / `double spend attempt reported by peer` | duas transações do mesmo remetente e nonce (alerta listado em `/api/v1/alerts`) |
| `saved pending transactions txs=…` | ao desligar, o mempool foi gravado em `mempool.dat` |
| `restored pending transactions kept=… dropped=…` | ao iniciar, transações de `mempool.dat` revalidadas (as que ficaram inválidas são descartadas) |

Alertas sugeridos: `peers == 0` por mais de 10 min; altura parada por mais de
15 min; `software_upgrade_required != null`; disco > 80 %; `congestion_bp` alto
por muito tempo (blocos cheios: considere propor aumento de limites).

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

## 10. Poda (economia de disco)

```toml
[storage]
prune = true
prune_keep = 10000      # ~7 dias de blocos
```

O nó continua validando tudo, mas apaga corpos de blocos mais antigos que
`prune_keep` (mínimo 1 000, para suportar reorganizações) e, junto com cada
corpo, as entradas do índice de transações e os **recibos** daquele bloco. Nós
podados não servem blocos antigos a outros peers, não respondem
`/api/v1/tx/{txid}` nem `/api/v1/block/{id}` para blocos antigos e **não mantêm o
índice de endereços** (histórico por endereço é papel de nós arquivo/exploradores).
Cabeçalhos e estado completo são mantidos. Dados de *undo* só são guardados
para os últimos 736 blocos em qualquer modo.

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
* Atualize durante o `activation_delay` (≈ 2 dias na mainnet).
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
5. **Subir seed1 e seed2** (minerando) antes do anúncio; confirmar que se
   conectam (`/api/v1/peers`) e produzem blocos.
6. **Site** no ar com proxy para os seeds e o instalador em `/install.sh`.
7. **Anúncio** com o hash do gênese e instruções de instalação.
8. Nos blocos 1 a 6 a dificuldade fica no valor de gênese; a partir do bloco 7
   o LWMA ajusta a cada bloco (janela crescente até 60 blocos, no máximo 2× por
   bloco). Acompanhe o tempo médio de bloco (alvo 60 s). Lembre que recompensas
   só ficam 25 % gastáveis após 100 blocos e 100 % após 1 000.
9. **Checkpoints:** após algumas semanas, adicionar em `checkpoints` alturas
   profundamente confirmadas numa versão nova (protege contra reescritas longas).
10. Documentar responsáveis por segurança (`security@the-coin.cloud`) e
    processo de divulgação responsável.
