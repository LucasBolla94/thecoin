# Operação de nós — The Coin

Guia para quem roda `thecoind`: participantes que mineram numa VPS, operadores
dos nós seed (`seed1`/`seed2.the-coin.cloud`) e quem hospeda o site.

## 1. Requisitos

| Recurso | Mínimo | Recomendado |
|---|---|---|
| CPU | 1 vCPU x86_64 ou ARM64 | 2 vCPU |
| RAM | 1 GB (+ swap de 1 GB) | 2–4 GB |
| Disco | 5 GB livres | 20 GB SSD |
| SO | Linux com systemd | Ubuntu/Debian LTS |
| Rede | porta TCP 7333 aberta para entrada | IP público fixo |
| Relógio | NTP ativo (`timedatectl`) | — |

Consumo medido na v0.1 (VPS 2 vCPU Haswell, 3,7 GB RAM):

| Métrica | Valor |
|---|---|
| Memória do nó minerando com 1 thread | ≈ 42 MB RSS |
| CoinHash | ≈ 12 ms/hash → 83–87 H/s por thread |
| Verificação de assinaturas | ≈ 14 400 transações/s por núcleo |
| Aplicar bloco com 5 000 tx (790 kB) | 0,39 s + 62 ms de LtHash |
| Disco por bloco vazio | ≈ 765 bytes (~400 MB/ano) |
| Disco por transação (nó arquivo com índice de endereços) | ≈ 886 bytes |
| Disco por transação (sem índice de endereços) | ≈ 437 bytes |
| Transferência simples | 158 bytes |

Cada thread de mineração usa 16 MiB para o CoinHash.

## 2. Instalação

### 2.1 Instalador (recomendado)

```bash
curl -fsSL https://the-coin.cloud/install.sh | sudo bash
```

O instalador (`installer/install.sh`):

1. detecta a arquitetura e baixa `thecoind` + `thecoin-wallet`, verificando o SHA-256 (ou compila do código-fonte se não houver binário);
2. oferece criar 1 GB de swap em máquinas com menos de 2 GB de RAM sem swap;
3. cria o usuário de sistema `thecoin` e `/var/lib/thecoin`;
4. cria uma carteira nova (ou usa um endereço informado) para receber as recompensas;
5. grava `/etc/thecoin/thecoind.toml` e o serviço systemd `thecoind` (com *hardening*, `Nice=5`, `MemoryHigh` = 70 % da RAM);
6. abre a porta P2P no `ufw` (se ativo) e inicia o nó.

Opções (também por variável de ambiente):

| Opção | Variável | Descrição |
|---|---|---|
| `--network mainnet\|testnet` | `THECOIN_NETWORK` | rede (padrão mainnet) |
| `--miner-address tc1…` | `THECOIN_MINER_ADDRESS` | endereço de recompensa (sem perguntar) |
| `--no-mine` | `THECOIN_NO_MINE=1` | só valida e retransmite |
| `--threads N` | `THECOIN_THREADS` | threads de mineração (0 = núcleos − 1) |
| `--version x.y.z` | `THECOIN_VERSION` | versão específica |
| `--from-source` | `THECOIN_FROM_SOURCE=1` | compila localmente |
| `--public-api` | `THECOIN_PUBLIC_API=1` | API em `0.0.0.0` (só para seeds/explorador) |
| `--yes` | — | não interativo |

Exemplo não interativo:

```bash
curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes --miner-address tc1seuendereco...
```

Desinstalar (mantém dados e carteiras; `--purge` apaga dados e configuração):

```bash
curl -fsSL https://the-coin.cloud/uninstall.sh | sudo bash
```

### 2.2 Manual (código-fonte)

```bash
sudo apt install -y build-essential pkg-config git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://github.com/LucasBolla94/thecoin && cd thecoin
cargo build --release -p thecoin-node -p thecoin-wallet
sudo install -m755 target/release/thecoind target/release/thecoin-wallet /usr/local/bin/
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
address_index = true           # histórico por endereço (necessário para /address/{a}/txs)

[mempool]
max_mb = 32
```

| Campo | Padrão | Notas |
|---|---|---|
| `network` | `mainnet` | define portas e diretório padrão |
| `data_dir` | `~/.thecoin[/rede]` | contém `chain.redb` e `peers.json` |
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
| `mining.signal` | `[]` | ver [GOVERNANCE.md](GOVERNANCE.md#44-sinalizar-mineradores) |
| `storage.cache_mb` | 64 | reduza para 16–32 em máquinas de 1 GB |
| `storage.prune` | `false` | nós seed e explorador devem ser **arquivo** (sem poda) |
| `storage.prune_keep` | 10 000 | |
| `storage.address_index` | `true` | pode desligar em mineradores para economizar disco |
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
curl -s http://127.0.0.1:7334/api/v1/status | jq '{height, peers, syncing, mempool_txs, software_upgrade_required}'
curl -s http://127.0.0.1:7334/api/v1/mining  | jq '{hashrate, blocks_found}'
curl -s http://127.0.0.1:7334/api/v1/peers   | jq length
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

Alertas sugeridos: `peers == 0` por mais de 10 min; altura parada por mais de
15 min; `software_upgrade_required != null`; disco > 80 %.

## 9. Backups

* **Carteira:** o que importa é a **frase de recuperação** (24 palavras) —
  guarde em papel, offline. O arquivo `~/.thecoin/wallet-<rede>.json` é
  criptografado e pode ser copiado, mas sem a senha não serve.
  `thecoin-wallet show-mnemonic` mostra a frase (cuidado com o terminal).
* **Blockchain:** não precisa de backup — é re-sincronizada da rede. Para
  acelerar a recuperação de um seed, copie `/var/lib/thecoin/chain.redb` com o nó **parado**.
* **Configuração:** `/etc/thecoin/thecoind.toml`.

## 10. Poda (economia de disco)

```toml
[storage]
prune = true
prune_keep = 10000      # ~7 dias de blocos
```

O nó continua validando tudo, mas apaga corpos de blocos mais antigos que
`prune_keep` (mínimo 1 000, para suportar reorganizações). Nós podados não
servem blocos antigos a outros peers nem histórico antigo pela API.
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
  `chain.redb` e reinicie para sincronizar de novo.

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
3. **Build reprodutível** da versão 0.1.0 (`scripts/package.sh`), publicar
   `thecoin-<target>.tar.gz` + `.sha256` no GitHub Releases e em
   `https://the-coin.cloud/releases/`.
4. **DNS** de `seed1`/`seed2.the-coin.cloud` e firewall conforme §5–6.
5. **Subir seed1 e seed2** (minerando) antes do anúncio; confirmar que se
   conectam (`/api/v1/peers`) e produzem blocos.
6. **Site** no ar com proxy para os seeds e o instalador em `/install.sh`.
7. **Anúncio** com o hash do gênese e instruções de instalação.
8. Nos primeiros 61 blocos a dificuldade fica no valor de gênese; depois o
   LWMA ajusta a cada bloco. Acompanhe o tempo médio de bloco (alvo 60 s).
9. **Checkpoints:** após algumas semanas, adicionar em `checkpoints` alturas
   profundamente confirmadas numa versão nova (protege contra reescritas longas).
10. Documentar responsáveis por segurança (`security@the-coin.cloud`) e
    processo de divulgação responsável.
