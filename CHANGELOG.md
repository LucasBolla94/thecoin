# Changelog

Todas as mudanças relevantes deste projeto são documentadas aqui.
O formato segue [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e o projeto usa
[versionamento semântico](https://semver.org/lang/pt-BR/). Mudanças de consenso são sempre
destacadas.

## [0.2.0] — 2026-09-13

Contratos inteligentes TCCL, privacidade por contratos, novo modelo de taxas e
confirmação mais segura. **Incompatível com a v0.1** (novo formato de transação,
novo registro global e, portanto, novo gênese; o banco de dados precisa ser
sincronizado de novo). Especificação: [docs/PROTOCOL.md](docs/PROTOCOL.md).

### Consenso
- **Formato de transação:** novo byte `flags` no `TxBody`, logo após `chain_id`; bit `0x01` = `FLAG_REPLACEABLE` (substituição por taxa opcional); bits desconhecidos invalidam a transação.
- **Novo modelo de taxas:** `mínima = (base_fee + ceil(bytes × fee_per_kb / 1000) + ceil(max_fuel × fee_per_kfuel / 1000)) × congestion_bp / 10000`. A sobretaxa acima de 1× é **queimada**; o restante e as gorjetas vão ao minerador.
- **Multiplicador de congestionamento** (`ChainGlobal.congestion_bp`): ajusta até ±12,5 % por bloco buscando 50 % de ocupação (bytes ou combustível), entre 1× e 1000×.
- **Parâmetros de governança** agora: `max_block_bytes`, `max_block_fuel`, `base_fee`, `fee_per_kb`, `fee_per_kfuel`, `storage_deposit_per_kb`, `proposal_deposit`, `vote_period`, `quorum_bp`, `approval_bp`, `miner_approval_bp`, `activation_delay` (substituem `min_fee_per_byte`).
- **Cooldown de recompensas:** 25 % da recompensa do bloco (subsídio + taxas) liberados após `coinbase_maturity` (100) e o restante após `reward_unlock_blocks` (1 000; regtest 5/12). `PendingReward` ganhou `released`.
- **LWMA-1 com aquecimento:** ajusta a partir de 6 tempos de bloco (bloco 7) com janela crescente até 60, e limite de ±2× por bloco.
- **Contratos inteligentes TCCL:** ações `Deploy { source, init_args, value, max_fuel, max_deposit }` e `Invoke { contract, function, args, value, max_fuel, max_deposit }`; o compilador faz parte do consenso (o código-fonte vai na transação); VM determinística com combustível calibrado (≈ 20 ns de CPU por unidade); chamada que falha paga a taxa e reverte todo o resto; novos registros de estado `0x07` (metadados), `0x08` (código) e `0x09` (armazenamento), todos no `state_root`; limites de 64 kB por `Deploy`, 10 M de combustível por transação e 50 M por bloco.
- **Depósitos de armazenamento reembolsáveis** (`storage_deposit_per_kb`): contratos nativos (`Contract.deposit`, devolvido ao criador ao terminar) e TCCL (`ProgramMeta.deposit`, cobrado de quem faz o estado crescer, até `max_deposit`, e devolvido a quem libera espaço ou no `destroy`).
- **`MultisigClose`**: encerra um multisig vazio e devolve o depósito.
- **`ring_verify`**: assinaturas em anel bLSAG sobre Ristretto255 como função embutida da VM, base dos pools de privacidade.

### Nó (`thecoind`)
- Mempool ordenado por taxa por peso (`bytes + max_fuel/100`), com simulação de contratos, níveis de prioridade, substituição só para transações substituíveis (taxa ≥ 125 %), registro de tentativas de gasto duplo e transações abaixo da taxa por congestionamento mantidas até voltarem a ser válidas.
- Mempool persistido em `<data-dir>/mempool.dat` ao desligar e revalidado ao iniciar.
- P2P: **compact blocks** (ids curtos de 6 bytes salgados com o hash do bloco, `GetBlockTxs`/`BlockTxs`, cabeçalho e PoW verificados antes de alocar) e mensagem **`DoubleSpend`** (um alerta por remetente e nonce, só para contas com saldo).
- API: `/api/v1/tx/simulate`, `/api/v1/program/{addr}`, `/api/v1/program/{addr}/view`, `/api/v1/security`, `/api/v1/alerts`; `/api/v1/fees` com parâmetros e prioridades; transações com recibo (`success`, `error`, `fuel_used`, `burned`, `logs`, `return_value`, `program`), `replaceable` e `conflict`; endereços com `immature`; `congestion_bp` no status.
- Armazenamento: esquema 2, tabela de recibos, índice de endereços compacto (sem txid), poda também remove índice de transações e recibos; nós podados não mantêm o índice de endereços.
- Opção `--signal <id>` (repetível) para apoiar propostas de governança.
- Medição de disco (100 000 transferências): ≈ 430 bytes de dados por transação com índice de endereços e ≈ 348 sem ele.

### Carteira (`thecoin-wallet`)
- `--priority low|normal|high|urgent`, `fees`, `--replaceable`, `bump-fee <txid>`, `confirmations <valor>`, `alerts`.
- Contratos TCCL: `contract deploy|invoke|view|program` com combustível medido por simulação; `contract multisig-close`.
- Privacidade: `privacy keygen|deposit|withdraw|status` (pools TCCL-PRIV-1, chaves de anel em `m/44'/7333'/0'/7'/i'`).
- Vetores de teste publicados verificados por `cargo test -p thecoin-wallet --test vectors`.

### TCCL (`tccl`)
- Nova crate e ferramenta `tccl`: `check`, `abi`, `run` (simulador local), `ring keygen|sign`; exemplos em `crates/tccl/examples` (token, crowdfund, tip jar, contador, pool de privacidade).

### Distribuição e documentação
- Instalador de uma linha (`curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes`) passa a instalar também o `tccl`; comando `thecoin` ganhou `signals`, `signal <id>` e `unsignal <id>`.
- Whitepaper v0.2, roteiro de consenso PoW → híbrido → PoS ([docs/ROADMAP.md](docs/ROADMAP.md)), referência TCCL e cookbook; documentação de protocolo, API, carteiras, contratos, governança, operação, P2P e arquitetura atualizada.

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
