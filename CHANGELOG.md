# Changelog

Todas as mudanças relevantes deste projeto são documentadas aqui.
O formato segue [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e o projeto usa
[versionamento semântico](https://semver.org/lang/pt-BR/). Mudanças de consenso são sempre
destacadas.

## [0.1.0] — 2026-09-13

Primeira versão pública da implementação de referência.

### Consenso
- Prova de trabalho **CoinHash** (Argon2id, 16 MiB, t=1) e ajuste de dificuldade **LWMA-1** por bloco (janela 60, alvo 60 s).
- Emissão: 40 TCN iniciais, halving a cada 625 000 blocos, teto de 50 000 000 TCN, maturação de 100 blocos, sem pré-mineração.
- Modelo de contas com Ed25519 estrito, BLAKE3 com separação de domínio, endereços bech32m (`tc`/`tct`/`tcr`).
- Compromisso de estado **LtHash16** em cada cabeçalho; árvore Merkle sem duplicação de folhas.
- Transações: transferência com memo, lote (até 128 saídas), validade opcional, replay protection por `chain_id` + nonce.
- Contratos de pagamento nativos: escrow, vesting, assinatura recorrente, HTLC (SHA-256) e multisig M-de-N.
- Governança bicameral (detentores com votos bloqueados + sinalização de mineradores), parâmetros com limites rígidos.
- Escolha da cadeia por trabalho acumulado, reorganização máxima de 720 blocos, suporte a checkpoints.

### Nó (`thecoind`)
- Armazenamento redb (ACID) com zstd, undo limitado, poda opcional, índice de endereços, comando `compact`.
- P2P com framing verificado, sincronização por localizadores, verificação paralela de PoW, relay por inventário, gerenciador de endereços e banimento.
- Mempool com simulação completa, substituição por taxa, limites de memória.
- Minerador de CPU com prioridade mínima e pausa durante a sincronização.
- API REST `/api/v1` para carteiras, explorador e site.
- Endurecimento contra DoS: pool de órfãos limitado, forks antigos ignorados, filas por par limitadas em bytes.

### Carteira (`thecoin-wallet`)
- BIP-39 + SLIP-0010 (`m/44'/7333'/a'/0'/i'`), arquivo cifrado (Argon2id + ChaCha20-Poly1305).
- Envio, lote, cobrança/pagamento por URI `thecoin:`, histórico, contratos e governança.

### Distribuição e documentação
- Instalador `install.sh` com verificação SHA-256, serviço systemd endurecido e criação de carteira; `uninstall.sh`.
- Workflows de CI e release (Linux musl x86_64/aarch64, macOS, Windows).
- Site the-coin.cloud (landing, explorador, governança, downloads) com configuração nginx.
- Whitepaper v0.1 (PDF) e documentação de protocolo, P2P, API, carteiras, contratos, governança, operação e arquitetura.
