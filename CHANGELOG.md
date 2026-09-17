# Changelog

Todas as mudanças relevantes deste projeto são documentadas aqui.
O formato segue [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e o projeto usa
[versionamento semântico](https://semver.org/lang/pt-BR/). Mudanças de consenso são sempre
destacadas.

## [Não lançado]

Mineração em computadores comuns, blocos de 15 s e pagamentos irreversíveis em
cerca de 30 segundos. **Incompatível com a v0.2.0** (nova prova de trabalho, novo
cabeçalho de bloco, novos parâmetros e novo registro de estado, portanto novo
gênese: a cadeia recomeça e o banco de dados precisa ser sincronizado do zero).
Especificação: [docs/PROTOCOL.md](docs/PROTOCOL.md); estudo que motivou as
mudanças: [docs/ESCALA.md](docs/ESCALA.md). A versão do `Cargo.toml` continua 0.2.0.

### Consenso
- **Prova de trabalho RandomX:** o CoinHash passa a ser `RandomX(key, borsh(BlockHeader))` com `key = tagged_hash("randomx-key", chain_id || época)` e `época = altura / 2048` (regtest: 64), no lugar do Argon2id (16 MiB), em que uma GPU valia de 20 a 100 CPUs. A chave depende só da altura; verificação no modo leve (cache de 256 MiB, ≈ 30 ms por bloco numa VPS de 2 vCPU). O modo de mineração não é consenso.
- **Blocos de 15 s** (mainnet/testnet; regtest continua em 60 s): LWMA-1 com janela de 120 blocos (era 60), ainda ajustando a partir do bloco 7 com limite de ±2× por bloco; alvo inicial recalibrado (`genesis_target_shift` 12 na mainnet, 9 na testnet).
- **Emissão recalibrada, mesma curva no tempo:** 10 TCN por bloco (era 40) e halving a cada 2 500 000 blocos (era 625 000) — os mesmos 40 TCN por minuto, ≈ 1,19 ano por era e o mesmo teto de 50 000 000 TCN.
- **Cooldown de recompensas:** 25 % após `coinbase_maturity` = 400 blocos (≈ 1 h 40 min) e o restante após `reward_unlock_blocks` = 4 000 (≈ 16 h 40 min); regtest continua 5/12.
- **Reorganização máxima:** 2 880 blocos (≈ 12 h; era 720). Regtest continua 720.
- **Cabeçalho de 244 bytes** (era 180): novos campos `uncles_root` e `signer`; o corpo do bloco passa a ser `BlockBody { txs, uncles }`.
- **Tios (*uncles*):** até 2 por bloco, com até 6 blocos de idade, prova de trabalho e alvo válidos, pai num dos 6 blocos anteriores do ramo e nunca repetidos. O minerador do tio recebe `subsídio × (7 − idade) / 24` (25 % a 4 %), **descontado do subsídio** do bloco que o inclui — a emissão não aumenta. O trabalho dos tios entra no `chainwork`. `PendingReward` virou uma lista de `Payout { who, amount, released }` (minerador primeiro, depois os tios), cada um com o próprio cooldown.
- **Finalidade assinada pelos mineradores:** os mineradores anunciam uma chave Ed25519 no campo `signer`; os mineradores dos últimos `finality_window` blocos (200; regtest 20), com peso igual aos blocos que mineraram, assinam o bloco anterior ao topo, um voto por altura. Com ≥ 2/3 do peso e pelo menos 4 mineradores diferentes na janela cheia, o bloco fica **final** e os nós recusam ramos que o removam. Assinar dois blocos na mesma altura bane a chave. Sem votos a cadeia segue só pelo trabalho.
- **TCCL versão 2** (tag `v0.3.0` do repositório da linguagem): novos deploys compilam em `LANGUAGE_VERSION = 2` (records, enums com transições, papéis, interfaces, módulos e biblioteca padrão); contratos chamam contratos na mesma transação atômica (sem reentrada, até 8 contratos encadeados); o depósito de armazenamento é acertado para cada contrato tocado, sob o mesmo `max_deposit`. Programas da versão 1 continuam rodando como antes.
- **Upgrades de contratos:** novas ações `Upgrade { contract, source, expected_code_hash, args, max_fuel, max_deposit }` e `SetUpgradeAuthority { contract, new_authority, expected_code_hash }`, novo registro de estado `0x0A ProgramAdmin { authority, code_version, language, previous_code_hash }` (ausente = contrato final) e flag `FLAG_FINAL_DEPLOY` (`0x02`) para publicar um contrato já final.
- **Governança (mainnet):** `vote_period` 80 640 blocos (≈ 14 dias) e `activation_delay` 2 880 (≈ 12 h); limites de `vote_period` 5 760 – 806 400 e de `activation_delay` 720 – 172 800. Testnet: `vote_period` 5 760.

### Nó (`thecoind`)
- RandomX com cache compartilhado por todas as threads (até dois caches na virada de época) e `[mining] mode = "auto" | "fast" | "light"`: `auto` usa o dataset de 2 GiB quando há ≥ 3 072 MiB livres e cai para o modo leve se ele não couber. Memória do nó minerando com 1 thread no modo leve: ≈ 280 MB RSS (antes ≈ 42 MB).
- Votos de finalidade: chave em `<data-dir>/finality.key` criada na primeira inicialização, voto a cada novo topo, nova mensagem P2P `FinalityVote` (votos estacionados para blocos ainda desconhecidos, equívocos banidos) e bloco final registrado no log.
- Tios: blocos que perdem a corrida são guardados e incluídos como tios pelo minerador; compact blocks carregam os cabeçalhos dos tios.
- **Poda ligada por padrão:** `prune = true` e `prune_keep = 40 320` blocos (≈ 1 semana); nós arquivo usam `prune = false`.
- O mempool não executa código de contrato para transações recebidas pela rede nem na revalidação (evita gastar CPU de graça); nonce, saldo e taxa continuam conferidos, e envios pela API do próprio nó ainda recusam chamadas que falhariam.
- API: `finalized_height` em `/api/v1/status`; `finalized` e `uncles` em `/api/v1/block/{id}`; `blocks_to_finality` em `/api/v1/security` (quando a rede está finalizando, a recomendação passa a ser esperar a finalidade); `upgrade_authority`, `code_version`, `code_hash` e `language` em `/api/v1/program/{addr}`; ações `upgrade` e `set_upgrade_authority` nas transações.
- `thecoind params` mostra a prova de trabalho RandomX e a época.
- `/api/v1/security`: `minutes` passa a corresponder a `confirmations` (antes usava a recomendação sem o limite da finalidade e mostrava 25 min para 2 blocos).

### Carteira (`thecoin-wallet`)
- `contract upgrade <endereço> <arquivo> [args…] [--yes-upgrade]` e `contract authority <endereço> <novo|none>` (renunciar torna o contrato final para sempre, com confirmação).
- `contract program` mostra a versão da linguagem, a versão do código e a autoridade de upgrade; `contract deploy` compila na versão 2.
- `confirmations` informa os blocos até a finalidade quando a rede está finalizando.
- Prazos padrão ajustados para blocos de 15 s: `escrow-create --deadline-blocks` 40 320 (≈ 7 dias, era 10 080) e `htlc-create --timeout-blocks` 5 760 (≈ 1 dia, era 1 440).

### TCCL (`tccl`)
- A linguagem saiu de `crates/tccl` para o repositório próprio [github.com/LucasBolla94/tccl](https://github.com/LucasBolla94/tccl), fixado na tag `v0.3.0` no `Cargo.toml` (trocar a tag é hard fork). A ferramenta `tccl` vem desse repositório (`tccl-cli`), com `new`, `bundle`, `test` (cenários), `explain`, `bench` e `run … upgrade|authority|state`.
- Cópias locais da versão 1 dos exemplos em `docs/tccl/examples`; `scripts/publish-site.sh` pega os exemplos do repositório da linguagem na tag fixada.

### Explorador/site
- Explorador mostra finalidade dos mineradores (blocos e transações finais, altura finalizada), tios com idade e recompensa, upgrades e autoridade de upgrade dos contratos, durações em blocos de 15 s e cooldown de 400 / 4 000 blocos.
- Explorador multi-rede em `explore.the-coin.cloud` e variante do site hospedada no próprio nó (`scripts/host-site-here.sh`).

### Instalador
- Novo `--archive` (`THECOIN_ARCHIVE=1`): nó arquivo com `prune = false`; sem ele o `thecoind.toml` gerado usa a poda padrão (40 320 blocos).
- Compilação a partir do código instala `cmake` e `g++` (o RandomX é C++); binários Linux de release passam a ser `x86_64/aarch64-unknown-linux-gnu` (antes `musl`).
- A ferramenta `tccl` é compilada a partir do repositório da linguagem na tag fixada, no instalador (compilação a partir do código; opcional), em `scripts/package.sh` e no workflow de release.
- `scripts/host-site-here.sh`: HTTPS sem os arquivos do plugin nginx do certbot e criação do diretório de cache da API.

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
