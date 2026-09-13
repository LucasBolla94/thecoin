# The Coin (TCN)

**Uma moeda digital de prova de trabalho, leve e democrática, feita para rodar em VPS simples.**
Site: <https://the-coin.cloud> · Whitepaper: [`docs/whitepaper/the-coin-whitepaper-v0.1.pdf`](docs/whitepaper/the-coin-whitepaper-v0.1.pdf)

| | |
|---|---|
| Oferta máxima | **50.000.000 TCN** (sem pré-mineração) |
| Emissão | 40 TCN/bloco, halving a cada 625.000 blocos (≈ 1,19 ano) — 93,75% emitido no ciclo de estabilização de ≈ 4,76 anos |
| Tempo de bloco | 60 s, dificuldade ajustada a cada bloco (LWMA-1) |
| Prova de trabalho | **CoinHash** = Argon2id (16 MiB, memory-hard, amigável a CPU) |
| Modelo | Contas + nonces, Ed25519, BLAKE3, endereços bech32m (`tc1…`) |
| Contratos | Escrow, vesting, assinatura recorrente, HTLC (atomic swaps), multisig |
| Governança | Bicameral: detentores votam com moedas bloqueadas **e** mineradores sinalizam nos blocos |
| Estado | Compromisso homomórfico LtHash em cada bloco, banco redb, blocos comprimidos com zstd, poda opcional |
| Requisitos | 1 vCPU, 1 GB RAM, 5 GB de disco, Linux |

## Entrar na rede (instalador)

```bash
curl -fsSL https://the-coin.cloud/install.sh | sudo bash
```

O instalador baixa os binários verificando SHA-256, cria uma carteira para receber as recompensas de
mineração, configura um serviço `systemd` endurecido e inicia o nó, que passa a validar, retransmitir e
minerar blocos automaticamente. Detalhes em [`docs/OPERATIONS.md`](docs/OPERATIONS.md).

## Compilar a partir do código

```bash
# Rust 1.80+ (https://rustup.rs)
cargo build --release
./target/release/thecoind params            # parâmetros da rede
./target/release/thecoind --miner-address tc1...   # roda um nó minerando
```

## Carteira

```bash
thecoin-wallet create                         # cria carteira (anote as 24 palavras!)
thecoin-wallet address                        # seu endereço
thecoin-wallet balance
thecoin-wallet send tc1... 12.5 --memo "Pedido 42"
thecoin-wallet request 3.5 --memo "Café"      # gera URI thecoin:... para cobrar
thecoin-wallet pay "thecoin:tc1...?amount=3.5&memo=Caf%C3%A9"
thecoin-wallet contract escrow-create --payee tc1... --amount 100 --arbiter tc1...
thecoin-wallet gov list
thecoin-wallet gov vote <proposta> yes 1000
```

## Rede local de testes (regtest)

```bash
export THECOIN_NETWORK=regtest THECOIN_WALLET_PASSWORD=dev
thecoin-wallet create && ADDR=$(thecoin-wallet address)
thecoind --network regtest --miner-address $ADDR --threads 1
# em outro terminal, um segundo nó que sincroniza do primeiro:
thecoind --network regtest --data-dir /tmp/node2 --listen 127.0.0.1:27335 --rpc 127.0.0.1:27336 \
         --no-mine --peer 127.0.0.1:27333 --connect-only
```

## Estrutura do repositório

| Caminho | Conteúdo |
|---|---|
| `crates/core` | Regras de consenso (tipos, criptografia, PoW, dificuldade, emissão, contratos, governança, função de transição de estado). Sem I/O — reutilizável por qualquer carteira ou implementação. |
| `crates/storage` | Armazenamento redb + zstd (blocos, estado, undo, índices). |
| `crates/node` | `thecoind`: gerenciador da cadeia, P2P, mempool, minerador, API REST. |
| `crates/wallet` | `thecoin-wallet`: BIP-39/SLIP-10, keystore criptografado, construção de transações, cliente da API. |
| `installer/` | `install.sh` / `uninstall.sh`. |
| `website/` | Site the-coin.cloud (landing, explorador, governança, downloads) + configuração nginx. |
| `docs/` | Especificação do protocolo, P2P, API, guia para desenvolvedores de carteiras, governança, contratos, operação, arquitetura, whitepaper. |

## Documentação

- [PROTOCOL.md](docs/PROTOCOL.md) — especificação de consenso (para implementações independentes)
- [P2P.md](docs/P2P.md) — protocolo de rede
- [API.md](docs/API.md) — API REST do nó
- [WALLET_DEVELOPERS.md](docs/WALLET_DEVELOPERS.md) — como criar carteiras compatíveis (com vetores de teste)
- [CONTRACTS.md](docs/CONTRACTS.md) — contratos de pagamento
- [GOVERNANCE.md](docs/GOVERNANCE.md) — governança on-chain
- [OPERATIONS.md](docs/OPERATIONS.md) — operação de nós e seeds
- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — arquitetura do software
- [CONTRIBUTING.md](CONTRIBUTING.md) — como contribuir

## Testes

```bash
cargo test --workspace          # unitários, consenso ponta a ponta e rede multi-nó (regtest)
cargo test -p thecoin-core --release --test bench -- --ignored --nocapture   # medições de desempenho
```

## Licença

MIT ou Apache-2.0, à sua escolha.
