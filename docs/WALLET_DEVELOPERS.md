# Guia para desenvolvedores de carteiras

Este guia explica como construir uma carteira **compatível** com a The Coin em
qualquer linguagem (JavaScript, Kotlin, Swift, Go, Python…). Seguindo-o, sua
carteira gera os mesmos endereços que a carteira de referência
(`thecoin-wallet`) a partir das mesmas 24 palavras, e produz transações aceitas
por qualquer nó.

A implementação de referência está em `crates/wallet` (chaves, keystore,
construção de transações, cliente HTTP, URIs) e `crates/core` (formatos). A
especificação completa dos formatos está em [PROTOCOL.md](PROTOCOL.md).

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
A carteira de referência usa `account = 0` e incrementa `index`.

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
para mainnet por engano.

## 4. Construindo uma transação

### 4.1 Consultar a conta e a taxa

```bash
GET /api/v1/address/{remetente}   → use "next_nonce" e "spendable"
GET /api/v1/fees                  → use "suggested_fee_per_byte"
```

* `nonce` da transação = `next_nonce` (já conta as transações pendentes no mempool).
* Se enviar várias em sequência sem esperar confirmação, incremente o nonce localmente.

### 4.2 Serializar o corpo (Borsh)

Todos os inteiros little-endian. Estrutura de `TxBody`:

| Campo | Tipo | Bytes |
|---|---|---|
| `version` | u8 | 1 (sempre `0x01`) |
| `chain_id` | u32 | 4 (`0x54430001` mainnet, `…02` testnet, `…03` regtest) |
| `nonce` | u64 | 8 |
| `fee` | u64 | 8 |
| `expiry_height` | u64 | 8 (0 = não expira) |
| `action` | enum | 1 byte de índice + campos |

`Transfer` (índice `0x00`): `to` (20 bytes) `|| amount` (u64) `|| memo` (u32 tamanho + bytes).

Para as outras ações veja [PROTOCOL.md §5](PROTOCOL.md#5-transações).

### 4.3 Assinar

```
signing_hash = BLAKE3("TheCoin:tx-sign" || 0x00 || body_bytes)
signature    = Ed25519.sign(secret_key, signing_hash)      // assina os 32 bytes do hash
tx_bytes     = body_bytes || public_key(32) || signature(64)
txid         = BLAKE3("TheCoin:txid" || 0x00 || tx_bytes)
```

### 4.4 Truque da taxa

A taxa mínima é `min_fee_per_byte × len(tx_bytes)`. Como `fee` é um u64 de
tamanho fixo, o tamanho **não muda** com o valor da taxa:

```
1. monte o corpo com fee = 0, assine, meça size = len(tx_bytes)
2. fee = size × fee_per_byte
3. monte o corpo com essa fee e assine de novo
```

Uma transferência com memo de 4 bytes tem 162 bytes; com `min_fee_per_byte = 10`
(padrão da mainnet) a taxa mínima é 1 620 motes (0,0000162 TCN).

### 4.5 Enviar

```bash
curl -X POST https://the-coin.cloud/api/v1/tx \
  -H 'content-type: application/json' \
  -d '{"tx":"<hex de tx_bytes>"}'
# → {"txid":"…"}
```

O nó valida tudo antes de aceitar; mensagens de erro estão em [API.md](API.md#post-apiv1tx).

### 4.6 Acompanhar confirmações

```bash
GET /api/v1/tx/{txid}
```

* `in_mempool: true` → aguardando bloco.
* `block_height` preenchido → confirmada; `confirmations` = `tip − altura + 1`.
* 404 depois de estar no mempool → foi descartada (ex.: substituída por RBF,
  expirou, ou conflito de nonce); reconstrua com o `next_nonce` atual.

Recomendação de UX: 1 confirmação para valores pequenos, **6** (~6 min) para
valores comuns e **30+** para valores altos. Recompensas de mineração só são
gastáveis após 100 blocos (`immature` na API).

### 4.7 Substituição (RBF) e expiração

* Reenviar com o **mesmo nonce** e taxa ≥ 125 % da anterior substitui a pendente.
* `expiry_height` (ex.: `tip + 1440`) garante que uma transação esquecida não
  seja minerada dias depois.

## 5. Contratos e governança

* Ids determinísticos (a carteira pode mostrar antes da confirmação):
  ```
  contract_id = BLAKE3("TheCoin:contract-id" || 0x00 || remetente20 || u64_le(nonce))
  proposal_id = BLAKE3("TheCoin:proposal-id" || 0x00 || remetente20 || u64_le(nonce))
  ```
  (`nonce` = nonce da transação que cria.) A API também devolve em `created`.
* **Débito total a verificar antes de enviar:**
  `Transfer: amount + fee`, `BatchTransfer: Σ + fee`,
  `CreateContract: funding + fee` (subscription: `amount_per_period × max_periods`),
  `MultisigDeposit: amount + fee`, `Propose: proposal_deposit + fee`
  (valor em `/api/v1/status → params.proposal_deposit`), `Vote: fee`
  (mas `weight` fica bloqueado até o fim da votação).
* O voto pode entrar no mesmo bloco em que a proposta foi criada, desde que
  venha depois do `Propose` (nonce maior do mesmo remetente, ou transação
  posterior no bloco). O campo `created` de `GET /api/v1/tx/{txid}` já traz o id
  da proposta mesmo antes da confirmação.
* HTLC usa **SHA‑256** (compatível com Bitcoin/Lightning para atomic swaps).

Guias de uso: [CONTRACTS.md](CONTRACTS.md) e [GOVERNANCE.md](GOVERNANCE.md).

## 6. URI de pagamento

```
thecoin:<endereço>?amount=<TCN decimal>&memo=<texto>&label=<texto>
```

* `amount` em TCN decimal com até 8 casas (`12.5`), nunca notação científica.
* `memo` e `label` com *percent-encoding* (UTF‑8); `+` é decodificado como espaço.
* Parâmetros desconhecidos devem ser ignorados.
* Exemplo: `thecoin:tc1ggq2vlghwjy9mpmnrhktr3r58saswpz5pc78u4?amount=3.5&memo=Pedido%20%23123&label=Loja`

Lojas exibem a URI como QR code; a carteira preenche destino, valor e memo
(o memo vai no campo `memo` da `Transfer`, até 256 bytes).

## 7. Arquivo de carteira (keystore v1)

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

## 8. Vetores de teste

Use estes valores em testes automatizados da sua carteira.

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
```

A implementação SLIP‑0010 também passa nos vetores oficiais do padrão
(teste `slip10_vector` em `crates/wallet/src/keys.rs`).

### 8.2 Transação `Transfer` assinada

Parâmetros: mainnet, remetente = índice 0, `nonce = 0`, `fee = 1700`,
`expiry_height = 0`, `to` = endereço do índice 1, `amount = 150000000` (1,5 TCN),
`memo = "test"`.

```
body (66 bytes) =
01010043540000000000000000a4060000000000000000000000000000004200a67d1774885d87731decb1c4743c3b07045480d1f008000000000400000074657374

signing_hash = ea1cc60cc239fb024c89cca9088fbef6581f37fbd60bcf8fa769447804bf830c
signature    = 5a82e2fc01cdc6737e1e3ff43609223607d9a38284f02b231f6609049d9558e0d2608ead839e8cd0cc100b7a958687bb7361c6dfff5734abd9f2c137b0653e0e

tx (162 bytes) =
01010043540000000000000000a4060000000000000000000000000000004200a67d1774885d87731decb1c4743c3b07045480d1f00800000000040000007465737467a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5dbe065a82e2fc01cdc6737e1e3ff43609223607d9a38284f02b231f6609049d9558e0d2608ead839e8cd0cc100b7a958687bb7361c6dfff5734abd9f2c137b0653e0e

txid   = b83869153316a120559536c0d1f102a59520abae631a7c8a72f504382ea70b7e
sender = tc1f420y9z7hu6as5yexhcklv0tklh6glty0n9yvj
```

Decomposição do corpo:

| Bytes (hex) | Campo |
|---|---|
| `01` | version = 1 |
| `01004354` | chain_id = 0x54430001 (LE) |
| `0000000000000000` | nonce = 0 |
| `a406000000000000` | fee = 1700 |
| `0000000000000000` | expiry_height = 0 |
| `00` | action = Transfer |
| `4200a67d1774885d87731decb1c4743c3b070454` | to (20 bytes) |
| `80d1f00800000000` | amount = 150 000 000 |
| `04000000` | memo length = 4 |
| `74657374` | memo = "test" |

Ed25519 é determinístico: a mesma chave e mensagem sempre geram a mesma assinatura.

### 8.3 Ids e hashes

```
contract_id(sender índice 0, nonce 0) = 81b9fbe7ce8cc6ee852d1b0a8df820e5279e127865bf00a51dd03872f3773f6a
proposal_id(sender índice 0, nonce 0) = e7aa283fb54f5d9c6c95c57c1f148faf22f0eebc91cb905b473f144ec6675903
tagged_hash("txid", "")               = 38aa34a475f8d3b9c7b15275271dce321fc9e6835e273e6672c9ec3c4b653b6a
tagged_hash("block", "abc")           = 09bb3b792db7c48af43cdb15ba3f1b274440d964372789fadcc4d8da9c1679d8
merkle_root([])                       = 5740f3f044b5290cbda05ef48cf56fad55c4cab20e8202b66cbb84c7968e9fc1
merkle_root([txid acima])             = 34aa3be8522e9cf24d0a9fbb293e5a66af0a27e98144c3133737192c09e69ec5
```

## 9. Recomendações de segurança

* **Nunca** envie a frase, a seed ou chaves privadas a um servidor — nem ao nó.
  O nó só precisa da transação assinada.
* Use o gerador aleatório criptográfico do sistema operacional.
* Mostre a frase uma única vez na criação e peça confirmação de algumas palavras.
* Criptografe o armazenamento local (keystore v1 ou o cofre seguro do sistema:
  Keychain, Android Keystore). Apague buffers de chave da memória após o uso.
* Exiba ao usuário, antes de assinar: destino completo, valor, taxa, memo e,
  para contratos, as condições (prazos em blocos convertidos para tempo
  aproximado: 1 bloco ≈ 1 minuto).
* Valide endereço e HRP da rede; nunca "corrija" checksums automaticamente.
* Não confie num único nó para saldos altos: consulte dois nós
  (ex.: seed1 e seed2) e compare `tip`/saldo.
* Use HTTPS ao falar com nós remotos.
* Para HTLC, gere a pré-imagem com 32 bytes aleatórios e guarde-a até o resgate.
