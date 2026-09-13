# The Coin — Especificação do Protocolo de Consenso (v0.1)

Este documento especifica **todas as regras de consenso** da The Coin v0.1 com
precisão suficiente para uma implementação independente (em qualquer
linguagem) chegar exatamente aos mesmos hashes, estados e decisões que o nó de
referência em Rust (`crates/core`).

> **Convenções**
>
> * 🔒 **Consenso** — alterar qualquer item marcado assim é um *hard fork*
>   (nós antigos passam a rejeitar blocos de nós novos ou vice‑versa).
> * 🧩 **Enums são append-only** — a posição (índice) de cada variante é parte
>   do formato binário. Novas variantes só podem ser **adicionadas no final**;
>   nunca reordenar, remover ou renomear com mudança de posição.
> * Todos os inteiros são **little-endian**, exceto quando indicado (alvos de
>   PoW e chaves de altura no estado são big-endian).
> * `||` = concatenação de bytes.
> * Código de referência entre parênteses, ex.: (`core/src/tx.rs`).

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
14. [Validação de transações](#14-validação-de-transações)
15. [Processamento de bloco](#15-processamento-de-bloco)
16. [Contratos de pagamento](#16-contratos-de-pagamento)
17. [Governança](#17-governança)
18. [Seleção de cadeia](#18-seleção-de-cadeia)
19. [Parâmetros por rede](#19-parâmetros-por-rede)
20. [Gênese](#20-gênese)
21. [Vetores de teste](#21-vetores-de-teste)

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
inválido.

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
| `merkle-leaf`, `merkle-node`, `merkle-empty` | árvore de Merkle |
| `address` | derivação de endereço |
| `contract-id` | id de contrato |
| `proposal-id` | id de proposta |
| `state-root` | raiz de estado a partir do LtHash |
| `genesis` | `prev_hash` do bloco gênese |
| `p2p` | checksum de frames P2P (não é consenso) |

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
conhecida e é usado como "minerador" do gênese.

## 4. Codificação canônica (Borsh)

🔒 Todas as estruturas de consenso usam [Borsh](https://borsh.io) com as regras:

| Tipo | Codificação |
|---|---|
| `u8`, `u16`, `u32`, `u64` | little-endian, largura fixa |
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
    nonce: u64,         // deve ser igual ao nonce da conta remetente
    fee: u64,           // motes; >= min_fee_per_byte × tamanho serializado
    expiry_height: u64, // última altura em que pode entrar num bloco; 0 = sem expiração
    action: TxAction,
}
```

Layout do `TxBody` (antes da ação): `version(1) | chain_id(4) | nonce(8) | fee(8) | expiry_height(8)` = 29 bytes, seguido da ação.

### 5.2 `TxAction` 🧩

| Índice | Variante | Campos (em ordem) |
|---|---|---|
| 0 | `Transfer` | `to: Address`, `amount: u64`, `memo: Vec<u8>` |
| 1 | `BatchTransfer` | `outputs: Vec<TransferOutput>`, `memo: Vec<u8>` |
| 2 | `CreateContract` | `spec: ContractSpec` |
| 3 | `CallContract` | `contract: Hash32`, `call: ContractCall` |
| 4 | `Propose` | `proposal: ProposalSpec` |
| 5 | `Vote` | `proposal: Hash32`, `choice: VoteChoice`, `weight: u64` |

`TransferOutput { to: Address, amount: u64 }`.

### 5.3 `ContractSpec` 🧩

| Índice | Variante | Campos |
|---|---|---|
| 0 | `Escrow` | `payee: Address`, `arbiter: Option<Address>`, `amount: u64`, `deadline_height: u64` |
| 1 | `Vesting` | `beneficiary: Address`, `amount: u64`, `start_height: u64`, `cliff_height: u64`, `end_height: u64`, `revocable: bool` |
| 2 | `Subscription` | `payee: Address`, `amount_per_period: u64`, `period_blocks: u64`, `max_periods: u32` |
| 3 | `Htlc` | `recipient: Address`, `amount: u64`, `hash_lock: Hash32`, `timeout_height: u64` |
| 4 | `Multisig` | `signers: Vec<Address>`, `threshold: u8`, `initial_deposit: u64` |

### 5.4 `ContractCall` 🧩

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

### 5.5 Governança 🧩

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
| `MinFeePerByte` | 1 | `min_fee_per_byte` |
| `ProposalDeposit` | 2 | `proposal_deposit` |
| `VotePeriod` | 3 | `vote_period` |
| `QuorumBp` | 4 | `quorum_bp` |
| `ApprovalBp` | 5 | `approval_bp` |
| `MinerApprovalBp` | 6 | `miner_approval_bp` |
| `ActivationDelay` | 7 | `activation_delay` |

### 5.6 Assinatura e identificadores 🔒

```
signing_hash = tagged_hash("tx-sign", borsh(TxBody))
signature    = Ed25519.sign(secret_key, signing_hash)     // assina os 32 bytes do hash
txid         = tagged_hash("txid", borsh(Transaction))
sender       = Address::from_public_key(public_key)
size         = len(borsh(Transaction))
```

Como `fee` é um `u64` de largura fixa, **o tamanho não depende do valor da
taxa**: carteiras assinam uma vez com `fee = 0` para medir o tamanho e depois
assinam de novo com `fee = size × fee_per_byte`.

## 6. Blocos e cabeçalhos

(`core/src/block.rs`)

### 6.1 Cabeçalho — 180 bytes fixos 🔒

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
| 156 | 20 | `miner: Address` | recebe subsídio + taxas |
| 176 | 4 | `signal: u32` | bits de sinalização de governança |
| **180** | | | fim |

```rust
struct Block { header: BlockHeader, txs: Vec<Transaction> }
```

Tamanho serializado de um bloco = `180 + 4 + Σ tamanho(tx)`. Um bloco vazio tem
**184 bytes**.

### 6.2 Hash do bloco 🔒

```
block_hash = tagged_hash("block", borsh(BlockHeader))
```

É o identificador usado em toda parte (P2P, índices, `prev_hash`). Barato de
calcular — **não** é o hash da prova de trabalho.

## 7. Prova de trabalho — CoinHash

🔒 (`core/src/pow.rs`)

```
CoinHash(header) = Argon2id(
    password = borsh(BlockHeader)   // 180 bytes
    salt     = "TheCoin/PoW/v1.."   // 16 bytes ASCII fixos
    m        = pow.mem_kib (KiB)
    t        = pow.iterations
    p        = 1 (lanes)
    version  = 0x13
    tag_len  = 32
)
```

| Rede | `mem_kib` | `iterations` |
|---|---|---|
| mainnet | 16 384 (16 MiB) | 1 |
| testnet | 16 384 (16 MiB) | 1 |
| regtest | 64 | 1 |

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

🔒 Recalculado **a cada bloco** (`core/src/difficulty.rs`). Parâmetros:
`T = target_block_time = 60`, `N = lwma_window = 60`.

Seja `next_height` a altura do bloco sendo validado e `ancestors` a lista de
cabeçalhos do mesmo ramo, do mais antigo ao mais novo, terminando no **pai**.

```
função next_target(next_height, ancestors):
    se retarget == false               → retorna genesis_target
    se next_height <= N + 1            → retorna genesis_target
    se len(ancestors) < N + 1          → retorna genesis_target

    window = últimos N+1 elementos de ancestors        // window[0] .. window[N]
    k = N × (N + 1) × T / 2                            // mainnet: 109 800
    prev_ts = window[0].timestamp
    weighted = 0 (u64)
    sum_targets = 0 (U512)
    para j = 1..N:
        this_ts   = max(window[j].timestamp, prev_ts + 1)
        solvetime = min(this_ts − prev_ts, 6 × T)
        prev_ts   = this_ts
        weighted += solvetime × j
        sum_targets += window[j].target
    next = floor( sum_targets × weighted / (N × k) )    // aritmética inteira em 512 bits
    se next > pow_limit → next = pow_limit
    se next == 0        → next = 1
    retorna next (U256)
```

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

🔒 (`core/src/emission.rs`)

```
subsidy(0)      = 0                                         // gênese sem recompensa
subsidy(h >= 1) = initial_reward >> ((h − 1) / halving_interval)   (0 se shift >= 64)
```

Mainnet/testnet: `initial_reward = 40 TCN`, `halving_interval = 625 000` blocos
(≈ 434 dias). Alturas `1..=625 000` pagam 40 TCN, `625 001..=1 250 000` pagam
20 TCN, etc. Emissão total = 49 999 999,99… TCN < 50 000 000 TCN. As 4 primeiras
eras (≈ 4,76 anos) emitem 93,75 % do supply.

**Recompensa do bloco** = `subsidy(h) + Σ fees` das transações do bloco.
Ela fica **pendente** no estado e só é creditada ao `miner` no início do bloco
`h + coinbase_maturity` (mainnet: 100 blocos). Ver §15.

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

struct PendingReward { miner: Address, amount: u64 }

struct ChainGlobal {
    emitted: u64,
    burned: u64,
    params: GovParams,                       // 8 × u64 na ordem de GovParamId
    voting: Vec<(u8, Hash32)>,               // (bit de sinal, id) das propostas em votação
    pending_activations: Vec<(u64, Hash32)>, // (altura de ativação, id)
    proposal_count: u64,
    contract_count: u64,
}
circulating = emitted − burned (saturating)

struct GovParams {
    max_block_bytes, min_fee_per_byte, proposal_deposit, vote_period,
    quorum_bp, approval_bp, miner_approval_bp, activation_delay   // todos u64
}

struct Contract {
    id: Hash32, creator: Address, created_height: u64, balance: u64, state: ContractState
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
Σ Account.balance + Σ Contract.balance + Σ PendingReward.amount
  + Σ deposit das propostas com status Voting  ==  emitted − burned
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

O LtHash do estado é a soma de `element(k, v)` para todos os registros. Ao
aplicar um bloco, para cada chave alterada: `remove(k, old)` se existia e
`insert(k, new)` se continua existindo. O `state_root` do cabeçalho deve ser
igual ao valor calculado **depois** de aplicar o bloco inteiro. Escritas que
não mudam o valor não alteram o hash.

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

## 14. Validação de transações

(`core/src/execution.rs`)

### 14.1 Checagens independentes de estado 🔒

1. `size <= MAX_TX_BYTES = 16 384`.
2. `version == 1`.
3. `chain_id == params.chain_id`.
4. Por ação:
   * `Transfer`: `amount > 0`; `len(memo) <= 256`.
   * `BatchTransfer`: `1 <= len(outputs) <= 128`; todo `amount > 0`; `len(memo) <= 256`; soma sem overflow.
   * `CreateContract`: `spec.validate_static()` (§16).
   * `CallContract`: `HtlcRedeem.preimage <= 64 bytes`; `MultisigDeposit.amount > 0`; `MultisigPropose.amount > 0` e `memo <= 256`.
   * `Propose`: `title` não vazio após `trim`, `len(title) <= 120` bytes; `len(url) <= 256`; em `SoftwareUpgrade`, `1 <= len(version) <= 32`.
   * `Vote`: `weight > 0`.
5. Assinatura Ed25519 estrita sobre `signing_hash`.

### 14.2 Aplicação (dependente de estado) 🔒

Na altura `h` do bloco, com `g = ChainGlobal` corrente:

1. Se `expiry_height != 0` e `h > expiry_height` → inválida.
2. `fee >= g.params.min_fee_per_byte × size`.
3. `nonce == conta(sender).nonce`.
4. `debit` =
   * `Transfer`: `amount`; `BatchTransfer`: `Σ amount`;
   * `CreateContract`: `funding(spec)` (§16);
   * `CallContract`: `amount` se `MultisigDeposit`, senão 0;
   * `Propose`: `g.params.proposal_deposit`; `Vote`: 0.
5. `debit + fee <= spendable(h)`; então `balance −= debit + fee`, `nonce += 1`.
6. Executa a ação (crédito a destinatários, criação/chamada de contrato,
   proposta, voto). Créditos criam a conta se não existir.
7. Se qualquer passo falhar, a transação é **inválida** e o bloco que a contém
   é inválido. Não existe "falhou mas pagou taxa".

Taxas não são creditadas ao destinatário; somam-se à recompensa pendente do
bloco.

## 15. Processamento de bloco

🔒 Ordem exata de `apply_block` (após as checagens de cabeçalho da §18):

1. `g = ChainGlobal` atual.
2. `len(borsh(Block)) <= g.params.max_block_bytes` (e sempre `<= 8 000 000`).
3. `mask` = OR de `1 << bit` para cada `(bit, _)` em `g.voting`.
   `header.signal & !mask` deve ser 0 (bits não atribuídos devem ser zero).
   A máscara é calculada **antes** das transações do bloco.
4. **begin:** se `h > coinbase_maturity`, leia `PendingReward` na chave
   `0x05 || u64_be(h − coinbase_maturity)`; se existir, apague-o e credite
   `amount` a `miner`.
5. **transações**, em ordem. `txid` duplicado no bloco → inválido. Cada uma:
   checagens §14.1 e aplicação §14.2. `fees += fee`.
6. **end:**
   1. Governança (§17.4): contagem de sinais, fechamento de votações, ativações.
   2. `emitted += subsidy(h)`; se `emitted > MAX_SUPPLY` → inválido.
   3. Se `subsidy(h) + fees > 0`: grava `PendingReward { miner, amount }` em `0x05 || u64_be(h)`.
7. `state_root` calculado (§12) deve ser igual a `header.state_root`.

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

**Remoção 🔒:** após a chamada, contratos que não sejam `Multisig` com
`balance == 0` são **apagados** do estado. `Multisig` nunca é apagado.

## 17. Governança

🔒 (`core/src/governance.rs`)

```
proposal_id = tagged_hash("proposal-id", proposer(20) || u64_le(nonce_da_tx))
```

### 17.1 `Propose` (altura `h`)

1. Depósito `g.params.proposal_deposit` já debitado (§14.2).
2. `SetParam{param, value}`: `value` deve estar dentro de `gov_bounds[param]` (inclusivo).
3. `len(g.voting) < 32` (`MAX_CONCURRENT_PROPOSALS`).
4. `signal_bit` = menor bit `0..31` não usado por propostas em `g.voting`.
5. Colisão de id → inválido.
6. `end_height = h + g.params.vote_period`; status `Voting`; `g.voting.push((bit, id))`; `proposal_count += 1`.

### 17.2 `Vote{proposal, choice, weight}` (altura `h`)

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

### 17.3 Parâmetros e limites

Ver §19 para padrões e limites (`GovBounds`) por rede. **Não governáveis:**
supply máximo, curva de emissão, tempo de bloco, algoritmo de PoW, LWMA,
maturidade, profundidade máxima de reorg.

### 17.4 Processamento no fim do bloco (`end_block`, altura `h`)

Para cada `(bit, id)` em `g.voting` (na ordem da lista):

1. Se `h > created_height`: `miner_total_blocks += 1`; se `header.signal & (1 << bit) != 0`: `miner_yes_blocks += 1`.
2. Se `h >= end_height` — **apuração**:
   ```
   votes        = yes + no + abstain                      (u128)
   circulating  = g.emitted − g.burned                     (valor ANTES do subsídio deste bloco)
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
após as transações). O subsídio/pendência do bloco vem depois da governança (§15).

## 18. Seleção de cadeia

(`node/src/chain.rs`)

### 18.1 Checagens de cabeçalho 🔒 (antes de aplicar o corpo)

1. `version == 1`.
2. `height == pai.height + 1` (pai conhecido; senão o bloco é órfão).
3. `timestamp > MTP` e `timestamp <= now + 180` (§9).
4. `target == next_target(...)` (§8).
5. Checkpoints: se `height` é uma altura de checkpoint, o hash deve coincidir; se `height <= último checkpoint`, o pai deve estar na cadeia principal (sem forks abaixo do último checkpoint). *(A v0.1 não tem checkpoints definidos.)*
6. `len(borsh(Block)) <= 8 000 000`.
7. `tx_root` confere.
8. `CoinHash(header) <= target`.
9. Aplicação do corpo e `state_root` (§15). Descendentes de um bloco inválido são inválidos.

### 18.2 Regra de escolha

* A cadeia ativa é a de **maior trabalho acumulado** (`chainwork`).
* Um novo ramo só substitui a cadeia ativa se tiver trabalho **estritamente
  maior**; em empate vale o **primeiro visto**.
* **Profundidade máxima de reorganização: 720 blocos** (≈ 12 h). Um ramo mais
  pesado que exija desconectar mais de 720 blocos é guardado como lateral e
  **não** é adotado (proteção contra ataques de reescrita profunda; nós
  isolados por mais tempo exigem intervenção manual).
* Se a aplicação de qualquer bloco do novo ramo falhar, a reorganização inteira
  é abortada atomicamente e os blocos do ramo a partir do inválido são marcados
  como inválidos.

## 19. Parâmetros por rede

(`core/src/params.rs`)

| Parâmetro | mainnet | testnet | regtest |
|---|---|---|---|
| magic (P2P) | `TCN1` | `TCNT` | `TCNR` |
| `chain_id` 🔒 | `0x54430001` | `0x54430002` | `0x54430003` |
| porta P2P / API | 7333 / 7334 | 17333 / 17334 | 27333 / 27334 |
| CoinHash `mem_kib`, `t` 🔒 | 16 384, 1 | 16 384, 1 | 64, 1 |
| `pow_limit_shift` 🔒 | 8 | 6 | 0 |
| `genesis_target_shift` 🔒 | 13 | 10 | 0 |
| retarget (LWMA) 🔒 | sim | sim | **não** |
| tempo de bloco / janela LWMA 🔒 | 60 s / 60 | 60 s / 60 | 60 s / 60 |
| deriva futura máxima | 180 s | 180 s | 180 s |
| recompensa inicial 🔒 | 40 TCN | 40 TCN | 40 TCN |
| halving 🔒 | 625 000 | 625 000 | 150 |
| maturidade de recompensa 🔒 | 100 | 100 | 5 |
| reorg máxima | 720 | 720 | 720 |
| timestamp gênese | 1 789 257 600 | 1 789 257 600 | 1 789 257 600 |
| seeds | `seed1/seed2.the-coin.cloud:7333` | `testnet-seed1/2.the-coin.cloud:17333` | — |

### 19.1 Padrões de governança (estado gênese) 🔒

| Parâmetro | mainnet | testnet | regtest |
|---|---|---|---|
| `max_block_bytes` | 1 000 000 | 1 000 000 | 1 000 000 |
| `min_fee_per_byte` | 10 | 10 | 1 |
| `proposal_deposit` | 100 TCN | 10 TCN | 1 TCN |
| `vote_period` | 20 160 (≈14 d) | 1 440 | 20 |
| `quorum_bp` | 1 000 (10 %) | 500 (5 %) | 1 000 |
| `approval_bp` | 6 667 (66,67 %) | 6 667 | 6 667 |
| `miner_approval_bp` | 6 000 (60 %) | 5 000 | 5 000 |
| `activation_delay` | 2 880 (≈2 d) | 720 | 5 |

### 19.2 Limites (`GovBounds`, inclusivos) 🔒

| Parâmetro | mainnet e testnet | regtest |
|---|---|---|
| `max_block_bytes` | 250 000 – 8 000 000 | 10 000 – 8 000 000 |
| `min_fee_per_byte` | 1 – 100 000 | 0 – 100 000 |
| `proposal_deposit` | 1 TCN – 1 000 000 TCN | 0 – 1 000 000 TCN |
| `vote_period` | 1 440 – 201 600 | 5 – 201 600 |
| `quorum_bp` | 100 – 5 000 | 0 – 10 000 |
| `approval_bp` | 5 001 – 9 500 | 5 001 – 10 000 |
| `miner_approval_bp` | 5 000 – 9 500 | 0 – 10 000 |
| `activation_delay` | 720 – 43 200 | 1 – 43 200 |

## 20. Gênese

🔒 (`core/src/genesis.rs`) **Sem pré-mineração.**

```
genesis_message = "The Coin | the-coin.cloud | 2026-09-13 | A democratic, lightweight proof-of-work money for everyone"

estado gênese = { 0x06 → borsh(ChainGlobal { emitted: 0, burned: 0, params: gov_defaults,
                                             voting: [], pending_activations: [],
                                             proposal_count: 0, contract_count: 0 }) }

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

| Rede | Hash do gênese |
|---|---|
| mainnet | `f6dd9900a890feb121b30c83f567427b9fc5174a08c9dfe0091970b38ccb6160` |
| testnet | `771ecc84e4c13bdd60b696bb2d8458d8c9961fa3106b67a96aebc011d021bd1c` |
| regtest | `d4c9b8039d2cffefcfd6fe6d362bda992d149d47107dd73346e00e8f59e56c8b` |

## 21. Vetores de teste

Gerados com o código de referência v0.1.

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
010000000000000000000000628153829eca06c2fef7546b35d52d4308bc7d73e857b7c0d192518d3fa10de75740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc131e0b0b8a01bf11a42ed1cf8848007d04997772d3252724508df3b700d43be3480e7a56a000000000007ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff0000000000000000000000000000000000000000000000000000000000000000
  prev_hash  = 628153829eca06c2fef7546b35d52d4308bc7d73e857b7c0d192518d3fa10de7
  state_root = 31e0b0b8a01bf11a42ed1cf8848007d04997772d3252724508df3b700d43be34
  CoinHash   = d32c94407f5565fd776f3cb5e34379ab364d29a2d2a11873e7b6b2f57ee02f3b

testnet:
  prev_hash  = 8c4c3fc095f0fd4447d797cef32537b87b91ecc70ab422f237efe6f49460ef09
  state_root = 6817f44fc99f24ee4ede921378b0575a8e53ea1366d863b18dc8a31650c4405f

regtest:
  prev_hash  = 770efbde639a4320419a8fb4c18ac2d2fb57c5e6d6a38d77b870945adbe7ca72
  state_root = 29e49a70ec8891cd32c12d23950fc32f269189104658fd8c740547f2d9f36aa3
  CoinHash (m=64 KiB) = e901eef235a40549d7cddb029dbc41191a32ce63cb6dec18f5b6981ef4a841cc
```

Vetores de chaves, endereços e de uma transação completa assinada estão em
[`WALLET_DEVELOPERS.md`](WALLET_DEVELOPERS.md#8-vetores-de-teste).
