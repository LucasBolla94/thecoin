# The Coin — Especificação do Protocolo de Consenso (v0.2)

Este documento especifica **todas as regras de consenso** da The Coin v0.2 com
precisão suficiente para uma implementação independente (em qualquer
linguagem) chegar exatamente aos mesmos hashes, estados e decisões que o nó de
referência em Rust (`crates/core` e a linguagem TCCL,
[github.com/LucasBolla94/tccl](https://github.com/LucasBolla94/tccl) na tag fixada no `Cargo.toml`).

> **Convenções**
>
> * 🔒 **Consenso** — alterar qualquer item marcado assim é um *hard fork*
>   (nós antigos passam a rejeitar blocos de nós novos ou vice‑versa).
> * 🧩 **Enums são append-only** — a posição (índice) de cada variante é parte
>   do formato binário. Novas variantes só podem ser **adicionadas no final**;
>   nunca reordenar, remover ou renomear com mudança de posição.
> * Todos os inteiros são **little-endian**, exceto quando indicado (alvos de
>   PoW, chaves de altura no estado e chaves de armazenamento de contratos TCCL
>   são big-endian).
> * `||` = concatenação de bytes. `ceil(a/b)` = divisão inteira arredondada para cima.
> * Código de referência entre parênteses, ex.: (`core/src/tx.rs`).

> **v0.2 é incompatível com v0.1.** O `TxBody` ganhou o byte `flags`, o registro
> `ChainGlobal` e os parâmetros de governança mudaram (logo o gênese mudou), há
> novas ações (`Deploy`, `Invoke`), novo modelo de taxas, cooldown de recompensa,
> depósitos de armazenamento e aquecimento do LWMA. O roteiro de consenso de longo
> prazo (PoW → híbrido → PoS) está em [ROADMAP.md](ROADMAP.md).

> **Mudanças de consenso depois do v0.2.0 publicado** (ver [CHANGELOG.md](../CHANGELOG.md)):
> a prova de trabalho passou a ser **RandomX** com chave por época (§7), o tempo
> de bloco caiu para **15 s** com a mesma emissão no tempo e o mesmo teto (§10),
> o cabeçalho ganhou `uncles_root` e `signer` e passou a ter **244 bytes** (§6.1),
> blocos carregam até **2 tios** (§6.3), existe **finalidade assinada pelos
> mineradores** (§19.3), o estado ganhou o registro `0x0A ProgramAdmin` (§11) e a
> TCCL passou a compilar na **versão de linguagem 2**, com as ações `Upgrade` e
> `SetUpgradeAuthority` (§17). O estudo que motivou os itens de escala está em
> [ESCALA.md](ESCALA.md). Tudo isso muda o gênese e é incompatível com qualquer
> cadeia anterior.

Índice

1. [Unidades e valores](#1-unidades-e-valores)
2. [Hash com separação de domínio](#2-hash-com-separação-de-domínio)
3. [Chaves, assinaturas e endereços](#3-chaves-assinaturas-e-endereços)
4. [Codificação canônica (Borsh)](#4-codificação-canônica-borsh)
5. [Transações](#5-transações)
6. [Blocos e cabeçalhos](#6-blocos-e-cabeçalhos)
7. [Prova de trabalho — CoinHash](#7-prova-de-trabalho--coinhash)
8. [Ajuste de dificuldade — LWMA-1](#8-ajuste-de-dificuldade--lwma-1)
9. [Regras de tempo](#9-regras-de-tempo)
10. [Emissão e recompensas](#10-emissão-e-recompensas)
11. [Estado global](#11-estado-global)
12. [Compromisso de estado — LtHash16](#12-compromisso-de-estado--lthash16)
13. [Árvore de Merkle](#13-árvore-de-merkle)
14. [Validação de transações e taxas](#14-validação-de-transações-e-taxas)
15. [Processamento de bloco](#15-processamento-de-bloco)
16. [Contratos de pagamento](#16-contratos-de-pagamento)
17. [Contratos inteligentes TCCL](#17-contratos-inteligentes-tccl)
18. [Governança](#18-governança)
19. [Seleção de cadeia](#19-seleção-de-cadeia)
20. [Parâmetros por rede](#20-parâmetros-por-rede)
21. [Gênese](#21-gênese)
22. [Vetores de teste](#22-vetores-de-teste)

---

## 1. Unidades e valores

| Item | Valor |
|---|---|
| Ticker | `TCN` |
| Menor unidade | **mote** |
| 1 TCN | `100 000 000` motes (8 casas decimais) (`core/src/amount.rs`) |
| Supply máximo 🔒 | `50 000 000 TCN` = `5 000 000 000 000 000` motes (`MAX_SUPPLY`) |

Todos os valores no protocolo são `u64` em motes. Ponto flutuante nunca é usado
em consenso. Toda aritmética é **checada**: um overflow torna a transação/bloco
inválido (dentro de contratos TCCL, um overflow é uma falha do contrato, §17.8).

## 2. Hash com separação de domínio

🔒 Todo hash do protocolo é BLAKE3 (saída de 32 bytes) com uma *tag* de domínio
(`core/src/hash.rs`):

```
tagged_hash(tag, parts...) = BLAKE3( "TheCoin:" || tag (UTF-8) || 0x00 || parts[0] || parts[1] || ... )
```

Tags definidas:

| Tag | Uso |
|---|---|
| `tx-sign` | mensagem assinada de uma transação |
| `txid` | identificador de transação |
| `block` | hash (id) de bloco |
| `merkle-leaf`, `merkle-node`, `merkle-empty` | árvore de Merkle (txids e tios) |
| `address` | derivação de endereço |
| `contract-id` | id de contrato de pagamento nativo |
| `proposal-id` | id de proposta |
| `program-address` | endereço de contrato TCCL (§17.1) |
| `tccl-source` | hash do código-fonte TCCL (`ProgramMeta.source_hash`) |
| `program-code` | hash do código compilado (`expected_code_hash`, §17.11) |
| `randomx-key` | chave de época da prova de trabalho (§7) |
| `finality-vote` | mensagem assinada de um voto de finalidade (§19.3) |
| `state-root` | raiz de estado a partir do LtHash |
| `genesis` | `prev_hash` do bloco gênese |
| `finality-vote-id` | id de voto de finalidade (só relay, não é consenso) |
| `p2p` | checksum de frames P2P (não é consenso) |
| `short-id` | ids curtos de compact blocks (não é consenso, ver [P2P.md](P2P.md)) |

Um `Hash32` é serializado como 32 bytes crus; em JSON como hex minúsculo.

## 3. Chaves, assinaturas e endereços

### 3.1 Assinaturas 🔒

* Esquema: **Ed25519** (RFC 8032), chave pública de 32 bytes, assinatura de 64 bytes.
* Verificação **estrita** (`verify_strict` do `ed25519-dalek`): rejeita
  assinaturas não canônicas (`S >= L`), pontos não canônicos e **chaves
  públicas fracas** (de ordem pequena). Isso elimina maleabilidade: o `txid`
  só muda se o dono da chave produzir uma nova assinatura. (`core/src/crypto.rs`)

### 3.2 Endereços 🔒

```
address (20 bytes) = tagged_hash("address", public_key_32)[0..20]
```

Forma textual: **bech32m** (BIP‑350) sobre os 20 bytes (sem versão de
*witness*), com HRP por rede:

| Rede | HRP | Exemplo |
|---|---|---|
| mainnet | `tc` | `tc1f420y9z7hu6as5yexhcklv0tklh6glty0n9yvj` |
| testnet | `tct` | `tct1f420y9z7hu6as5yexhcklv0tklh6gltyfk2we0` |
| regtest | `tcr` | `tcr1f420y9z7hu6as5yexhcklv0tklh6glty6qra7u` |

Decodificação deve exigir checksum **bech32m** (não bech32), HRP igual ao da
rede e exatamente 20 bytes. O endereço `Address::ZERO` (20 zeros) não tem chave
conhecida e é usado como "minerador" do gênese. Contratos TCCL também têm
endereços de 20 bytes (§17.1), no mesmo espaço das contas.

## 4. Codificação canônica (Borsh)

🔒 Todas as estruturas de consenso usam [Borsh](https://borsh.io) com as regras:

| Tipo | Codificação |
|---|---|
| `u8`, `u16`, `u32`, `u64`, `i128` | little-endian, largura fixa (`i128` = 16 bytes, complemento de dois) |
| `bool` | 1 byte: `0x00` ou `0x01` (outros valores inválidos) |
| `[u8; N]` (arrays fixos: `Hash32`, `Address`, chave, assinatura, target) | N bytes crus, sem prefixo |
| `Vec<T>` | `u32` LE com o número de elementos, depois os elementos |
| `String` | `u32` LE com número de **bytes**, depois UTF‑8 (inválido se não for UTF‑8) |
| `Option<T>` | `0x00` (None) ou `0x01` seguido de `T` |
| `enum` | `u8` com o índice da variante, depois os campos da variante em ordem |
| `struct` | campos em ordem de declaração, sem separadores |
| `Box<T>` | igual a `T` |
| tupla `(A, B)` | `A` seguido de `B` |

Ao decodificar uma transação avulsa (ex.: `POST /api/v1/tx`), bytes extras no
final são rejeitados.

## 5. Transações

(`core/src/tx.rs`)

### 5.1 Estruturas

```rust
struct Transaction {
    body: TxBody,
    public_key: [u8; 32],
    signature: [u8; 64],
}

struct TxBody {
    version: u8,        // = 1 (TX_VERSION)
    chain_id: u32,      // proteção de replay entre redes
    flags: u8,          // bits de opção (§5.2); bits desconhecidos = inválida
    nonce: u64,         // deve ser igual ao nonce da conta remetente
    fee: u64,           // motes; >= taxa mínima (§14.3); o excedente é gorjeta do minerador
    expiry_height: u64, // última altura em que pode entrar num bloco; 0 = sem expiração
    action: TxAction,
}
```

Layout do `TxBody` (antes da ação):
`version(1) | chain_id(4) | flags(1) | nonce(8) | fee(8) | expiry_height(8)` = **30 bytes**, seguido da ação.

### 5.2 `flags` 🔒

| Bit | Constante | Significado |
|---|---|---|
| `0x01` | `FLAG_REPLACEABLE` | o remetente permite que a transação seja substituída **no mempool** por outra com o mesmo nonce e taxa maior (*replace-by-fee* opt-in) |
| `0x02` | `FLAG_FINAL_DEPLOY` | numa ação `Deploy`, implanta um contrato que **nunca** poderá ser atualizado (nenhum registro `ProgramAdmin` é criado, §17.11) |

`KNOWN_FLAGS = 0x03`. `flags & !KNOWN_FLAGS != 0` → transação inválida.
`FLAG_REPLACEABLE` não muda a validade on-chain; a regra de substituição
(taxa ≥ 125 %) é política de mempool (ver [API.md](API.md#post-apiv1tx)).
Receptores de pagamentos **sem** o bit podem confiar na primeira versão vista.
`FLAG_FINAL_DEPLOY` **é** consenso: ele decide se o registro `0x0A` é gravado no
deploy; em qualquer outra ação o bit é simplesmente ignorado.

### 5.3 `TxAction` 🧩

| Índice | Variante | Campos (em ordem) |
|---|---|---|
| 0 | `Transfer` | `to: Address`, `amount: u64`, `memo: Vec<u8>` |
| 1 | `BatchTransfer` | `outputs: Vec<TransferOutput>`, `memo: Vec<u8>` |
| 2 | `CreateContract` | `spec: ContractSpec` |
| 3 | `CallContract` | `contract: Hash32`, `call: ContractCall` |
| 4 | `Propose` | `proposal: ProposalSpec` |
| 5 | `Vote` | `proposal: Hash32`, `choice: VoteChoice`, `weight: u64` |
| 6 | `Deploy` | `source: String`, `init_args: Vec<Value>`, `value: u64`, `max_fuel: u64`, `max_deposit: u64` |
| 7 | `Invoke` | `contract: Address`, `function: String`, `args: Vec<Value>`, `value: u64`, `max_fuel: u64`, `max_deposit: u64` |
| 8 | `Upgrade` | `contract: Address`, `source: String`, `expected_code_hash: Hash32`, `args: Vec<Value>`, `max_fuel: u64`, `max_deposit: u64` |
| 9 | `SetUpgradeAuthority` | `contract: Address`, `new_authority: Option<Address>`, `expected_code_hash: Hash32` |

`TransferOutput { to: Address, amount: u64 }`.

### 5.4 `Value` (argumentos TCCL) 🧩

(`tccl/src/program.rs`)

| Índice | Variante | Conteúdo |
|---|---|---|
| 0 | `Int` | `i128` (16 bytes LE) |
| 1 | `Bool` | `bool` |
| 2 | `Text` | `String` |
| 3 | `Bytes` | `Vec<u8>` |
| 4 | `Address` | `[u8; 20]` |
| 5 | `List` | `Vec<Value>` |
| 6 | `Unit` | — |

Exemplo: `Value::Int(2500)` = `00 c409000000000000 0000000000000000`;
`Value::Address(a)` = `04 || a(20)`.

Decodificação 🔒: índice desconhecido é inválido e listas podem ter no máximo
`MAX_VALUE_DEPTH = 64` níveis de aninhamento; uma transação cujos `init_args`/`args`
violem isso não decodifica (é rejeitada como malformada).

### 5.5 `ContractSpec` 🧩

| Índice | Variante | Campos |
|---|---|---|
| 0 | `Escrow` | `payee: Address`, `arbiter: Option<Address>`, `amount: u64`, `deadline_height: u64` |
| 1 | `Vesting` | `beneficiary: Address`, `amount: u64`, `start_height: u64`, `cliff_height: u64`, `end_height: u64`, `revocable: bool` |
| 2 | `Subscription` | `payee: Address`, `amount_per_period: u64`, `period_blocks: u64`, `max_periods: u32` |
| 3 | `Htlc` | `recipient: Address`, `amount: u64`, `hash_lock: Hash32`, `timeout_height: u64` |
| 4 | `Multisig` | `signers: Vec<Address>`, `threshold: u8`, `initial_deposit: u64` |

### 5.6 `ContractCall` 🧩

| Índice | Variante | Campos |
|---|---|---|
| 0 | `EscrowRelease` | — |
| 1 | `EscrowRefund` | — |
| 2 | `VestingClaim` | — |
| 3 | `VestingRevoke` | — |
| 4 | `SubscriptionClaim` | — |
| 5 | `SubscriptionCancel` | — |
| 6 | `HtlcRedeem` | `preimage: Vec<u8>` |
| 7 | `HtlcRefund` | — |
| 8 | `MultisigDeposit` | `amount: u64` |
| 9 | `MultisigPropose` | `to: Address`, `amount: u64`, `memo: Vec<u8>` |
| 10 | `MultisigApprove` | `spend_id: u32` |
| 11 | `MultisigCancel` | `spend_id: u32` |
| 12 | `MultisigClose` | — |

### 5.7 Governança 🧩

```rust
struct ProposalSpec { title: String, url: String, content_hash: Hash32, action: ProposalAction }
```

| `ProposalAction` | Índice | Campos |
|---|---|---|
| `Text` | 0 | — |
| `SetParam` | 1 | `param: GovParamId`, `value: u64` |
| `SoftwareUpgrade` | 2 | `version: String`, `release_hash: Hash32` |

| `VoteChoice` | Índice |
|---|---|
| `Yes` | 0 |
| `No` | 1 |
| `Abstain` | 2 |

| `GovParamId` | Índice | Nome textual |
|---|---|---|
| `MaxBlockBytes` | 0 | `max_block_bytes` |
| `MaxBlockFuel` | 1 | `max_block_fuel` |
| `BaseFee` | 2 | `base_fee` |
| `FeePerKb` | 3 | `fee_per_kb` |
| `FeePerKfuel` | 4 | `fee_per_kfuel` |
| `StorageDepositPerKb` | 5 | `storage_deposit_per_kb` |
| `ProposalDeposit` | 6 | `proposal_deposit` |
| `VotePeriod` | 7 | `vote_period` |
| `QuorumBp` | 8 | `quorum_bp` |
| `ApprovalBp` | 9 | `approval_bp` |
| `MinerApprovalBp` | 10 | `miner_approval_bp` |
| `ActivationDelay` | 11 | `activation_delay` |

### 5.8 Assinatura e identificadores 🔒

```
signing_hash = tagged_hash("tx-sign", borsh(TxBody))
signature    = Ed25519.sign(secret_key, signing_hash)     // assina os 32 bytes do hash
txid         = tagged_hash("txid", borsh(Transaction))
sender       = Address::from_public_key(public_key)
size         = len(borsh(Transaction))
max_fuel     = action.max_fuel se Deploy/Invoke, senão 0
```

Como `fee` é um `u64` de largura fixa, **o tamanho não depende do valor da
taxa**: carteiras assinam uma vez com `fee = 0` para medir `size` e depois
assinam de novo com a taxa calculada pela fórmula da §14.3.

## 6. Blocos e cabeçalhos

(`core/src/block.rs`)

### 6.1 Cabeçalho — 244 bytes fixos 🔒

| Offset | Tamanho | Campo | Descrição |
|---|---|---|---|
| 0 | 4 | `version: u32` | `1` (`BLOCK_VERSION`) |
| 4 | 8 | `height: u64` | altura (gênese = 0) |
| 12 | 32 | `prev_hash: Hash32` | hash do cabeçalho pai |
| 44 | 32 | `tx_root: Hash32` | raiz de Merkle dos txids |
| 76 | 32 | `state_root: Hash32` | compromisso de estado **após** aplicar o bloco |
| 108 | 8 | `timestamp: u64` | segundos Unix |
| 116 | 32 | `target: [u8; 32]` | alvo de PoW, inteiro de 256 bits **big-endian** |
| 148 | 8 | `nonce: u64` | espaço de busca do minerador |
| 156 | 20 | `miner: Address` | recebe subsídio + taxas (após o cooldown, §10) |
| 176 | 4 | `signal: u32` | bits de sinalização de governança |
| 180 | 32 | `uncles_root: Hash32` | raiz de Merkle dos cabeçalhos dos tios (§6.3) |
| 212 | 32 | `signer: [u8; 32]` | chave pública Ed25519 com que este minerador vota na finalidade (§19.3); 32 zeros = não participa |
| **244** | | | fim |

```rust
struct Block     { header: BlockHeader, txs: Vec<Transaction>, uncles: Vec<BlockHeader> }
struct BlockBody { txs: Vec<Transaction>, uncles: Vec<BlockHeader> }   // corpo como é armazenado/relayado
```

Tamanho serializado de um bloco =
`244 + 4 + Σ tamanho(tx) + 4 + 244 × número de tios`. Um bloco vazio e sem tios
tem **252 bytes**; com 2 tios, 740 bytes.

### 6.2 Hash do bloco 🔒

```
block_hash = tagged_hash("block", borsh(BlockHeader))
```

É o identificador usado em toda parte (P2P, índices, `prev_hash`). Barato de
calcular — **não** é o hash da prova de trabalho. Um tio é identificado pelo
mesmo hash do seu cabeçalho.

### 6.3 Tios (*uncles*) 🔒

(`core/src/block.rs`, `node/src/chain.rs::check_uncles`, `core/src/execution.rs`)

Um **tio** é um bloco válido que perdeu a corrida: ele não está na cadeia, mas
um bloco posterior pode carregar o **cabeçalho** dele e pagar parte do subsídio
ao seu minerador. Isso tira do minerador pequeno o prejuízo de perder corridas
por milissegundos, que com blocos de 15 s seria grande (motivação e medições em
[ESCALA.md](ESCALA.md#2-blocos-órfãos)).

```
uncles_root = merkle_root([ hash(uncle_0), hash(uncle_1) ])       // §13; lista vazia = tagged_hash("merkle-empty")
MAX_UNCLES = 2            MAX_UNCLE_DEPTH = 6
depth(uncle) = bloco.height − uncle.height
uncle_reward(subsidy, depth) = floor(subsidy × (7 − depth) / 24)  // 0 se depth == 0 ou depth > 6
```

`uncle_reward` vale 25 % do subsídio em `depth = 1` e cai até 4 % em `depth = 6`.

**Regras de validade** de cada tio de um bloco na altura `h` (todas obrigatórias):

1. `header.uncles_root` é igual à raiz calculada sobre os tios do corpo, e
   `len(uncles) <= 2`.
2. Nenhum tio repetido dentro do bloco.
3. `uncle.height < h` e `h − uncle.height <= 6`.
4. `uncle.version == BLOCK_VERSION`.
5. O tio **não** pode já estar na cadeia nem ter sido incluído como tio por
   algum dos **6** blocos anteriores deste ramo.
6. `uncle.prev_hash` é um dos 6 blocos anteriores deste ramo (o tio realmente
   saiu desta cadeia) e `pai.height + 1 == uncle.height`.
7. `uncle.target` é exatamente o alvo exigido para a altura dele (§8) e
   `uncle.timestamp` respeita MTP e deriva futura como um bloco normal (§9).
8. `CoinHash(uncle) <= uncle.target` — o tio tem prova de trabalho válida.

**Pagamento:** o total pago aos tios é **descontado do subsídio do bloco** —
a emissão por bloco não aumenta e o teto de 50 000 000 TCN não muda (§10). Se
`Σ uncle_reward > subsidy` o bloco é inválido.

**Peso da cadeia:** o trabalho dos tios entra no `chainwork` do bloco que os
inclui (§19.2), o que dá ao minerador o incentivo de incluí-los e encarece
reorganizações.

## 7. Prova de trabalho — CoinHash

🔒 (`core/src/pow.rs`)

**CoinHash é RandomX** com uma chave que gira por época:

```
epoch          = height / pow.epoch_blocks
key            = tagged_hash("randomx-key", u32_le(chain_id) || u64_le(epoch))
CoinHash(head) = RandomX(key, borsh(BlockHeader))      // 244 bytes de entrada, 32 de saída
```

| Rede | `pow.epoch_blocks` | Época em segundos |
|---|---|---|
| mainnet | 2 048 | ≈ 8,5 h (15 s por bloco) |
| testnet | 2 048 | ≈ 8,5 h |
| regtest | 64 | — |

* A chave depende **apenas** de `chain_id` e da altura, então qualquer nó
  verifica um cabeçalho sabendo só a altura dele, sem ler a cadeia, e uma
  reorganização nunca muda a chave de um bloco.
* **Modo leve** (todo nó, para verificar): cache de **256 MiB** compartilhado
  por todas as threads, ≈ 30 ms por bloco numa VPS de 2 vCPU.
* **Modo rápido** (opcional, só para minerar): dataset de **2 GiB**, várias
  vezes mais rápido; escolhido em `[mining] mode = auto|fast|light`
  (`auto` = rápido quando há ~3 GiB livres). O modo **não** é consenso: o
  resultado do hash é idêntico nos dois.
* **Por que RandomX:** Argon2id é limitado por *banda* de memória, e uma placa
  de vídeo tem ~1 000 GB/s contra ~2 GB/s de um servidor pequeno — uma GPU
  valeria de 20 a 100 CPUs. RandomX executa um programa aleatório feito das
  operações em que uma CPU é boa, então GPU ≈ CPU e a mineração continua nos
  computadores comuns. Medições:
  `cargo test -p thecoin-core --release --test pow_bench -- --ignored --nocapture`.

**Validade:** interpretar os 32 bytes do CoinHash como inteiro de 256 bits
big-endian `H`. O bloco é válido se `H <= target`.

**Trabalho** de um bloco (usado na escolha de cadeia):

```
work(target) = 2^256 / (target + 1)  = (~target / (target + 1)) + 1     // U256
work(0xff..ff) = 1
chainwork(bloco) = chainwork(pai) + work(bloco.target)    // gênese: work(genesis_target)
```

Dificuldade exibida em APIs = `work(target)` como `f64` (informativo).

## 8. Ajuste de dificuldade — LWMA-1

🔒 Recalculado **a cada bloco** (`core/src/difficulty.rs`). Parâmetros de
mainnet/testnet: `T = target_block_time = 15`, `N = lwma_window = 120`,
`LWMA_MIN_WINDOW = 6` (regtest não reajusta, §20).

Seja `next_height` a altura do bloco sendo validado e `ancestors` a lista de
cabeçalhos do mesmo ramo, do mais antigo ao mais novo, terminando no **pai**
(o nó passa até `N + 1 = 121` ancestrais; perto do gênese, todos os disponíveis).

```
função next_target(next_height, ancestors):
    se retarget == false ou len(ancestors) < 2 ou next_height < 2 → retorna genesis_target

    // aquecimento: usa os tempos de bloco disponíveis até existir a janela completa
    n = min(N, next_height − 1, len(ancestors) − 1)
    se n < LWMA_MIN_WINDOW (6)           → retorna genesis_target

    window = últimos n+1 elementos de ancestors        // window[0] .. window[n]
    k = n × (n + 1) × T / 2                            // n = 120, T = 15: 108 900
    prev_ts = window[0].timestamp
    weighted = 0 (u64)
    sum_targets = 0 (U512)
    para j = 1..n:
        this_ts   = max(window[j].timestamp, prev_ts + 1)
        solvetime = min(this_ts − prev_ts, 6 × T)
        prev_ts   = this_ts
        weighted += solvetime × j
        sum_targets += window[j].target
    next = floor( sum_targets × weighted / (n × k) )    // aritmética inteira em 512 bits

    // limite de ±2× por bloco em relação ao alvo do pai
    prev = window[n].target
    se next < floor(prev / 2) → next = floor(prev / 2)
    se next > prev × 2        → next = prev × 2

    se next > pow_limit → next = pow_limit
    se next == 0        → next = 1
    retorna next (U256)
```

* Com retarget ativo, os blocos de altura **1 a 6** usam `genesis_target`; a
  partir da altura 7 o LWMA já ajusta (com janela crescente até 120 blocos).
* O limite de ±2× suaviza a janela curta do aquecimento e limita o efeito de
  timestamps manipulados.
* `pow_limit = U256::MAX >> pow_limit_shift` (alvo mais fácil permitido).
* `genesis_target = U256::MAX >> genesis_target_shift`.
* O campo `target` do cabeçalho deve ser **exatamente** igual a `next_target`.

## 9. Regras de tempo

🔒 (`node/src/chain.rs::check_header`, `core/src/difficulty.rs`)

* **MTP (median time past):** pegue os timestamps dos últimos
  `min(11, disponíveis)` blocos terminando no pai, ordene e tome o elemento de
  índice `len/2`. Regra: `header.timestamp > MTP`.
* **Deriva futura:** `header.timestamp <= now_local + max_future_drift`
  (180 s em todas as redes). Esta regra depende do relógio local — mantenha o
  NTP ativo. Um bloco rejeitado por estar no futuro não é marcado como inválido
  permanentemente e pode ser aceito mais tarde.

## 10. Emissão e recompensas

🔒 (`core/src/emission.rs`, `core/src/execution.rs`)

```
subsidy(0)      = 0                                         // gênese sem recompensa
subsidy(h >= 1) = initial_reward >> ((h − 1) / halving_interval)   (0 se shift >= 64)
```

Mainnet/testnet: `initial_reward = 10 TCN`, `halving_interval = 2 500 000` blocos
(≈ 434 dias a 15 s por bloco). Alturas `1..=2 500 000` pagam 10 TCN,
`2 500 001..=5 000 000` pagam 5 TCN, etc. Emissão total = 49 999 999,99… TCN
< 50 000 000 TCN. As 4 primeiras eras (≈ 4,76 anos) emitem 93,75 % do supply.

> 10 TCN a cada 15 s são os mesmos **40 TCN por minuto** do cronograma antigo de
> 60 s, e o intervalo de halving é 4× maior — a curva **no tempo** e o teto de
> 50 000 000 TCN são exatamente os mesmos de antes da mudança para 15 s.

**Recompensa do bloco** `R(h) = subsidy(h) − Σ uncle_reward + Σ (fee − burned)`
das transações do bloco (a sobretaxa de congestionamento `burned` é destruída,
§14.3; os tios recebem `uncle_reward` do próprio subsídio, §6.3). Tudo fica
**pendente** no estado num único registro por altura:

```rust
struct PendingReward { payouts: Vec<Payout> }         // o minerador primeiro, depois os tios
struct Payout { who: Address, amount: u64, released: u64 }
locked(r) = Σ (amount − released)
```

e é liberado em duas partes (**cooldown**), para cada `Payout`:

| Quando (início do bloco) | Libera | Mainnet/testnet | Regtest |
|---|---|---|---|
| `h + coinbase_maturity` | `floor(amount / 4)` (25 %) | +400 blocos (≈ 100 min) | +5 |
| `h + reward_unlock_blocks` | o restante `amount − released` | +4 000 blocos (≈ 16,7 h) | +12 |

`reward_unlock_blocks` (4 000) é maior que a reorganização máxima (2 880): a
maior parte da recompensa de um bloco que ainda pode ser reorganizado nunca está
gastável, o que desestimula ataques de reescrita por mineradores. Ver §15.

**Checagem de teto:** após somar o subsídio a `ChainGlobal.emitted`, se
`emitted > MAX_SUPPLY` o bloco é inválido.

## 11. Estado global

🔒 (`core/src/state.rs`) O estado é um mapa chave → valor (valores em Borsh).

| Prefixo | Chave (bytes) | Valor |
|---|---|---|
| `0x01` | `0x01 || address(20)` | `Account` |
| `0x02` | `0x02 || contract_id(32)` | `Contract` |
| `0x03` | `0x03 || proposal_id(32)` | `Proposal` |
| `0x04` | `0x04 || proposal_id(32) || voter(20)` | `VoteRecord` |
| `0x05` | `0x05 || height(8, **big-endian**)` | `PendingReward` |
| `0x06` | `0x06` | `ChainGlobal` |
| `0x07` | `0x07 || program_address(20)` | `ProgramMeta` |
| `0x08` | `0x08 || program_address(20)` | programa TCCL compilado: `borsh(Program)` |
| `0x09` | `0x09 || program_address(20) || chave_local` | valor do armazenamento do contrato (§17.4) |
| `0x0A` | `0x0A || program_address(20)` | `ProgramAdmin` — quem pode atualizar o código (§17.11). **Registro ausente = contrato final** |

Todos os registros — inclusive código, metadados e armazenamento de contratos
TCCL — entram no compromisso de estado (§12).

### 11.1 Registros

```rust
struct Account {            // 32 bytes
    balance: u64,           // saldo total (inclui locked)
    nonce: u64,
    locked: u64,            // bloqueado por votos...
    locked_until: u64,      // ...até esta altura (inclusive)
}
// gasto disponível na altura h:
spendable(h) = if h <= locked_until { balance.saturating_sub(locked) } else { balance }
// Uma conta com balance == 0 && nonce == 0 NÃO é armazenada (a chave é removida).
// Contas com nonce > 0 são mantidas para sempre (proteção de replay).
// O saldo em TCN de um contrato TCCL é a Account no endereço do contrato.

struct PendingReward {
    payouts: Vec<Payout>,    // [0] = minerador do bloco (subsídio menos tios + taxas); depois um por tio
}
struct Payout {
    who: Address,
    amount: u64,
    released: u64,           // parte já liberada (o quarto inicial)
}
locked(r) = Σ (amount − released)

struct ChainGlobal {
    emitted: u64,
    burned: u64,                             // depósitos de propostas sem quórum + sobretaxas de congestionamento
    params: GovParams,                       // 12 × u64 na ordem de GovParamId
    voting: Vec<(u8, Hash32)>,               // (bit de sinal, id) das propostas em votação
    pending_activations: Vec<(u64, Hash32)>, // (altura de ativação, id)
    proposal_count: u64,
    contract_count: u64,                     // contratos nativos + contratos TCCL implantados com sucesso
    congestion_bp: u64,                      // multiplicador de congestionamento (10 000 = 1,0×)
}
circulating = emitted − burned (saturating)

struct GovParams {
    max_block_bytes, max_block_fuel, base_fee, fee_per_kb, fee_per_kfuel,
    storage_deposit_per_kb, proposal_deposit, vote_period,
    quorum_bp, approval_bp, miner_approval_bp, activation_delay      // todos u64
}

struct Contract {
    id: Hash32, creator: Address, created_height: u64,
    balance: u64,
    deposit: u64,            // depósito de armazenamento reembolsável (§16.2)
    state: ContractState,
}
enum ContractState {                                         // 🧩
    0 Escrow       { payer: Address, payee: Address, arbiter: Option<Address>, deadline_height: u64 },
    1 Vesting      { beneficiary: Address, total: u64, claimed: u64, start_height: u64,
                     cliff_height: u64, end_height: u64, revocable: bool },
    2 Subscription { payer: Address, payee: Address, amount_per_period: u64, period_blocks: u64,
                     max_periods: u32, start_height: u64, claimed_periods: u32 },
    3 Htlc         { sender: Address, recipient: Address, hash_lock: Hash32, timeout_height: u64 },
    4 Multisig     { signers: Vec<Address>, threshold: u8, next_spend_id: u32, pending: Vec<PendingSpend> },
}
struct PendingSpend { id: u32, to: Address, amount: u64, memo: Vec<u8>, approvals: Vec<Address>, created_height: u64 }

struct ProgramMeta {
    creator: Address,
    created_height: u64,
    deploy_txid: Hash32,     // transação que implantou (o código-fonte está nela)
    source_hash: Hash32,     // tagged_hash("tccl-source", source)
    name: String,            // nome declarado em `contract Nome`
    state_bytes: u64,        // bytes do código compilado + Σ (chave_local + valor) do armazenamento
    storage_items: u64,      // número de entradas de armazenamento
    deposit: u64,            // depósito reembolsável mantido por state_bytes
}

struct ProgramAdmin {        // registro 0x0A; AUSENTE = contrato final (§17.11)
    authority: Option<Address>,   // quem pode trocar o código
    code_version: u32,            // 1 no deploy, +1 a cada Upgrade
    language: u16,                // versão da linguagem TCCL do código guardado
    previous_code_hash: Hash32,   // code_hash anterior ao último Upgrade (zeros no deploy)
}

struct Proposal {
    id: Hash32, proposer: Address, spec: ProposalSpec, deposit: u64,
    created_height: u64, end_height: u64, signal_bit: u8,
    tally: Tally, status: ProposalStatus, outcome: Option<Outcome>,
}
struct Tally { yes: u64, no: u64, abstain: u64, voters: u64, miner_yes_blocks: u64, miner_total_blocks: u64 }
enum ProposalStatus { 0 Voting, 1 Approved { activation_height: u64 }, 2 Rejected, 3 Activated { height: u64 } }
struct Outcome { quorum_reached: bool, holders_approved: bool, miners_approved: bool,
                 circulating_at_end: u64, deposit_refunded: bool }
struct VoteRecord { choice: VoteChoice, weight: u64, height: u64 }
```

**Invariante de supply** (verificado nos testes a cada bloco):

```
Σ Account.balance
  + Σ (Contract.balance + Contract.deposit)
  + Σ ProgramMeta.deposit
  + Σ (Payout.amount − Payout.released) de todos os PendingReward
  + Σ deposit das propostas com status Voting
  ==  emitted − burned
```

## 12. Compromisso de estado — LtHash16

🔒 (`core/src/lthash.rs`) Hash homomórfico de multiconjunto: um vetor de
**2048 elementos `u16`**, somados módulo 2^16.

```
element(key, value):
    X = BLAKE3 em modo XOF sobre:
        "TheCoin:lthash16" || 0x00
        || u32_le(len(key))   || key
        || u32_le(len(value)) || value
    ler 4096 bytes de X; elemento[i] = u16_le(bytes[2i], bytes[2i+1])

insert(key, value): L[i] = L[i] + element[i]  (mod 2^16)
remove(key, value): L[i] = L[i] − element[i]  (mod 2^16)
to_bytes(L)      = concatenação dos 2048 u16 em little-endian (4096 bytes)
state_root       = tagged_hash("state-root", to_bytes(L))
```

Observação: o prefixo do XOF é o literal `"TheCoin:lthash16\0"` — **não** passa
por `tagged_hash`.

O LtHash do estado é a soma de `element(k, v)` para todos os registros da §11
(todos os prefixos `0x01`–`0x0A`). Ao aplicar um bloco, para cada chave
alterada: `remove(k, old)` se existia e `insert(k, new)` se continua existindo.
O `state_root` do cabeçalho deve ser igual ao valor calculado **depois** de
aplicar o bloco inteiro. Escritas que não mudam o valor não alteram o hash.
Recibos de transação (§17.9) **não** fazem parte do estado.

## 13. Árvore de Merkle

🔒 (`core/src/merkle.rs`) Sobre a lista ordenada de `txid`s do bloco:

```
se vazia:   root = tagged_hash("merkle-empty")
folhas:     L_i  = tagged_hash("merkle-leaf", txid_i)
nível:      pares (a, b) → tagged_hash("merkle-node", a || b)
            elemento ímpar no fim do nível é PROMOVIDO sem alteração (sem duplicar)
repetir até restar 1 → root
```

`header.tx_root` deve ser igual à raiz dos txids na ordem do bloco.

## 14. Validação de transações e taxas

(`core/src/execution.rs`)

### 14.1 Checagens independentes de estado 🔒

1. `size <= MAX_DEPLOY_TX_BYTES = 64 000` **apenas** para `Deploy`;
   `size <= MAX_TX_BYTES = 16 384` para todas as demais ações — inclusive
   `Upgrade`, que também carrega código-fonte e portanto só aceita contratos
   cujo fonte caiba em ~16 kB.
2. `version == 1`.
3. `chain_id == params.chain_id`.
4. `flags & !0x03 == 0` (§5.2).
5. Por ação:
   * `Transfer`: `amount > 0`; `len(memo) <= 256`.
   * `BatchTransfer`: `1 <= len(outputs) <= 128`; todo `amount > 0`; `len(memo) <= 256`; soma sem overflow.
   * `CreateContract`: `spec.validate_static()` (§16).
   * `CallContract`: `HtlcRedeem.preimage <= 64 bytes`; `MultisigDeposit.amount > 0`; `MultisigPropose.amount > 0` e `memo <= 256`.
   * `Propose`: `title` não vazio após `trim`, `len(title) <= 120` bytes; `len(url) <= 256`; em `SoftwareUpgrade`, `1 <= len(version) <= 32`.
   * `Vote`: `weight > 0`.
   * `Deploy`: `source.trim()` não vazio; `1 <= max_fuel <= MAX_TX_FUEL = 10 000 000`; `len(init_args) <= 32`.
   * `Invoke`: `1 <= len(function) <= 64` bytes; `1 <= max_fuel <= 10 000 000`; `len(args) <= 32`.
   * `Upgrade`: `source.trim()` não vazio; `1 <= max_fuel <= 10 000 000`; `len(args) <= 32`.
   * `SetUpgradeAuthority`: nenhuma checagem estática (tudo depende do estado, §17.11).
6. Assinatura Ed25519 estrita sobre `signing_hash`.

### 14.2 Aplicação (dependente de estado) 🔒

Na altura `h` do bloco, com `g = ChainGlobal` corrente:

1. Se `expiry_height != 0` e `h > expiry_height` → inválida.
2. `(required, base) = required_fee(g.params, g.congestion_bp, size, max_fuel)` (§14.3);
   `fee >= required`, senão inválida. `burned = required − base`.
3. `nonce == conta(sender).nonce`.
4. `debit` =
   * `Transfer`: `amount`; `BatchTransfer`: `Σ amount`;
   * `CreateContract`: `funding(spec)` (§16);
   * `CallContract`: `amount` se `MultisigDeposit`, senão 0;
   * `Propose`: `g.params.proposal_deposit`; `Vote`: 0;
   * `Deploy` / `Invoke`: `value`;
   * `Upgrade` / `SetUpgradeAuthority`: 0 (o depósito de armazenamento de um
     `Upgrade` é cobrado dentro da execução, §17.7).
5. `debit + fee <= spendable(h)`, senão inválida. Então `nonce += 1` e:
   * ações nativas: `balance −= debit + fee`;
   * `Deploy` / `Invoke` / `Upgrade`: `balance −= fee` apenas (o `value` e o
     depósito de armazenamento são movidos dentro da execução reversível, §17).
6. Executa a ação (crédito a destinatários, criação/chamada de contrato,
   proposta, voto, execução TCCL). Créditos criam a conta se não existir.
7. **Ações nativas** (inclusive `SetUpgradeAuthority`, que não executa código):
   se qualquer passo falhar, a transação é **inválida** e o bloco que a contém é
   inválido. **`Deploy`/`Invoke`/`Upgrade`:** se as checagens 1–5
   passam, a transação é **sempre incluível**: uma falha do código do contrato
   (require, falta de combustível, erro de compilação, depósito acima de
   `max_deposit`…) mantém a taxa e o nonce, reverte todo o resto e é registrada
   no recibo com `success = false` (§17.8).

A taxa não é creditada ao destinatário: `fee − burned` soma-se à recompensa
pendente do bloco (§10) e `burned` é somado a `ChainGlobal.burned` (§15).

### 14.3 Taxa mínima e congestionamento 🔒

```
required_fee(params, congestion_bp, size, max_fuel):
    base     = params.base_fee
             + ceil(size     × params.fee_per_kb    / 1000)
             + ceil(max_fuel × params.fee_per_kfuel / 1000)          // em u128
    required = ceil(base × max(congestion_bp, 10 000) / 10 000)
    retorna (min(required, u64::MAX), min(base, u64::MAX))
```

* `size` é o tamanho serializado da transação inteira; `max_fuel` é o
  combustível **reservado** (não o usado — combustível não usado não é
  devolvido, por isso carteiras medem o consumo por simulação).
* `required − base` (a **sobretaxa de congestionamento**) é **queimada**; o
  restante da taxa, incluindo qualquer valor acima de `required` (gorjeta de
  prioridade), vai para o minerador. Assim o minerador não ganha nada enchendo
  blocos com transações próprias para subir as taxas.

Exemplo (mainnet, 1,0×): transferência de 163 bytes →
`1 000 + ceil(163 × 10 000 / 1 000) = 2 630` motes. Com `congestion_bp = 20 000`:
`required = 5 260`, dos quais 2 630 queimados.

**Atualização do multiplicador** (no fim de cada bloco, §15), com os limites
`max_block_bytes`/`max_block_fuel` **em vigor durante o bloco**:

```
next_congestion(cur, usage, params):
    fill(u, m) = min(u × 10 000 / max(m, 1), 10 000)                 // inteiro (u128)
    fill_bp = max(fill(usage.bytes, params.max_block_bytes),
                  fill(usage.fuel,  params.max_block_fuel))
    cur = max(cur, 10 000)
    se fill_bp > 5 000: next = cur + max(cur × (fill_bp − 5 000) / 5 000 / 8, 1)
    se fill_bp < 5 000: next = cur − cur × (5 000 − fill_bp) / 5 000 / 8   (saturating)
    se fill_bp = 5 000: next = cur
    retorna clamp(next, CONGESTION_MIN_BP = 10 000, CONGESTION_MAX_BP = 10 000 000)
```

`usage.bytes` = tamanho serializado do bloco (inclui os 252 bytes fixos de um
bloco vazio e 244 bytes por tio, §6.1);
`usage.fuel` = `Σ max_fuel` das transações. Alvo: blocos 50 % cheios. O
multiplicador sobe no máximo 12,5 % por bloco (bloco 100 % cheio), desce no
máximo 12,5 % (bloco vazio) e fica entre 1× e 1000×. O gênese começa em 10 000.

## 15. Processamento de bloco

🔒 Ordem exata de `apply_block` (após as checagens de cabeçalho da §19):

1. `g = ChainGlobal` atual.
2. `len(borsh(Block)) <= g.params.max_block_bytes` (e sempre `<= 8 000 000`).
3. `Σ max_fuel das transações <= g.params.max_block_fuel` (soma sem overflow), senão inválido.
4. `mask` = OR de `1 << bit` para cada `(bit, _)` em `g.voting`.
   `header.signal & !mask` deve ser 0 (bits não atribuídos devem ser zero).
   A máscara é calculada **antes** das transações do bloco.
4b. **Tios:** `len(uncles) <= 2`, `uncles_root` confere, nenhum tio repetido e
   `1 <= h − uncle.height <= 6` (as demais regras da §6.3 — trabalho, alvo,
   ancestralidade e não repetição entre blocos — são checadas pelo gerenciador
   de cadeia antes de aplicar o corpo, §19.1).
5. **begin (cooldown de recompensas):**
   1. se `h > coinbase_maturity`: leia `PendingReward` em
      `0x05 || u64_be(h − coinbase_maturity)`; para cada `Payout` com
      `released == 0` e `floor(amount/4) > 0`: credite `floor(amount/4)` a
      `payout.who` e grave `released = floor(amount/4)`;
   2. se `h > reward_unlock_blocks`: leia `PendingReward` em
      `0x05 || u64_be(h − reward_unlock_blocks)`; se existir: apague o registro e
      credite `amount − released` a cada `payout.who`.
6. **transações**, em ordem. `txid` duplicado no bloco → inválido. Cada uma:
   checagens §14.1 e aplicação §14.2. `fees += fee − burned`; `burned_total += burned`.
7. **end:**
   1. `params_before = g.params` (valores em vigor durante o bloco).
   2. Governança (§18.4): contagem de sinais, fechamento de votações, ativações.
   3. `emitted += subsidy(h)`; se `emitted > MAX_SUPPLY` → inválido.
   4. `burned += burned_total`.
   5. `congestion_bp = next_congestion(congestion_bp, {bytes: len(borsh(Block)), fuel: Σ max_fuel}, params_before)`.
   6. **Pagamentos** (§6.3, §10): `to_uncles = Σ uncle_reward(subsidy(h), depth)`
      (pagamentos de valor 0 não geram entrada); se `to_uncles > subsidy(h)` →
      bloco inválido. Grava em `0x05 || u64_be(h)` um `PendingReward` cujos
      `payouts` são `[{ miner, subsidy(h) − to_uncles + fees }]` (só se esse
      valor for > 0) seguidos de um `Payout` por tio pago. Se não houver nenhum
      pagamento, nada é gravado.
8. `state_root` calculado (§12) deve ser igual a `header.state_root`.

## 16. Contratos de pagamento

🔒 (`core/src/contracts.rs`)

```
contract_id = tagged_hash("contract-id", creator(20) || u64_le(nonce_da_tx_de_criação))
```

**Funding** (debitado do criador e colocado em `Contract.balance`):
`Escrow/Vesting/Htlc: amount`; `Subscription: amount_per_period × max_periods`
(overflow → inválido); `Multisig: initial_deposit`.

### 16.1 Validação estática

| Tipo | Regras |
|---|---|
| Escrow | `amount > 0` |
| Htlc | `amount > 0` |
| Vesting | `amount > 0`; `start <= cliff <= end` e `start < end` |
| Subscription | `amount_per_period > 0`; `period_blocks >= 1`; `1 <= max_periods <= 100 000`; funding sem overflow |
| Multisig | `1 <= len(signers) <= 16`; `1 <= threshold <= len(signers)`; sem signatários duplicados |

### 16.2 Criação (na altura `h`)

| Tipo | Regras adicionais | Estado inicial |
|---|---|---|
| Escrow | `payee != criador`; `arbiter ∉ {payee, criador}`; `deadline_height > h` | `payer = criador` |
| Vesting | `end_height > h` | `total = amount`, `claimed = 0` |
| Subscription | `payee != criador` | `payer = criador`, `start_height = h`, `claimed_periods = 0` |
| Htlc | `timeout_height > h` | `sender = criador` |
| Multisig | — | `next_spend_id = 0`, `pending = []` |

Colisão de id (id já existente) → inválido. `ChainGlobal.contract_count += 1`.

**Depósito de armazenamento** (após montar o registro com `deposit = 0`):

```
storage_deposit(bytes, per_kb) = ceil(bytes / 1000) × per_kb                 (saturating)
record_bytes     = len(borsh(Contract)) + 33          // 33 = tamanho da chave 0x02 || id
Contract.deposit = storage_deposit(record_bytes, g.params.storage_deposit_per_kb)
```

Se `deposit > 0`: exige `deposit <= spendable(h)` do criador (já descontados
funding e taxa), senão a transação é inválida; debita `deposit` do saldo. O
valor fica fixo durante a vida do contrato.

### 16.3 Chamadas (altura `h`, `caller` = remetente)

Chamada que não corresponde ao tipo do contrato → inválida. Contrato inexistente → inválida.

| Chamada | Quem pode | Condição | Pagamentos |
|---|---|---|---|
| `EscrowRelease` | payer ou arbiter | — | `balance` → payee |
| `EscrowRefund` | payee, arbiter, ou payer se `h > deadline_height` | — | `balance` → payer |
| `VestingClaim` | beneficiary | `vested(h) − claimed > 0` | essa diferença → beneficiary; `claimed += x` |
| `VestingRevoke` | criador, só se `revocable` | — | `vested(h) − claimed` → beneficiary; restante do `balance` → criador |
| `SubscriptionClaim` | payee | `available(h) − claimed_periods > 0` | `p × amount_per_period` → payee; `claimed_periods += p` |
| `SubscriptionCancel` | payer | — | `(available(h) − claimed) × amount_per_period` → payee; restante → payer |
| `HtlcRedeem{preimage}` | qualquer um | `h <= timeout_height` e `SHA-256(preimage) == hash_lock` | `balance` → recipient |
| `HtlcRefund` | qualquer um | `h > timeout_height` | `balance` → sender |
| `MultisigDeposit{amount}` | qualquer um | — | `balance += amount` (debitado do caller) |
| `MultisigPropose{to,amount,memo}` | signatário | `len(pending) < 16` | cria `PendingSpend{id = next_spend_id, approvals = [caller]}`, `next_spend_id += 1`. Se `threshold <= 1`: paga na hora (exige `amount <= balance`) e não fica pendente |
| `MultisigApprove{spend_id}` | signatário que ainda não aprovou | spend existe | adiciona aprovação; ao atingir `threshold`: exige `amount <= balance` (senão a tx é inválida), paga `to` e remove o pendente |
| `MultisigCancel{spend_id}` | proponente (`approvals[0]`) | spend existe | remove o pendente |
| `MultisigClose` | signatário | `balance == 0` e `pending` vazio | encerra o cofre (ver remoção) |

Fórmulas:

```
vested(total, start, cliff, end, h) =
    0                                         se h < cliff ou h <= start
    total                                     se h >= end
    floor(total × (h − start) / (end − start))  (em u128)

available(start, period, max, h) =
    0                                  se h < start
    min(max, (h − start) / period + 1)       // 1º período disponível na criação
```

Pagamentos de valor zero são ignorados. `balance −= Σ pagamentos` (nunca negativo).

**Remoção e reembolso 🔒:** após a chamada, o contrato está *concluído* se:
`Multisig` → a chamada foi `MultisigClose`; `Subscription` →
`balance == 0` ou `claimed_periods >= max_periods`; demais tipos → `balance == 0`.
Se concluído **e** `balance == 0`: o registro é **apagado** e, se
`deposit > 0`, `deposit` é creditado ao **criador**. Caso contrário o registro é
regravado. Um `Multisig` só é apagado por `MultisigClose`.

## 17. Contratos inteligentes TCCL

🔒 (`core/src/programs.rs`, crate `tccl`) A linguagem **TCCL** (The Coin Cloud
Language) e sua VM determinística fazem parte do consenso: as transações
`Deploy` e `Upgrade` carregam o **código-fonte**, e todo nó o compila de forma
idêntica. A referência completa da linguagem, dos tipos, das funções embutidas e
da tabela de combustível está em [docs/tccl/TCCL.md](tccl/TCCL.md); aqui ficam as
regras de integração com a cadeia.

> **A TCCL agora é um repositório separado:** <https://github.com/LucasBolla94/tccl>,
> fixado no `Cargo.toml` da The Coin na **tag `v0.3.0`**. A tag faz parte do
> consenso na prática: dois nós só compilam o mesmo fonte para o mesmo
> `borsh(Program)` se usarem a mesma versão do compilador.

### 17.1 Endereço e identificação

```
program_address(creator, nonce) = tagged_hash("program-address", creator(20) || u64_le(nonce))[0..20]
source_hash(source)             = tagged_hash("tccl-source", source (UTF-8))
```

`nonce` é o nonce da transação `Deploy`. O saldo do contrato é a `Account` nesse
endereço (qualquer um pode enviar TCN a ele com `Transfer`).

### 17.2 Compilação e limites

* `compile(source, address_prefixes = [HRP da rede], version = LANGUAGE_VERSION)`
  produz um `Program` totalmente resolvido e verificado por tipos
  (`tccl::compile`). O formato `borsh(Program)` é consenso e fica em
  `0x08 || endereço`.
* **`LANGUAGE_VERSION = 2`** (`MIN_LANGUAGE_VERSION = 1`): novos deploys
  compilam na versão 2 — registros, enums com transições, papéis (*roles*),
  interfaces, módulos, biblioteca padrão e preços de combustível fixos. Programas
  já implantados na versão 1 continuam rodando exatamente como na v0.2.0.
* **Chamadas entre contratos** (versão 2): um contrato chama outro por uma
  interface, dentro da **mesma transação atômica**. O chamado vê o contrato
  chamador como `caller` (o `origin` continua sendo quem assinou), o combustível
  é compartilhado, qualquer falha aborta a transação inteira, e **reentrância é
  sempre recusada** (um contrato que já está rodando na transação não pode ser
  chamado de novo — erro `R018`). Profundidade máxima de contratos
  `MAX_CONTRACT_DEPTH = 8`.
* Código-fonte ≤ 48 000 bytes (limite do analisador léxico); programa compilado
  ≤ `MAX_PROGRAM_BYTES = 262 144` bytes; até 256 funções, 256 variáveis de
  estado, 1 024 locais por função; aninhamento de blocos, expressões e tipos
  ≤ 32; no máximo 64 operadores encadeados por nível de precedência e árvore de
  expressão com profundidade ≤ 128 (fontes que excedem são erro de compilação);
  profundidade de chamada `MAX_CALL_DEPTH = 16`, listas até 4 096 itens e valores
  até 65 536 bytes; memória viva de uma execução ≤ 16 MiB
  (`MAX_MEMORY_BYTES`); até `MAX_EVENTS_PER_CALL = 64` eventos por chamada.

### 17.3 Combustível (*fuel*)

* **Compilação** (Deploy): `5 × len(source)` (`COMPILE_PER_BYTE`).
* **Carga** (Invoke): `100 + floor(len(código_compilado) / 100)`.
* **Execução**: tabela `tccl::vm::fuel` (calibrada para ≈ 20 ns de CPU por
  unidade numa VPS de 2 vCPU: um bloco cheio de 50 M executa em ≈ 1 s no pior caso) —
  instrução 2, expressão 1, chamada 20, leitura de armazenamento 250,
  escrita `400 + 4/byte` (chave + valor), `sha256`/`blake3` `60 + 20 por 64 bytes`,
  `verify_ed25519` `3 500 + 1 por 64 bytes`, `ring_verify` `5 000 + 10 000 por membro`,
  `send` 300, `emit` `100 + tamanho`, `destroy` 1 000 (detalhes em [TCCL.md](tccl/TCCL.md)).
* O limite total é `max_fuel` da transação. Esgotar o combustível é uma falha do
  contrato e o `fuel_used` registrado é o limite inteiro da etapa.
* A **taxa** é cobrada sobre `max_fuel` reservado (§14.3), independentemente do uso.

### 17.4 Armazenamento

Chave de estado: `0x09 || endereço(20) || chave_local`. Chaves locais geradas pela VM
(`var` = índice `u16` big-endian da variável de estado):

| Chave local | Valor |
|---|---|
| `0x00 || var` | escalar: `borsh(Value)`; valor igual ao padrão do tipo **apaga** o registro |
| `0x01 || var || borsh(chave: Value)` | item de mapa: `borsh(Value)`; valor padrão apaga |
| `0x02 || var` | comprimento de lista: `u64` **big-endian** |
| `0x03 || var || u64_be(índice)` | item de lista: `borsh(Value)` |

`ProgramMeta.state_bytes` = `len(código compilado)` + `Σ (len(chave_local) + len(valor))`
de todas as entradas; `storage_items` = número de entradas. Toda escrita atualiza
os dois contadores (subtrai a entrada antiga, soma a nova).

### 17.5 `Deploy` (altura `h`, após §14.2)

```
compile_fuel = 5 × len(source)
se compile_fuel > max_fuel            → falha "out of fuel while compiling" (fuel_used = max_fuel)
program = compile(source)             → erro: falha (fuel_used = compile_fuel)
code = borsh(program); len(code) > 262 144 → falha
addr = program_address(sender, nonce)
se existe ProgramMeta em addr ou Account(addr).nonce > 0 → falha "contract address collision"
meta = { creator: sender, created_height: h, deploy_txid: txid, source_hash, name: program.name,
         state_bytes: len(code), storage_items: 0, deposit: 0 }
em um overlay filho:
    se flags & FLAG_FINAL_DEPLOY == 0:                      // contrato atualizável
        grava 0x0A || addr → ProgramAdmin { authority: Some(sender), code_version: 1,
                                            language: program.version, previous_code_hash: 0…0 }
    grava 0x08 || addr → code
    run(Mode::Deploy, "init", init_args, fuel = max_fuel − compile_fuel)     // §17.7
sucesso → aplica o overlay; fuel_used += compile_fuel; receipt.program = addr; contract_count += 1
falha   → descarta o overlay;  fuel_used += compile_fuel
```

Com `FLAG_FINAL_DEPLOY` (§5.2) **nenhum** registro `0x0A` é criado: o contrato
nasce final e seu código nunca poderá mudar.

No modo Deploy a VM grava os valores iniciais constantes das variáveis de estado
e chama `init(init_args)` se existir. Sem `init`, `init_args` deve ser vazio e
`value` deve ser 0; com `init` não `payable`, `value` deve ser 0.

### 17.6 `Invoke` (altura `h`, após §14.2)

```
se não existe ProgramMeta em contract → falha "no contract at <hex>" (fuel_used = 0)
load = 100 + floor(len(code) / 100)
se load > max_fuel                    → falha (fuel_used = max_fuel)
em um overlay filho: run(Mode::Action, function, args, fuel = max_fuel − load)
sucesso → aplica o overlay; fuel_used += load
falha   → descarta o overlay;  fuel_used += load
```

A função deve existir e ser `action` (views e funções internas não são chamáveis
por transação); o número e os tipos dos argumentos devem bater com a
declaração; `value > 0` exige função `payable`.

### 17.7 `run`: valor, execução e depósito

```
run(mode, function, args, fuel):
    old_bytes = (mode == Deploy) ? 0 : meta.state_bytes
    se value > 0:
        value > spendable(h) do sender → falha; senão sender −= value; contrato += value
    contexto = { caller: sender, value, height: h, self_address: addr }
    executa a VM com o combustível dado; erro → falha
    se o contrato chamou destroy(to):
        total = Account(addr).balance + meta.deposit
        Account(addr).balance = 0; apaga 0x07||addr e 0x08||addr
        credita total a `to`
        (a VM só chega aqui com o armazenamento vazio: `destroy` apaga as variáveis
         escalares e falha com StorageNotEmpty se ainda houver entradas de listas/mapas,
         portanto não restam registros 0x09||addr||…)
    senão, para CADA contrato tocado pela transação (versão 2 permite que um
    contrato chame outros, §17.2 — todos compartilham o MESMO max_deposit):
        new_bytes = meta.state_bytes
        se mode == Deploy ou new_bytes > old_bytes:
            required = ceil(new_bytes / 1000) × g.params.storage_deposit_per_kb
            extra = max(required − meta.deposit, 0)
            extra > max_deposit                → falha
            extra > spendable(h) do sender     → falha
            sender −= extra; meta.deposit += extra
        senão se new_bytes < old_bytes e old_bytes > 0 e meta.deposit > 0:
            refund = floor(meta.deposit × (old_bytes − new_bytes) / old_bytes)
            meta.deposit −= refund; credita refund ao sender (quem liberou espaço)
        grava 0x07||addr → meta
```

O depósito é **reembolsável**: quem faz o contrato crescer paga a diferença (até
`max_deposit`), quem libera armazenamento recebe a parte proporcional, e
`destroy` devolve saldo e depósito inteiros.

### 17.8 Falha

Uma falha descarta o overlay filho: valor enviado, escritas, `send`s, eventos e
depósitos são revertidos. Permanecem apenas a taxa e o incremento de nonce
(§14.2). O recibo registra `success = false`, `error` (mensagem) e `fuel_used`.
Falhas de armazenamento do nó (não do contrato) abortam o bloco inteiro.

### 17.9 Recibos (não é consenso)

O nó grava para cada transação confirmada (`core/src/execution.rs::TxReceipt`):

```rust
struct TxReceipt {
    txid: Hash32, fee: u64, burned: u64,
    touched: Vec<Address>,          // remetente primeiro, depois endereços afetados (ordenados, sem repetição)
    created: Option<Hash32>,        // contrato nativo ou proposta criado
    program: Option<Address>,       // contrato TCCL implantado com sucesso
    success: bool, error: Option<String>, fuel_used: u64,
    logs: Vec<LogEntry>,            // LogEntry { contract: Address, event: String, fields: Vec<(String, Value)> }
    return_value: Option<Value>,    // None quando a função não retorna valor
}
```

Recibos são dados derivados (tabela `receipts`, expostos em `GET /api/v1/tx`);
não entram no `state_root` e mensagens de erro podem mudar entre versões.
*Views* (`POST /api/v1/program/{addr}/view`) também não são consenso.

### 17.10 `ring_verify` (assinaturas em anel)

Função embutida usada por contratos de privacidade (`tccl/src/ring.rs`):
**bLSAG** (Back's LSAG, como no Monero) sobre o grupo **Ristretto255**.

```
Hp(P)        = RistrettoPoint::hash_from_bytes::<SHA-512>("TheCoin:ring:hash-to-point\0" || P)
ring_hash    = SHA-512("TheCoin:ring:members\0" || P_0 || … || P_{n−1})
challenge(L, R) = Scalar::from_hash(SHA-512("TheCoin:ring:challenge\0" || ring_hash
                   || u64_le(len(msg)) || msg || I || L || R))
key_image  I = x · Hp(P)                     // único por chave secreta
assinatura   = c_0 || r_0 || … || r_{n−1}    // 32 × (n + 1) bytes

verify(msg, ring, sig, I):
    1 <= n <= 64; len(sig) == 32(n+1); todo P_i descomprime e não é identidade;
    I descomprime e não é identidade; escalares canônicos
    c = c_0
    para i = 0..n−1: c = challenge(r_i·G + c·P_i, r_i·Hp(P_i) + c·I)
    válido se c == c_0
```

Chaves de anel das carteiras: `x = Scalar::from_hash(SHA-512("TheCoin:ring:secret\0" || seed32))`,
`P = x·G` (derivação em [WALLET_DEVELOPERS.md](WALLET_DEVELOPERS.md#7-privacidade-pools-tccl-priv-1)).

## 18. Governança

🔒 (`core/src/governance.rs`)

```
proposal_id = tagged_hash("proposal-id", proposer(20) || u64_le(nonce_da_tx))
```

### 18.1 `Propose` (altura `h`)

1. Depósito `g.params.proposal_deposit` já debitado (§14.2).
2. `SetParam{param, value}`: `value` deve estar dentro de `gov_bounds[param]` (inclusivo).
3. `len(g.voting) < 32` (`MAX_CONCURRENT_PROPOSALS`).
4. `signal_bit` = menor bit `0..31` não usado por propostas em `g.voting`.
5. Colisão de id → inválido.
6. `end_height = h + g.params.vote_period`; status `Voting`; `g.voting.push((bit, id))`; `proposal_count += 1`.

### 18.2 `Vote{proposal, choice, weight}` (altura `h`)

1. Proposta existe, status `Voting`, e `created_height <= h <= end_height`
   (é permitido votar no próprio bloco da criação, desde que o `Vote` venha
   depois do `Propose` na ordem das transações).
2. Um voto por endereço por proposta (chave `0x04`).
3. `weight <= balance` (saldo total, **após** debitar a taxa).
4. Bloqueio:
   ```
   active_lock = if h <= locked_until { locked } else { 0 }
   locked       = max(active_lock, weight)
   locked_until = if active_lock > 0 { max(locked_until, end_height) } else { end_height }
   ```
   As mesmas moedas podem votar em propostas diferentes; o bloqueio é o
   máximo dos pesos até o maior `end_height` (pode bloquear um pouco a mais).
5. `tally.yes|no|abstain += weight`; `tally.voters += 1`; grava `VoteRecord { choice, weight, height: h }`.

### 18.3 Parâmetros e limites

Ver §20 para padrões e limites (`GovBounds`) por rede. **Não governáveis:**
supply máximo, curva de emissão, tempo de bloco, algoritmo de PoW, LWMA,
maturidade/cooldown de recompensas, profundidade máxima de reorg, limites
absolutos (`MAX_BLOCK_BYTES_HARD`, `MAX_TX_FUEL`, tamanhos de transação e de programa).

### 18.4 Processamento no fim do bloco (`end_block`, altura `h`)

Para cada `(bit, id)` em `g.voting` (na ordem da lista):

1. Se `h > created_height`: `miner_total_blocks += 1`; se `header.signal & (1 << bit) != 0`: `miner_yes_blocks += 1`.
2. Se `h >= end_height` — **apuração**:
   ```
   votes        = yes + no + abstain                      (u128)
   circulating  = g.emitted − g.burned      (ANTES do subsídio e da queima de taxas deste bloco)
   quorum       = votes × 10000 >= quorum_bp × circulating  E  votes > 0
   holders_ok   = yes > 0  E  yes × 10000 >= approval_bp × (yes + no)
   miners_ok    = miner_yes_blocks × 10000 >= miner_approval_bp × miner_total_blocks
   ```
   * Se `quorum`: depósito devolvido ao `balance` do proponente. Senão: `g.burned += deposit`.
   * `outcome = { quorum, holders_ok, miners_ok, circulating, deposit_refunded: quorum }`.
   * Se `quorum && holders_ok && miners_ok`: status `Approved { activation_height: h + g.params.activation_delay }` e `g.pending_activations.push((activation_height, id))`. Senão: `Rejected`.
   * Removida de `g.voting` (o bit fica livre a partir do próximo bloco).

Depois, **ativações**: ordene `g.pending_activations` (por altura, depois id);
para cada entrada com `activation_height <= h`:

* `SetParam{param, value}`: se `value` ainda está dentro dos limites, `g.params[param] = value` (senão é ignorado).
* `Text` / `SoftwareUpgrade`: apenas registrado.
* status `Activated { height: h }`; entrada removida.

Mudanças de parâmetro passam a valer no **bloco seguinte** (`end_block` roda
após as transações; o multiplicador de congestionamento deste bloco usa os
limites antigos). O subsídio/pendência do bloco vem depois da governança (§15).

As transições futuras de consenso (PoW → híbrido → PoS) seguem este mesmo
mecanismo; ver [ROADMAP.md](ROADMAP.md).

## 19. Seleção de cadeia

(`node/src/chain.rs`)

### 19.1 Checagens de cabeçalho 🔒 (antes de aplicar o corpo)

1. `version == 1`.
2. `height == pai.height + 1` (pai conhecido; senão o bloco é órfão).
3. `timestamp > MTP` e `timestamp <= now + 180` (§9).
4. `target == next_target(...)` (§8).
5. Checkpoints: se `height` é uma altura de checkpoint, o hash deve coincidir; se `height <= último checkpoint`, o pai deve estar na cadeia principal (sem forks abaixo do último checkpoint). *(A v0.2 não tem checkpoints definidos.)*
6. `len(borsh(Block)) <= 8 000 000`.
7. `tx_root` confere.
8. `CoinHash(header) <= target`.
9. Aplicação do corpo e `state_root` (§15). Descendentes de um bloco inválido são inválidos.

### 19.2 Regra de escolha

* A cadeia ativa é a de **maior trabalho acumulado** (`chainwork`).
* Um novo ramo só substitui a cadeia ativa se tiver trabalho **estritamente
  maior**; em empate vale o **primeiro visto**.
* `chainwork(bloco) = chainwork(pai) + work(bloco.target) + Σ work(tio.target)`
  — o trabalho dos tios incluídos conta (§6.3).
* **Finalidade:** um ramo que desconecte um bloco **final** (§19.3) é guardado
  como lateral e **não** é adotado, qualquer que seja o trabalho dele.
* **Profundidade máxima de reorganização: 2 880 blocos** na mainnet e testnet
  (≈ 12 h a 15 s; 720 na regtest). Um ramo mais pesado que exija desconectar
  mais blocos que isso é guardado como lateral e **não** é adotado (proteção
  contra ataques de reescrita profunda; nós isolados por mais tempo exigem
  intervenção manual).
* Se a aplicação de qualquer bloco do novo ramo falhar, a reorganização inteira
  é abortada atomicamente e os blocos do ramo a partir do inválido são marcados
  como inválidos.

### 19.3 Finalidade assinada pelos mineradores

(`core/src/finality.rs`, `node/src/finality.rs`, `node/src/node.rs`)

A prova de trabalho só diz que desfazer um bloco fica cada vez mais caro. A
finalidade acrescenta um sinal barato por cima dela: quando os mineradores
recentes assinam um bloco, ele passa a ser **irreversível** para os nós.

**Chave de voto.** Cada nó guarda uma chave Ed25519 em
`<data-dir>/finality.key` (criada no primeiro uso, permissão `0600`) e publica a
chave pública no campo `signer` dos blocos que minera (§6.1). `signer` com 32
zeros = o minerador não participa. O campo faz parte do cabeçalho 🔒; os votos
**não** entram em blocos — circulam só pela rede P2P (mensagem `FinalityVote`,
[P2P.md](P2P.md)).

```
FINALITY_WINDOW       = 200          // finality_window por rede (regtest: 20)
FINALITY_THRESHOLD_BP = 6 667        // 2/3
MIN_FINALITY_SIGNERS  = 4
MAX_VOTE_DISTANCE     = 64

vote_message(chain_id, height, block) = tagged_hash("finality-vote", u32_le(chain_id) || u64_le(height) || block)
FinalityVote { height: u64, block: Hash32, signer: [u8; 32], signature: [u8; 64] }   // Borsh
vote_id = tagged_hash("finality-vote-id", signer || u64_le(height) || block)       // só para relay
```

**Janela de um bloco `B`** (altura `h`): as chaves `signer` não nulas dos até
`FINALITY_WINDOW` blocos que antecedem `B` na cadeia (do pai de `B` para trás),
com repetição — quem minerou 30 blocos da janela pesa 30.

**Quem vota e em quê.** A cada nova ponta, o nó assina o **pai da ponta** (um
bloco que já tem um bloco de trabalho em cima, então não persegue blocos que
estão para perder a corrida), só se esse bloco é da sua cadeia principal, só se
a própria chave aparece na janela e **no máximo um voto por altura**. O voto é
contado localmente e retransmitido.

**Contagem** (para cada voto recebido, na ordem):

1. Chave banida → ignorado.
2. `height + 64 < altura da ponta` ou `height > ponta + 1` → longe demais.
3. `height <=` altura do último bloco final → já decidido.
4. `signer` não está na janela de `block` → sem peso.
5. Mesmo `signer` já votou em **outro** bloco nessa altura → **equivocação**: a
   chave é banida e todos os votos dela deixam de contar.
6. Assinatura inválida para o `chain_id` da rede → recusado.
7. Voto para um bloco ainda desconhecido fica guardado (até 512) e é contado
   quando o bloco chega.

```
peso(B)    = número de blocos da janela minerados pelas chaves que votaram em B
necessário = ceil(len(janela) × 6 667 / 10 000)                 // 134 com janela cheia
B é final  ⇔ peso(B) ≥ necessário
             e len(janela) == FINALITY_WINDOW                 // todos os 200 blocos com signer
             e a janela tem ≥ 4 chaves distintas
```

**Efeito.** O nó grava `(altura, hash)` do último bloco final no banco
(`finalized`, sobrevive a reinícios), expõe `finalized_height` em
`/api/v1/status` e `finalized` em `/api/v1/block/{id}` ([API.md](API.md)), e
passa a recusar qualquer ramo que desconecte esse bloco (§19.2).

**Propriedades.**

* Reverter um bloco final exige dois terços do poder de mineração recente —
  acima dos 51 % que já quebrariam a cadeia.
* Não trava a rede: sem votos a cadeia segue só pelo trabalho, sem a marca de
  final. Redes com menos de 4 mineradores distintos (ou com algum bloco sem
  `signer` na janela) simplesmente não finalizam.
* Não depende de quem tem moedas (não é stake).
* Num caminho normal um pagamento fica final cerca de **1 bloco** depois de
  minerado (≈ 30 s a 15 s por bloco); `/api/v1/security` informa
  `blocks_to_finality` quando a rede está finalizando.
* A finalidade é uma regra **local** de escolha de cadeia, não de validade de
  bloco: um nó que acabou de sincronizar do zero só conhece a finalidade dos
  votos que receber daí em diante.

## 20. Parâmetros por rede

(`core/src/params.rs`)

| Parâmetro | mainnet | testnet | regtest |
|---|---|---|---|
| magic (P2P) | `TCN1` | `TCNT` | `TCNR` |
| `chain_id` 🔒 | `0x54430001` | `0x54430002` | `0x54430003` |
| porta P2P / API | 7333 / 7334 | 17333 / 17334 | 27333 / 27334 |
| CoinHash: RandomX, `pow.epoch_blocks` 🔒 | 2 048 | 2 048 | 64 |
| `pow_limit_shift` 🔒 | 8 | 6 | 0 |
| `genesis_target_shift` 🔒 | 12 | 9 | 0 |
| retarget (LWMA) 🔒 | sim | sim | **não** |
| tempo de bloco / janela LWMA / janela mínima 🔒 | 15 s / 120 / 6 | 15 s / 120 / 6 | 60 s / 60 / 6 |
| deriva futura máxima | 180 s | 180 s | 180 s |
| `finality_window` (§19.3) | 200 | 200 | 20 |
| recompensa inicial 🔒 | 10 TCN | 10 TCN | 40 TCN |
| halving 🔒 | 2 500 000 | 2 500 000 | 150 |
| `coinbase_maturity` (libera 25 %) 🔒 | 400 | 400 | 5 |
| `reward_unlock_blocks` (libera o resto) 🔒 | 4 000 | 4 000 | 12 |
| reorg máxima | 2 880 | 2 880 | 720 |
| timestamp gênese | 1 789 257 600 | 1 789 257 600 | 1 789 257 600 |
| seeds | `seed1/seed2.the-coin.cloud:7333` | `testnet-seed1/2.the-coin.cloud:17333` | — |

Constantes comuns 🔒: `MAX_BLOCK_BYTES_HARD = 8 000 000`, `MAX_TX_BYTES = 16 384`,
`MAX_DEPLOY_TX_BYTES = 64 000`, `MAX_TX_FUEL = 10 000 000`,
`MAX_PROGRAM_BYTES = 262 144`, `MAX_EVENTS_PER_CALL = 64`,
`CONGESTION_MIN_BP = 10 000`, `CONGESTION_MAX_BP = 10 000 000`,
`LWMA_MIN_WINDOW = 6`, `MAX_MEMO_BYTES = 256`, `MAX_BATCH_OUTPUTS = 128`, `MTP_WINDOW = 11`.

### 20.1 Padrões de governança (estado gênese) 🔒

| Parâmetro | mainnet | testnet | regtest |
|---|---|---|---|
| `max_block_bytes` | 1 000 000 | 1 000 000 | 1 000 000 |
| `max_block_fuel` | 50 000 000 | 50 000 000 | 50 000 000 |
| `base_fee` | 1 000 (0,00001 TCN) | 1 000 | 100 |
| `fee_per_kb` | 10 000 (10 motes/byte) | 10 000 | 1 000 |
| `fee_per_kfuel` | 1 000 | 1 000 | 100 |
| `storage_deposit_per_kb` | 100 000 (0,001 TCN) | 100 000 | 10 000 |
| `proposal_deposit` | 100 TCN | 10 TCN | 1 TCN |
| `vote_period` | 80 640 (≈14 d) | 5 760 (≈1 d) | 20 |
| `quorum_bp` | 1 000 (10 %) | 500 (5 %) | 1 000 |
| `approval_bp` | 6 667 (66,67 %) | 6 667 | 6 667 |
| `miner_approval_bp` | 6 000 (60 %) | 5 000 | 5 000 |
| `activation_delay` | 2 880 (≈12 h) | 720 (≈3 h) | 5 |

`ChainGlobal.congestion_bp` inicial = 10 000 em todas as redes.

### 20.2 Limites (`GovBounds`, inclusivos) 🔒

| Parâmetro | mainnet e testnet | regtest |
|---|---|---|
| `max_block_bytes` | 250 000 – 8 000 000 | 10 000 – 8 000 000 |
| `max_block_fuel` | 5 000 000 – 500 000 000 | 1 000 000 – 500 000 000 |
| `base_fee` | 0 – 10 000 000 | 0 – 10 000 000 |
| `fee_per_kb` | 1 – 100 000 000 | 0 – 100 000 000 |
| `fee_per_kfuel` | 1 – 100 000 000 | 0 – 100 000 000 |
| `storage_deposit_per_kb` | 0 – 1 000 000 000 | 0 – 1 000 000 000 |
| `proposal_deposit` | 1 TCN – 1 000 000 TCN | 0 – 1 000 000 TCN |
| `vote_period` | 5 760 – 806 400 | 5 – 201 600 |
| `quorum_bp` | 100 – 5 000 | 0 – 10 000 |
| `approval_bp` | 5 001 – 9 500 | 5 001 – 10 000 |
| `miner_approval_bp` | 5 000 – 9 500 | 0 – 10 000 |
| `activation_delay` | 720 – 172 800 | 1 – 43 200 |

## 21. Gênese

🔒 (`core/src/genesis.rs`) **Sem pré-mineração.**

```
genesis_message = "The Coin | the-coin.cloud | 2026-09-13 | A democratic, lightweight proof-of-work money for everyone"

estado gênese = { 0x06 → borsh(ChainGlobal { emitted: 0, burned: 0, params: gov_defaults,
                                             voting: [], pending_activations: [],
                                             proposal_count: 0, contract_count: 0,
                                             congestion_bp: 10 000 }) }

header = {
  version: 1, height: 0,
  prev_hash:  tagged_hash("genesis", genesis_message (UTF-8) || u32_le(chain_id)),
  tx_root:    merkle_root([]) = tagged_hash("merkle-empty"),
  state_root: LtHash(estado gênese).root,
  timestamp:  genesis_timestamp,
  target:     u256_be(genesis_target),
  nonce: 0, miner: Address::ZERO, signal: 0,
}
block = { header, txs: [] }
```

O gênese **não** precisa satisfazer a PoW; ele é embutido no software.

| Rede | Hash do gênese (v0.2) |
|---|---|
| mainnet | `e46f323de0ebf9168f5b65bad54710cb025263cf38038c1eca2895565360091d` |
| testnet | `b26ef97d496d11b218dd430a96515c7a48aecb24156610a69ba74acc861642a8` |
| regtest | `7022c9deda5f997eed99a09ca77ad4c48dcb1f5a8ab892adc0a4a180c19622cd` |

## 22. Vetores de teste

Gerados e verificados pelo código de referência v0.2
(`cargo test -p thecoin-wallet --test vectors -- --nocapture`, arquivo
`crates/wallet/tests/vectors.rs`).

```
tagged_hash("txid", "")        = 38aa34a475f8d3b9c7b15275271dce321fc9e6835e273e6672c9ec3c4b653b6a
tagged_hash("block", "abc")    = 09bb3b792db7c48af43cdb15ba3f1b274440d964372789fadcc4d8da9c1679d8
merkle_root([])                = 5740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc1
address(pubkey = 32 × 0x00)    = 7b32d1267e52aebd6fde6e0bdc70e394c133183e   (20 bytes, hex)
LtHash vazio  .root            = 0fe12e22656003a7066f77e6b814dde5649b40f99249c737fc256a62277b9b1f
LtHash {"k":"v"}.root          = 2ab3e575a30d01995680fbd29443b525bb861f070d548c118012a044a6d61435
```

Cabeçalhos gênese (180 bytes, hex):

```
mainnet:
010000000000000000000000628153829eca06c2fef7546b35d52d4308bc7d73e857b7c0d192518d3fa10de75740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc18e944d5c888c688ab07397bd7b9053ec4c95dfa50951258e26da9058cc0cb03280e7a56a000000000007ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff0000000000000000000000000000000000000000000000000000000000000000
  prev_hash  = 628153829eca06c2fef7546b35d52d4308bc7d73e857b7c0d192518d3fa10de7
  state_root = 8e944d5c888c688ab07397bd7b9053ec4c95dfa50951258e26da9058cc0cb032
  CoinHash   = b036510a55da453b7e7e221c2ea59edf66df8a85ae654add27436f4b6d8086d7

testnet:
  prev_hash  = 8c4c3fc095f0fd4447d797cef32537b87b91ecc70ab422f237efe6f49460ef09
  state_root = 890d5ea84fd146f3f2d932b878215f80651889efe45d4c0a4aed216364276767

regtest:
010000000000000000000000770efbde639a4320419a8fb4c18ac2d2fb57c5e6d6a38d77b870945adbe7ca725740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc1b87872c9e9506f8be2d05dce0b41b6d7cefcfb8c9c59c7071ceb53d1e31cf04f80e7a56a00000000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff0000000000000000000000000000000000000000000000000000000000000000
  prev_hash  = 770efbde639a4320419a8fb4c18ac2d2fb57c5e6d6a38d77b870945adbe7ca72
  state_root = b87872c9e9506f8be2d05dce0b41b6d7cefcfb8c9c59c7071ceb53d1e31cf04f
  CoinHash (m=64 KiB) = fe79348dd5c7d846c9a08d750a30d810f7adf71fc867cdc7d1f91123d7f46b81
```

Endereço de programa e taxa mínima (mainnet, 1,0×):

```
program_address(sender = tc1f420y9z7hu6as5yexhcklv0tklh6glty0n9yvj, nonce 0)
    = ae2db50538a731352cb1d695dbba1e4eeac5ffd4 = tc14ckm2pfc5ucn2t93662ahws7fm4vtl75u534g7
required_fee(size 163, max_fuel 0)      = 2 630
required_fee(size 225, max_fuel 20 000) = 23 250
```

Vetores de chaves, endereços, chaves de anel e de transações completas assinadas
(transferência, transferência substituível e `Invoke`) estão em
[`WALLET_DEVELOPERS.md`](WALLET_DEVELOPERS.md#8-vetores-de-teste).
