# Guia para desenvolvedores de carteiras

Este guia explica como construir uma carteira **compatível** com a The Coin v0.2
em qualquer linguagem (JavaScript, Kotlin, Swift, Go, Python…). Seguindo-o, sua
carteira gera os mesmos endereços que a carteira de referência
(`thecoin-wallet`) a partir das mesmas 24 palavras, calcula as mesmas taxas e
produz transações aceitas por qualquer nó.

A implementação de referência está em `crates/wallet` (chaves, keystore,
construção de transações, cliente HTTP, URIs) e `crates/core` (formatos). A
especificação completa dos formatos está em [PROTOCOL.md](PROTOCOL.md) e a API
em [API.md](API.md).

> **Mudou na v0.2:** o `TxBody` tem um byte `flags` depois do `chain_id`, a taxa
> mínima é `base + por kB + por 1 000 de combustível` com multiplicador de
> congestionamento, há prioridade de taxa, substituição (RBF) opt-in, alertas
> de gasto duplo, cooldown de recompensas, contratos TCCL (`Deploy`/`Invoke`) e
> pools de privacidade. Transações e vetores de teste da v0.1 **não são válidos** na v0.2.

## Bibliotecas necessárias

| Função | Padrão | Exemplos |
|---|---|---|
| Frase de recuperação | BIP‑39 (inglês) | `bip39` (JS), `bip-utils` (Python) |
| Derivação | SLIP‑0010 Ed25519 | `ed25519-hd-key` (JS), implementação própria (≈20 linhas com HMAC‑SHA512) |
| Assinatura | Ed25519 (RFC 8032) | `@noble/ed25519`, libsodium, `ed25519-dalek` |
| Hash | BLAKE3 | `@noble/hashes/blake3`, `blake3` (Python/Go) |
| Endereço | bech32m (BIP‑350) | `bech32` (JS/Python) |
| HTLC | SHA‑256 | padrão da linguagem |
| Keystore (opcional) | Argon2id + ChaCha20‑Poly1305 | `hash-wasm`/`argon2`, `@noble/ciphers` |
| Privacidade (opcional) | Ristretto255 + SHA‑512 (bLSAG) | `@noble/curves` (ristretto255), `ristretto255` (Go), `curve25519-dalek` |

## 1. Frase de recuperação e seed

1. Gere 256 bits (24 palavras) ou 128 bits (12 palavras) de entropia com um
   gerador criptográfico do sistema.
2. Converta para a frase BIP‑39 em inglês.
3. Seed = `PBKDF2-HMAC-SHA512(frase_normalizada, "mnemonic" + passphrase, 2048)`
   (64 bytes) — exatamente o BIP‑39. A carteira de referência usa passphrase vazia.

Ao importar: normalize espaços (um único espaço entre palavras) e letras
minúsculas, e valide o checksum BIP‑39.

## 2. Derivação SLIP‑0010 (Ed25519)

Caminho: **`m/44'/7333'/account'/0'/index'`** — todos os níveis *hardened*
(Ed25519 não suporta derivação não-hardened). `7333` é o *coin type* da The Coin.
A carteira de referência usa `account = 0` e incrementa `index`. Chaves de anel
para privacidade usam `m/44'/7333'/0'/7'/index'` (§7).

```
I = HMAC-SHA512(key = "ed25519 seed", data = seed)
k = I[0..32], c = I[32..64]
para cada i no caminho:
    I = HMAC-SHA512(key = c, data = 0x00 || k || u32_be(i | 0x80000000))
    k = I[0..32], c = I[32..64]
secret_key = k   (semente Ed25519 de 32 bytes)
```

Pseudocódigo em JavaScript:

```js
import { hmac } from '@noble/hashes/hmac';
import { sha512 } from '@noble/hashes/sha512';

function deriveEd25519(seed, path) {
  let I = hmac(sha512, new TextEncoder().encode('ed25519 seed'), seed);
  let k = I.slice(0, 32), c = I.slice(32);
  for (const index of path) {
    const data = new Uint8Array(37);
    data[0] = 0; data.set(k, 1);
    new DataView(data.buffer).setUint32(33, (index | 0x80000000) >>> 0, false);
    I = hmac(sha512, c, data);
    k = I.slice(0, 32); c = I.slice(32);
  }
  return k;
}
const sk = deriveEd25519(seed, [44, 7333, 0, 0, index]);
```

## 3. Endereço

```
public_key = Ed25519.public_key(secret_key)                  // 32 bytes
h          = BLAKE3("TheCoin:address" || 0x00 || public_key)
address20  = h[0..20]
texto      = bech32m_encode(hrp, address20)   // hrp: "tc" mainnet, "tct" testnet, "tcr" regtest
```

Os 20 bytes são codificados diretamente (conversão 8→5 bits normal), sem byte
de versão. Ao validar um endereço digitado pelo usuário, exija checksum
**bech32m** e o HRP da rede atual — assim uma moeda de testnet nunca é enviada
para mainnet por engano. Contratos TCCL também têm endereços `tc1…` (§6).

## 4. Construindo uma transação

### 4.1 Consultar a conta e as taxas

```bash
GET /api/v1/address/{remetente}   → use "next_nonce", "spendable" (e mostre "immature")
GET /api/v1/fees                  → parâmetros de taxa, congestionamento e prioridades
```

* `nonce` da transação = `next_nonce` (já conta as transações pendentes no mempool).
* Se enviar várias em sequência sem esperar confirmação, incremente o nonce localmente.
* `spendable` exclui moedas bloqueadas por voto; recompensas de mineração em
  cooldown aparecem em `immature` e **não** estão em `balance`.

Resposta real de `/api/v1/fees` (regtest):

```json
{
  "base_fee": 100,
  "fee_per_kb": 1000,
  "fee_per_kfuel": 100,
  "storage_deposit_per_kb": 10000,
  "congestion_bp": 10000,
  "typical_transfer_fee": 260,
  "priority": { "low_bp": 10000, "normal_bp": 12500, "high_bp": 20000, "urgent_bp": 40000 },
  "mempool_txs": 0,
  "mempool_bytes": 0
}
```

| Campo | Uso |
|---|---|
| `base_fee`, `fee_per_kb`, `fee_per_kfuel` | parâmetros de governança da fórmula (§4.4) |
| `congestion_bp` | multiplicador atual (10 000 = 1,0×); a parte acima de 1× é queimada |
| `storage_deposit_per_kb` | depósito reembolsável por kB de estado de contrato |
| `typical_transfer_fee` | taxa mínima atual de uma transferência de 160 bytes (referência para a UI) |
| `priority.*_bp` | multiplicadores **sobre a taxa mínima** para cada nível de prioridade |

### 4.2 Serializar o corpo (Borsh)

Todos os inteiros little-endian. Estrutura de `TxBody`:

| Campo | Tipo | Bytes |
|---|---|---|
| `version` | u8 | 1 (sempre `0x01`) |
| `chain_id` | u32 | 4 (`0x54430001` mainnet, `…02` testnet, `…03` regtest) |
| `flags` | u8 | 1 (`0x00`; `0x01` = substituível, §4.7) |
| `nonce` | u64 | 8 |
| `fee` | u64 | 8 |
| `expiry_height` | u64 | 8 (0 = não expira) |
| `action` | enum | 1 byte de índice + campos |

`Transfer` (índice `0x00`): `to` (20 bytes) `|| amount` (u64) `|| memo` (u32 tamanho + bytes).
`Invoke` (índice `0x07`): ver §6.2. Para as outras ações veja [PROTOCOL.md §5](PROTOCOL.md#5-transações).

### 4.3 Assinar

```
signing_hash = BLAKE3("TheCoin:tx-sign" || 0x00 || body_bytes)
signature    = Ed25519.sign(secret_key, signing_hash)      // assina os 32 bytes do hash
tx_bytes     = body_bytes || public_key(32) || signature(64)
txid         = BLAKE3("TheCoin:txid" || 0x00 || tx_bytes)
```

### 4.4 Calcular a taxa

A taxa mínima depende do tamanho da transação, do combustível reservado
(`max_fuel`, só em contratos TCCL) e do congestionamento:

```
base     = base_fee + ceil(size × fee_per_kb / 1000) + ceil(max_fuel × fee_per_kfuel / 1000)
mínima   = ceil(base × congestion_bp / 10000)
taxa     = ceil(mínima × priority_bp / 10000)          // priority_bp de /api/v1/fees
queimado = mínima − base                               // sobretaxa de congestionamento
minerador recebe taxa − queimado
```

Como `fee` é um u64 de tamanho fixo, o tamanho **não muda** com o valor da taxa:

```
1. monte o corpo com fee = 0, assine, meça size = len(tx_bytes)
2. calcule a taxa com a fórmula
3. monte o corpo com essa fee e assine de novo
```

**Exemplo passo a passo** (mainnet, parâmetros padrão `base_fee = 1 000`,
`fee_per_kb = 10 000`, `fee_per_kfuel = 1 000`), transferência com memo `"test"`
(vetor §8.2, **163 bytes**):

| Passo | Cálculo | Motes |
|---|---|---|
| base | `1 000 + ceil(163 × 10 000 / 1 000)` = `1 000 + 1 630` | 2 630 |
| mínima a 1,0× (`congestion_bp = 10 000`) | `ceil(2 630 × 10 000 / 10 000)` | 2 630 (0,0000263 TCN) |
| prioridade `normal` (12 500) | `ceil(2 630 × 1,25)` = `ceil(3 287,5)` | 3 288 |
| com congestionamento 1,5× (`15 000`) | mínima `ceil(2 630 × 1,5)` | 3 945 (1 315 queimados) |
| … e prioridade `normal` | `ceil(3 945 × 1,25)` = `ceil(4 931,25)` | 4 932 (minerador: 3 617) |

Chamada de contrato do vetor §8.4 (225 bytes, `max_fuel = 20 000`):
`1 000 + ceil(225 × 10) + ceil(20 000 × 1 000 / 1 000) = 1 000 + 2 250 + 20 000 = 23 250` motes a 1,0×.

**Níveis de prioridade** (a carteira de referência usa `normal` por padrão). O nó
calcula `high` e `urgent` a partir do mempool: ordena as pendentes por taxa por
peso e mede o multiplicador (sobre a taxa mínima por byte de uma transferência
típica) necessário para ficar dentro de um bloco cheio ou do primeiro quarto dele:

| Nível | `priority` | Quando usar |
|---|---|---|
| `low` | `low_bp` = 10 000 | blocos vazios; pode esperar se o congestionamento subir (fica no mempool até caber) |
| `normal` | `normal_bp` = 12 500 | continua válida mesmo se o multiplicador subir 12,5 % no próximo bloco |
| `high` | `max(multiplicador da pendente que completa um bloco cheio + 0,1×, 2,0×)` | à frente da maioria das pendentes |
| `urgent` | `max(multiplicador da pendente que completa ¼ de bloco + 0,25×, 2 × high)` | próximo bloco com alta probabilidade |

Mineradores ordenam por **taxa por unidade de peso**, `fee / (size + max_fuel / 100)`.
Se o congestionamento subir acima da sua taxa, a transação não é descartada:
fica no mempool e volta a ser incluível quando o multiplicador cair.

### 4.5 Enviar

```bash
curl -X POST https://the-coin.cloud/api/v1/tx \
  -H 'content-type: application/json' \
  -d '{"tx":"<hex de tx_bytes>"}'
# → {"txid":"…"}
```

O nó valida tudo antes de aceitar (inclusive simulando chamadas de contrato);
mensagens de erro estão em [API.md](API.md#post-apiv1tx).

### 4.6 Acompanhar confirmações

```bash
GET /api/v1/tx/{txid}
GET /api/v1/security?amount=<motes>
```

* `in_mempool: true` → aguardando bloco.
* `block_height` preenchido → confirmada; `confirmations` = `tip − altura + 1`.
* `success: false` → transação de contrato TCCL incluída, mas o código falhou:
  a taxa foi paga e nada mais aconteceu (`error` explica). Mostre isso claramente.
* `conflict` preenchido → outra transação do mesmo remetente com o mesmo nonce foi
  vista (**possível gasto duplo**, §4.7). Não entregue mercadoria sem confirmação.
* `replaceable: true` → o remetente pode trocá-la por outra; trate como não confirmada.
* 404 depois de estar no mempool → foi descartada (substituída por RBF,
  expirou, ou conflito de nonce); reconstrua com o `next_nonce` atual.

**Quantas confirmações?** Use `GET /api/v1/security?amount=` (ou
`thecoin-wallet confirmations <valor>`): recomenda `N` blocos tal que as
recompensas que um atacante abandonaria (`N × recompensa do bloco`) sejam pelo
menos o dobro do valor, entre 1 e a profundidade máxima de reorganização (2 880
blocos ≈ 12 h). Exemplo (regtest, recompensa 40 TCN): 500 TCN → 25
confirmações; 1 TCN → 1. Quando os mineradores estão assinando blocos
(finalidade), a resposta traz `blocks_to_finality` e a recomendação cai para
esse número — normalmente 2 blocos, ≈ 30 s — porque um bloco final nunca é
revertido, qualquer que seja o valor.

Recompensas de mineração: 25 % gastáveis após 400 blocos e o restante após
4 000 blocos (`immature` na API mostra o que ainda está em cooldown).

### 4.7 Substituição (RBF), gasto duplo e expiração

* **Opt-in:** só transações assinadas com `flags = 0x01` (`FLAG_REPLACEABLE`)
  podem ser substituídas no mempool. A substituta usa o **mesmo nonce** e
  `fee >= fee_antiga + floor(fee_antiga / 4)` (e maior que a antiga).
  Na carteira de referência: `--replaceable` ao enviar e `bump-fee <txid>` depois.
* Uma segunda transação com o mesmo nonce de uma pendente **não** substituível é
  recusada (`400`), registrada como **tentativa de gasto duplo**, retransmitida aos
  peers (mensagem `DoubleSpend`) e listada em `GET /api/v1/alerts`; as duas
  transações passam a ter `conflict` preenchido. Lojas que aceitam pagamentos
  sem confirmação devem consultar `/api/v1/alerts` ou `conflict` e exigir
  confirmação para pagamentos `replaceable`.
* `expiry_height` (ex.: `tip + 1440`) garante que uma transação esquecida não
  seja minerada dias depois.

## 5. Contratos nativos e governança

* Ids determinísticos (a carteira pode mostrar antes da confirmação):
  ```
  contract_id = BLAKE3("TheCoin:contract-id" || 0x00 || remetente20 || u64_le(nonce))
  proposal_id = BLAKE3("TheCoin:proposal-id" || 0x00 || remetente20 || u64_le(nonce))
  ```
  (`nonce` = nonce da transação que cria.) A API também devolve em `created`.
* **Débito total a verificar antes de enviar:**
  `Transfer: amount + fee`, `BatchTransfer: Σ + fee`,
  `CreateContract: funding + fee + depósito de armazenamento`
  (subscription: `amount_per_period × max_periods`; o depósito é
  `ceil((tamanho do registro + 33) / 1000) × storage_deposit_per_kb` — 1 kB para
  quase todos os contratos, ex. 0,001 TCN na mainnet — e volta ao criador quando o
  contrato termina ou o multisig é fechado com `MultisigClose`),
  `MultisigDeposit: amount + fee`, `Propose: proposal_deposit + fee`
  (valor em `/api/v1/status → params.proposal_deposit`), `Vote: fee`
  (mas `weight` fica bloqueado até o fim da votação),
  `Deploy`/`Invoke: value + max_deposit + fee` (no pior caso).
* O voto pode entrar no mesmo bloco em que a proposta foi criada, desde que
  venha depois do `Propose` (nonce maior do mesmo remetente, ou transação
  posterior no bloco). O campo `created` de `GET /api/v1/tx/{txid}` já traz o id
  da proposta mesmo antes da confirmação.
* HTLC usa **SHA‑256** (compatível com Bitcoin/Lightning para atomic swaps).

Guias de uso: [CONTRACTS.md](CONTRACTS.md) e [GOVERNANCE.md](GOVERNANCE.md).

## 6. Contratos TCCL (`Deploy` / `Invoke`)

A linguagem está em [docs/tccl/TCCL.md](tccl/TCCL.md). Para uma carteira, o que
importa é montar a transação, reservar combustível suficiente e ler o recibo.

### 6.1 Endereço do contrato

```
program_address = BLAKE3("TheCoin:program-address" || 0x00 || remetente20 || u64_le(nonce_do_deploy))[0..20]
```

### 6.2 Codificação

```
Deploy (0x06): source: String || init_args: Vec<Value> || value: u64 || max_fuel: u64 || max_deposit: u64
Invoke (0x07): contract: [u8;20] || function: String || args: Vec<Value> || value: u64 || max_fuel: u64 || max_deposit: u64

Value: 0x00 Int(i128 LE, 16 bytes) | 0x01 Bool | 0x02 Text(String) | 0x03 Bytes(Vec<u8>)
       | 0x04 Address([u8;20]) | 0x05 List(Vec<Value>) | 0x06 Unit
```

Os argumentos precisam ter exatamente os tipos declarados na função. Consulte a
interface em `GET /api/v1/program/{endereço}` (`functions[].params` como
`[nome, tipo]`, ex. `["amount", "int"]`). Limites: `Deploy` ≤ 64 000 bytes,
demais ≤ 16 384; `1 <= max_fuel <= 10 000 000`; até 32 argumentos; nome da função ≤ 64 bytes.

### 6.3 Medir o combustível por simulação

A taxa é cobrada sobre `max_fuel` **reservado** e combustível não usado não é
devolvido; por outro lado, se faltar combustível a chamada falha e a taxa é
perdida. Por isso **simule antes**:

1. Monte a transação com um `max_fuel` generoso (a carteira de referência usa
   2 000 000), a taxa correspondente e o nonce correto; assine.
2. `POST /api/v1/tx/simulate` com `{"tx":"<hex>"}`. O nó executa sobre o estado
   atual, depois das transações pendentes do mesmo remetente, sem gravar nada:
   ```json
   {"valid":true,"invalid_reason":null,"success":true,"error":null,"fuel_used":2225,
    "required_fee":5325,"logs":[{"contract":"tcr1nlak75fjnu8ez5g4c54yprf22ez02l7rn2ahqh","event":"Transfer",
    "fields":[["from","tcr1fexczggrd9s5af9fgu2dje00v5v8s6au7l8s8r"],["to","tcr1fgy8fh9zunze8ctehqh695g79hyxna260ylvyh"],["amount","10"]]}],
    "return_value":null,"program":null}
   ```
3. Se `valid = false`, a transação seria recusada (`invalid_reason`, ex.
   `"bad nonce: expected 12, got 1"`). Se `success = false`, o código falharia
   (`error`, ex. `"requirement failed: insufficient token balance"`): não envie.
4. Defina `max_fuel = fuel_used × 1,3 + 5 000` (regra da carteira de referência,
   limitada a 10 000 000), recalcule a taxa (§4.4), assine e envie.

A simulação verifica assinatura e taxa, então a transação de teste precisa
estar assinada e o saldo precisa cobrir a taxa do `max_fuel` de teste (ela não é
cobrada). `required_fee` na resposta é a mínima atual **da transação simulada**.

### 6.4 Depósito de armazenamento

Contratos pagam um depósito **reembolsável** pelo estado que ocupam:
`ceil(state_bytes / 1000) × storage_deposit_per_kb`. Quem faz o contrato crescer
paga a diferença; quem libera espaço recebe a parte proporcional; `destroy`
devolve tudo. Defina `max_deposit` como o máximo que o usuário aceita travar
(a carteira de referência usa 1 TCN); se o depósito necessário passar disso, a
chamada falha (com taxa cobrada). Para um `Deploy`, o estado inicial é
aproximadamente o tamanho compilado mostrado por `tccl check`.

### 6.5 Resultado

Depois de confirmada, `GET /api/v1/tx/{txid}` traz `success`, `error`,
`fuel_used`, `burned`, `logs` (eventos com campos já formatados como texto),
`return_value` e, para `Deploy`, `program` (endereço do contrato criado).
Leituras sem transação: `POST /api/v1/program/{endereço}/view` com
`{"function": "balance_of", "args": ["tc1…"]}` — argumentos em texto (inteiros em
decimal ou `2.5tcn`, bytes `0x…`, endereços bech32m, listas `[1, 2]`).

## 7. Privacidade (pools TCCL-PRIV-1)

Privacidade na The Coin é **opcional e feita por contratos**: um pool recebe
depósitos de valor fixo associados a chaves públicas de anel e permite saques
para qualquer endereço provando, com uma **assinatura em anel (bLSAG)**, que o
saque pertence a *um* dos depósitos — sem revelar qual. A *key image* impede
sacar o mesmo depósito duas vezes. Exemplo completo:
[`private_pool.tccl`](https://github.com/LucasBolla94/tccl/blob/v0.3.0/examples/private_pool.tccl). Primitiva: [PROTOCOL.md §17.10](PROTOCOL.md#1710-ring_verify-assinaturas-em-anel).

### 7.1 Chaves de anel

```
seed32     = SLIP-0010(seed, m/44'/7333'/0'/7'/index')             // mesmo algoritmo da §2
x          = Scalar::from_hash(SHA-512("TheCoin:ring:secret\0" || seed32))   // 64 bytes reduzidos mod ℓ
P          = x · G                      (Ristretto255 comprimido, 32 bytes) → depositado no pool
key_image  = x · Hp(P),  Hp(P) = RistrettoPoint::hash_from_bytes::<SHA-512>("TheCoin:ring:hash-to-point\0" || P)
```

Use um `index` diferente para cada depósito (a carteira de referência:
`privacy deposit --key N`).

### 7.2 Interface TCCL-PRIV-1

| Função | Tipo | Assinatura |
|---|---|---|
| `denomination()` | view | `-> int` (motes por depósito) |
| `deposits()` | view | `-> int` (quantidade de chaves depositadas) |
| `key_at(index: int)` | view | `-> bytes` (chave pública de 32 bytes) |
| `is_withdrawn(key_image: bytes)` | view | `-> bool` |
| `message_for(to: address, relayer: address, fee: int)` | view | `-> bytes` (mensagem a assinar) |
| `deposit(public_key: bytes)` | action **payable** | `value` deve ser exatamente `denomination()` |
| `withdraw(to: address, relayer: address, fee: int, members: list[int], signature: bytes, key_image: bytes)` | action | paga `denomination − fee` a `to` e `fee` a `relayer` |

No pool de exemplo, `message_for` = `BLAKE3(endereço_do_pool(20) || to(20) || relayer(20) || i128_be(fee))`
e anéis têm de 2 a 32 membros. Carteiras devem **sempre** chamar `message_for`
em vez de recalcular.

### 7.3 Fluxo de saque

1. `deposits()` e `key_at(i)` para **todos** os índices; ache o índice da sua chave
   localmente (baixar só até encontrar revela ao nó qual depósito é seu).
2. `is_withdrawn(key_image)` deve ser `false`.
3. Escolha aleatoriamente `k − 1` outros índices (k entre 2 e 32; a carteira de
   referência usa 16) e embaralhe junto com o seu → `members`; `ring` = as chaves nessa ordem.
4. `msg = message_for(to, relayer, fee)`; assine com bLSAG na posição do seu
   índice: `signature = c_0 || r_0 … r_{k−1}` (32 × (k + 1) bytes).
5. Envie `Invoke withdraw(...)` com `value = 0`, medindo o combustível por
   simulação (§6.3). `ring_verify` custa `5 000 + 10 000 × k` de combustível.

Cuidados: o endereço que **envia** a transação de saque é público — use um
*relayer* ou um endereço sem ligação com o depósito; todos os depósitos têm o
mesmo valor; espere outros depósitos entrarem antes de sacar.


> **Não vaze a privacidade pelo nó.** As consultas (`view`) feitas a um nó revelam o que a
> carteira pergunta. Baixe **todas** as chaves do pool (`deposits` + `key_at` de 0 a n−1,
> sem parar na sua) e monte o anel localmente — é o que a carteira de referência faz. A
> consulta `is_withdrawn(key_image)` e o envio do saque ligam seu IP à operação: para
> privacidade forte use um nó próprio (ou Tor) e um relayer para enviar o saque.

## 8. Vetores de teste

Use estes valores em testes automatizados da sua carteira. Todos são verificados
pelo teste `crates/wallet/tests/vectors.rs`
(`cargo test -p thecoin-wallet --test vectors -- --nocapture`).

### 8.1 Chaves e endereços

Frase: `abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about`
Passphrase: vazia

```
seed (BIP-39) = 5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4
chave mestra SLIP-0010 (caminho m) = 560f9f3c94558b6551928bb781cf6092c6b8800b4fc544af2c9444ed126d51aa

m/44'/7333'/0'/0'/0'
  secret_key = a146f5dbeff18a4189d3bb0a03c636252ee07dba1fbd6e2e954fa3feff27db97
  public_key = 67a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5dbe06
  address20  = 4d54f2145ebf35d8509935f16fb1ebb7efa47d64
  mainnet    = tc1f420y9z7hu6as5yexhcklv0tklh6glty0n9yvj
  testnet    = tct1f420y9z7hu6as5yexhcklv0tklh6gltyfk2we0
  regtest    = tcr1f420y9z7hu6as5yexhcklv0tklh6glty6qra7u

m/44'/7333'/0'/0'/1'
  secret_key = 34e4f81414e800631d613d26011450e307870118a6ca68a99d2ff833cebbd028
  public_key = c4595ca32d93a95c85896b9988ee0d2181d867b892433358314f7887f365cf5a
  address20  = 4200a67d1774885d87731decb1c4743c3b070454
  mainnet    = tc1ggq2vlghwjy9mpmnrhktr3r58saswpz5pc78u4
  testnet    = tct1ggq2vlghwjy9mpmnrhktr3r58saswpz58a3dfg
  regtest    = tcr1ggq2vlghwjy9mpmnrhktr3r58saswpz55tc7wm

Chave de anel m/44'/7333'/0'/7'/0'
  secret     = 39d100ebdc3a849a4c856e976d4a3d684b70cb5fd9368915937e3a17710b9d09
  public     = 34fde0fcd54780a5d77aca8583e9ff0af6cf676a6e81a28d45e5cda3bd446971
  key_image  = 6a51935c920b9d18bd046c0383e9168f3b86d4bd1bee81a1924cd85413a4f75e
```

A implementação SLIP‑0010 também passa nos vetores oficiais do padrão
(teste `slip10_vector` em `crates/wallet/src/keys.rs`).

### 8.2 Transação `Transfer` assinada

Parâmetros: mainnet, remetente = índice 0, `flags = 0`, `nonce = 0`,
`fee = 2630` (mínima a 1,0×), `expiry_height = 0`, `to` = endereço do índice 1,
`amount = 150000000` (1,5 TCN), `memo = "test"`.

```
body (67 bytes) =
0101004354000000000000000000460a0000000000000000000000000000004200a67d1774885d87731decb1c4743c3b07045480d1f008000000000400000074657374

signing_hash = e81ec27086aa0dfe1bfc204fade8dee9290eefef33b511bc865eb32770fd6fed
signature    = 16e8dec69deadd63934267d3a321def787aa20ad7ceb09633a2440f1e67a167fcdeb13b4e0375490de0bfa1e0d445c88babf44d205545000546376bd7c274409

tx (163 bytes) =
0101004354000000000000000000460a0000000000000000000000000000004200a67d1774885d87731decb1c4743c3b07045480d1f00800000000040000007465737467a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5dbe0616e8dec69deadd63934267d3a321def787aa20ad7ceb09633a2440f1e67a167fcdeb13b4e0375490de0bfa1e0d445c88babf44d205545000546376bd7c274409

txid   = 492e5c610cd3913d94a20a2cd5ddc076cec7d3f3b8abcc718923fdc9fe595d02
sender = tc1f420y9z7hu6as5yexhcklv0tklh6glty0n9yvj
taxa com prioridade normal (12 500 bp) = 3288
```

Decomposição do corpo:

| Bytes (hex) | Campo |
|---|---|
| `01` | version = 1 |
| `01004354` | chain_id = 0x54430001 (LE) |
| `00` | flags = 0 |
| `0000000000000000` | nonce = 0 |
| `460a000000000000` | fee = 2630 |
| `0000000000000000` | expiry_height = 0 |
| `00` | action = Transfer |
| `4200a67d1774885d87731decb1c4743c3b070454` | to (20 bytes) |
| `80d1f00800000000` | amount = 150 000 000 |
| `04000000` | memo length = 4 |
| `74657374` | memo = "test" |

Ed25519 é determinístico: a mesma chave e mensagem sempre geram a mesma assinatura.

### 8.3 Transação substituível (`FLAG_REPLACEABLE`)

Mesmos parâmetros, com `flags = 1` e `nonce = 1` (mesmo tamanho, 163 bytes, `fee = 2630`):

```
body =
0101004354010100000000000000460a0000000000000000000000000000004200a67d1774885d87731decb1c4743c3b07045480d1f008000000000400000074657374

tx =
0101004354010100000000000000460a0000000000000000000000000000004200a67d1774885d87731decb1c4743c3b07045480d1f00800000000040000007465737467a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5dbe06a51c9ba4cbad1ffae073abd7b0418daff5aff0359998f74f5018f80f7490bd677b801e24a7336f0df63040f6a01ac87e952cebc913be442c088332c0f4165809

txid = e3d311122d703363416d23c266dde108b19a47b756359117b3d388f9cfa7dcbe
taxa mínima de uma substituta = 2630 + floor(2630 / 4) = 3287
```

### 8.4 Transação `Invoke`

Parâmetros: mainnet, remetente = índice 0, `flags = 0`, `nonce = 2`,
`contract = program_address(remetente, 0)`, `function = "transfer"`,
`args = [Address(índice 1), Int(2500)]`, `value = 0`, `max_fuel = 20000`,
`max_deposit = 100000`, `fee = 23250` (mínima a 1,0×).

```
program_address(remetente, nonce 0) = ae2db50538a731352cb1d695dbba1e4eeac5ffd4
                                    = tc14ckm2pfc5ucn2t93662ahws7fm4vtl75u534g7

body (129 bytes) =
0101004354000200000000000000d25a000000000000000000000000000007ae2db50538a731352cb1d695dbba1e4eeac5ffd4080000007472616e7366657202000000044200a67d1774885d87731decb1c4743c3b07045400c40900000000000000000000000000000000000000000000204e000000000000a086010000000000

tx (225 bytes) =
0101004354000200000000000000d25a000000000000000000000000000007ae2db50538a731352cb1d695dbba1e4eeac5ffd4080000007472616e7366657202000000044200a67d1774885d87731decb1c4743c3b07045400c40900000000000000000000000000000000000000000000204e000000000000a08601000000000067a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5dbe060a3277de1e233517d871382e9c26e9b9728a902033ae9fbc49ae8165c1a1dedb4f801f92127eeea68f254bec07b3addfef9ca49c99dd5200fb17ae5e259a670d

txid = 8d6fe1b36f4470a75d111c42b8d1f96e31113e90b03a60b09070df6c0518981b
```

Decomposição da ação:

| Bytes (hex) | Campo |
|---|---|
| `07` | action = Invoke |
| `ae2db50538a731352cb1d695dbba1e4eeac5ffd4` | contract (20 bytes) |
| `08000000` `7472616e73666572` | function = "transfer" |
| `02000000` | 2 argumentos |
| `04` `4200a67d1774885d87731decb1c4743c3b070454` | `Value::Address` |
| `00` `c4090000000000000000000000000000` | `Value::Int(2500)` (i128 LE) |
| `0000000000000000` | value = 0 |
| `204e000000000000` | max_fuel = 20 000 |
| `a086010000000000` | max_deposit = 100 000 |

### 8.5 Ids e hashes

```
contract_id(sender índice 0, nonce 0) = 81b9fbe7ce8cc6ee852d1b0a8df820e5279e127865bf00a51dd03872f3773f6a
proposal_id(sender índice 0, nonce 0) = e7aa283fb54f5d9c6c95c57c1f148faf22f0eebc91cb905b473f144ec6675903
tagged_hash("txid", "")               = 38aa34a475f8d3b9c7b15275271dce321fc9e6835e273e6672c9ec3c4b653b6a
tagged_hash("block", "abc")           = 09bb3b792db7c48af43cdb15ba3f1b274440d964372789fadcc4d8da9c1679d8
merkle_root([])                       = 5740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc1
merkle_root([txid da §8.2])           = 53e021906eeffc0562bf1e9e52249c620d33a011d09a322d7d92147b17eff617
```

## 9. URI de pagamento

```
thecoin:<endereço>?amount=<TCN decimal>&memo=<texto>&label=<texto>
```

* `amount` em TCN decimal com até 8 casas (`12.5`), nunca notação científica.
* `memo` e `label` com *percent-encoding* (UTF‑8); `+` é decodificado como espaço.
* Parâmetros desconhecidos devem ser ignorados.
* Exemplo: `thecoin:tc1ggq2vlghwjy9mpmnrhktr3r58saswpz5pc78u4?amount=3.5&memo=Pedido%20%23123&label=Loja`

Lojas exibem a URI como QR code; a carteira preenche destino, valor e memo
(o memo vai no campo `memo` da `Transfer`, até 256 bytes).

## 10. Arquivo de carteira (keystore v1)

Formato usado por `thecoin-wallet` (`crates/wallet/src/keystore.rs`). Outras
carteiras de desktop podem ler/gravar o mesmo arquivo.

```json
{
  "version": 1,
  "network": "mainnet",
  "kdf": { "name": "argon2id", "m_kib": 65536, "t": 3, "p": 1, "salt": "<16 bytes hex>" },
  "cipher": { "name": "chacha20poly1305", "nonce": "<12 bytes hex>", "ciphertext": "<hex>" },
  "accounts": [ { "index": 0, "label": "default" } ],
  "node_url": "http://127.0.0.1:7334"
}
```

* `key = Argon2id(password UTF-8, salt, m = m_kib, t, p, version 0x13, len 32)`
* `plaintext = JSON {"mnemonic": "<frase>", "passphrase": "<passphrase BIP-39>"}`
* `ciphertext = ChaCha20-Poly1305(key, nonce, plaintext, AAD = "thecoin-wallet-v1")` (inclui a tag de 16 bytes no final)
* Senha errada = falha de autenticação da AEAD.
* `accounts[].index` = índices derivados (`m/44'/7333'/0'/0'/index'`); `node_url` é opcional.
* Grave com permissão `0600` e de forma atômica (arquivo temporário + rename).
* A carteira de referência guarda, ao lado, `<arquivo>.sent.jsonl` com as
  transações enviadas (txid, índice e hex), usado por `bump-fee`.

## 11. Recomendações de segurança

* **Nunca** envie a frase, a seed ou chaves privadas (inclusive de anel) a um
  servidor — nem ao nó. O nó só precisa da transação assinada.
* Use o gerador aleatório criptográfico do sistema operacional.
* Mostre a frase uma única vez na criação e peça confirmação de algumas palavras.
* Criptografe o armazenamento local (keystore v1 ou o cofre seguro do sistema:
  Keychain, Android Keystore). Apague buffers de chave da memória após o uso.
* Exiba ao usuário, antes de assinar: destino completo, valor, taxa (e
  prioridade), memo, se é substituível e, para contratos, as condições (prazos
  em blocos convertidos para tempo aproximado: 1 bloco ≈ 15 segundos), `value`,
  `max_fuel` e `max_deposit`.
* Simule toda chamada de contrato antes de enviar (§6.3).
* Valide endereço e HRP da rede; nunca "corrija" checksums automaticamente.
* Não confie num único nó para saldos altos: consulte dois nós
  (ex.: seed1 e seed2) e compare `tip`/saldo. Para privacidade, prefira um nó próprio.
* Use HTTPS ao falar com nós remotos.
* Para HTLC, gere a pré-imagem com 32 bytes aleatórios e guarde-a até o resgate.
