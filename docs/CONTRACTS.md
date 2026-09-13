# Contratos de pagamento

A The Coin v0.1 traz **cinco modelos de contrato nativos**, executados pelo
consenso da rede. Não há código arbitrário: cada modelo tem regras fixas e
auditáveis, custo previsível (só a taxa da transação, sem "gas") e nenhum
risco de loop infinito ou reentrância — ideal para máquinas modestas.

| Modelo | Para quê |
|---|---|
| **Escrow** | compra e venda com garantia, com ou sem árbitro |
| **Vesting** | salários, bônus e distribuição de tokens liberados ao longo do tempo |
| **Subscription** | assinaturas e mensalidades pré-pagas |
| **HTLC** | pagamentos condicionados a um segredo; *atomic swaps* com outras blockchains |
| **Multisig** | cofre compartilhado que exige M de N aprovações |

Especificação exata: [PROTOCOL.md §16](PROTOCOL.md#16-contratos-de-pagamento).
Código: `crates/core/src/contracts.rs`.

## Conceitos comuns

* **Tempo em blocos.** Prazos são alturas de bloco. 1 bloco ≈ 1 minuto:
  60 ≈ 1 hora, 1 440 ≈ 1 dia, 10 080 ≈ 1 semana, 43 200 ≈ 30 dias.
  A carteira converte "daqui a N blocos" em altura absoluta.
* **Fundos no contrato.** Ao criar, o valor sai do seu saldo e fica guardado no
  contrato (`balance`). Só sai pelas regras do modelo.
* **Id do contrato.** Determinístico: `tagged_hash("contract-id", criador || nonce)`.
  Depois de enviar, veja com `thecoin-wallet tx <txid>` (campo `created`).
* **Contratos concluídos** (saldo zero) são removidos do estado para mantê-lo
  compacto. O histórico continua nas transações. Multisig nunca é removido.
* **Taxas.** Toda criação ou chamada paga a taxa normal da transação, por
  quem envia.
* **Consultar.** `thecoin-wallet contract show <id>` ou `GET /api/v1/contract/{id}`.

Opções globais úteis: `--from N` (endereço da carteira), `-y` (sem confirmação),
`--dry-run` (só mostra a transação assinada), `--network testnet`.

---

## 1. Escrow (garantia)

**Regras**

| Ação | Quem | Resultado |
|---|---|---|
| criar | comprador (pagador) | trava `amount` |
| `escrow-release` | pagador **ou** árbitro | todo o saldo → vendedor |
| `escrow-refund` | vendedor **ou** árbitro a qualquer momento; pagador só **após o prazo** | todo o saldo → pagador |

Restrições: vendedor ≠ pagador; árbitro (opcional) ≠ vendedor e ≠ pagador; prazo no futuro.

**CLI**

```bash
# comprador trava 250 TCN por até 7 dias, com árbitro
thecoin-wallet contract escrow-create \
  --payee tc1vendedor... --amount 250 --deadline-blocks 10080 --arbiter tc1arbitro...

thecoin-wallet contract escrow-release <id>   # comprador recebeu o produto
thecoin-wallet contract escrow-refund <id>    # vendedor desistiu / árbitro decidiu / prazo venceu
```

**Cenário — e-commerce.** Uma loja online exibe o pedido #9812. O cliente cria
o escrow com a loja como `payee` e um serviço de mediação como `arbiter`. A loja
vê o contrato na blockchain (valor garantido) e despacha. Ao receber, o cliente
libera. Se o produto não chegar, o árbitro devolve; se ninguém fizer nada, o
cliente recupera o valor sozinho após o prazo.

## 2. Vesting (liberação gradual)

**Regras**

* `amount` é liberado **linearmente** entre `start` e `end`; nada antes do `cliff`.
  ```
  liberado(h) = 0 se h < cliff;  amount se h ≥ end;  amount × (h − start)/(end − start) no meio
  ```
* `vesting-claim`: só o beneficiário; saca o liberado ainda não sacado.
* `vesting-revoke`: só o criador e só se `--revocable`; o beneficiário recebe o
  que já foi liberado e o criador recupera o resto.

**CLI**

```bash
# 12 000 TCN ao longo de 1 ano (525 600 blocos), cliff de 3 meses, revogável
thecoin-wallet contract vesting-create \
  --beneficiary tc1funcionario... --amount 12000 \
  --start-in 0 --cliff-blocks 129600 --duration-blocks 525600 --revocable

thecoin-wallet --from 0 contract vesting-claim <id>    # beneficiário, quando quiser
thecoin-wallet contract vesting-revoke <id>            # empresa, se o contrato de trabalho terminar
```

(`--start-in` = blocos a partir de agora até o início; `--cliff-blocks` e
`--duration-blocks` contados a partir do início.)

**Cenário — salário/bônus de colaborador.** Uma startup reserva o bônus anual
de um desenvolvedor. Ele pode acompanhar pela blockchain quanto já liberou e
sacar a qualquer momento; se sair antes do cliff, a empresa revoga e recupera tudo.

## 3. Subscription (assinatura pré-paga)

**Regras**

* O assinante deposita `amount × periods` na criação.
* O **primeiro período já pode ser sacado** na criação; depois, um novo a cada `period-blocks`.
* `subscription-claim`: só o prestador (payee); saca todos os períodos disponíveis.
* `subscription-cancel`: só o assinante; o prestador recebe os períodos já
  disponíveis e não sacados, o assinante recebe de volta os períodos futuros.

**CLI**

```bash
# 30 TCN por mês (43 200 blocos), 12 meses (trava 360 TCN)
thecoin-wallet contract subscription-create \
  --payee tc1saas... --amount 30 --period-blocks 43200 --periods 12

thecoin-wallet contract subscription-claim <id>    # prestador, todo mês
thecoin-wallet contract subscription-cancel <id>   # assinante cancela
```

**Cenário — SaaS / academia / streaming.** O cliente pré-paga um ano. O
prestador tem garantia de recebimento mês a mês; o cliente pode cancelar e
receber de volta os meses que ainda não começaram, sem depender da empresa.

## 4. HTLC (pagamento com hash e prazo)

**Regras**

* O remetente trava `amount` com um `hash-lock = SHA-256(segredo)` e um prazo.
* `htlc-redeem --preimage <segredo>`: **qualquer um** pode enviar, até o prazo
  (inclusive); os fundos vão sempre ao `recipient`.
* `htlc-refund`: qualquer um, **depois** do prazo; os fundos voltam ao remetente.
* Pré-imagem de até 64 bytes. SHA-256 é compatível com Bitcoin e Lightning.

**CLI**

```bash
# sem --hash-lock a carteira gera e mostra um segredo aleatório de 32 bytes
thecoin-wallet contract htlc-create --recipient tc1bob... --amount 100 --timeout-blocks 1440
# com hash conhecido (ex.: vindo de outra blockchain)
thecoin-wallet contract htlc-create --recipient tc1bob... --amount 100 \
  --hash-lock <sha256_hex> --timeout-blocks 720

thecoin-wallet contract htlc-redeem <id> --preimage <segredo_hex>
thecoin-wallet contract htlc-refund <id>
```

**Cenário — atomic swap TCN ↔ BTC.** Alice tem TCN e quer BTC; Bob tem BTC.

1. Alice gera um segredo `s` e calcula `H = SHA-256(s)`.
2. Alice cria um HTLC na The Coin: 1 000 TCN para Bob, `hash-lock H`, prazo de **48 h** (2 880 blocos).
3. Bob confere o contrato e cria na Bitcoin um HTLC (script `OP_SHA256 H ...`) pagando Alice, com prazo **menor** (24 h).
4. Alice resgata os BTC revelando `s` na Bitcoin.
5. Bob lê `s` na blockchain do Bitcoin e executa `htlc-redeem` na The Coin, recebendo os TCN.

Se alguém desistir, cada um recupera seus fundos após o próprio prazo. O prazo
maior do lado de quem gerou o segredo é essencial para a segurança.

## 5. Multisig (cofre M de N)

**Regras**

* Criação com até **16 signatários** distintos e `threshold` entre 1 e N; depósito inicial opcional.
* `multisig-deposit`: **qualquer um** pode depositar.
* `multisig-propose`: um signatário propõe um pagamento (conta como a 1ª aprovação).
  Com `threshold = 1` o pagamento sai na hora.
* `multisig-approve`: outro signatário aprova; ao atingir o limiar o pagamento
  é executado (precisa haver saldo, senão a aprovação é recusada).
* `multisig-cancel`: só quem propôs cancela um pagamento pendente.
* Até 16 pagamentos pendentes por cofre. O cofre nunca é removido.

**CLI**

```bash
# cofre 2-de-3 com 5 000 TCN
thecoin-wallet contract multisig-create \
  --signers tc1ana...,tc1bruno...,tc1carla... --threshold 2 --deposit 5000

thecoin-wallet contract multisig-deposit <id> 1000
thecoin-wallet contract multisig-propose <id> --to tc1fornecedor... --amount 800 --memo "NF 4512"
thecoin-wallet contract show <id>                        # veja o spend_id pendente
thecoin-wallet --from 0 contract multisig-approve <id> 0 # segundo signatário aprova → pago
thecoin-wallet contract multisig-cancel <id> 0           # proponente desiste
```

**Cenário — tesouraria de empresa ou associação.** Três sócios controlam o
caixa. Qualquer pagamento precisa de dois deles. Clientes pagam depositando
direto no cofre. Nenhum sócio sozinho consegue mover os fundos, e a perda de uma
chave não bloqueia o dinheiro.

---

## Integração para desenvolvedores

* Ações e campos binários: [PROTOCOL.md §5](PROTOCOL.md#5-transações).
* Construção em Rust: `thecoin_wallet::builder::{create_contract, call_contract}` com
  `thecoin_core::contracts::{ContractSpec, ContractCall}`.
* Estado legível (incluindo `vested_now` e `claimable_periods_now`): [API.md](API.md#get-apiv1contractid).
* Histórico: `GET /api/v1/address/{addr}/txs` inclui criações e chamadas de contratos em que o endereço é parte.
