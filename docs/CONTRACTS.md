# Contratos na The Coin

A The Coin oferece dois níveis de contratos, ambos executados pelo consenso
da rede:

1. **Contratos de pagamento nativos** — cinco modelos prontos (escrow, vesting,
   assinatura, HTLC, multisig), com regras fixas e auditáveis, sem código
   arbitrário e sem combustível: o custo é só a taxa da transação e um pequeno
   depósito reembolsável.
2. **Contratos inteligentes TCCL** — programas escritos em **TCCL** (The Coin
   Cloud Language), uma linguagem pequena, tipada e determinística, com
   medição de combustível (*fuel*), eventos, visões gratuitas e assinaturas em
   anel para **privacidade opcional**. A rede compila e executa a **versão 2**
   da linguagem: records, enums com transições, papéis, interfaces e chamadas
   entre contratos, módulos com biblioteca padrão e **upgrades** com autoridade
   (§6.7). Contratos da versão 1 continuam rodando exatamente como antes.

| Nativo | Para quê |
|---|---|
| **Escrow** | compra e venda com garantia, com ou sem árbitro |
| **Vesting** | salários, bônus e distribuição de tokens liberados ao longo do tempo |
| **Subscription** | assinaturas e mensalidades pré-pagas |
| **HTLC** | pagamentos condicionados a um segredo; *atomic swaps* com outras blockchains |
| **Multisig** | cofre compartilhado que exige M de N aprovações |

Especificação exata: [PROTOCOL.md §16](PROTOCOL.md#16-contratos-de-pagamento)
(nativos) e [PROTOCOL.md §17](PROTOCOL.md#17-contratos-inteligentes-tccl) (TCCL).
Código: `crates/core/src/contracts.rs` e `crates/core/src/programs.rs`.

A linguagem vive num repositório próprio, **[github.com/LucasBolla94/tccl](https://github.com/LucasBolla94/tccl)**,
fixado em `Cargo.toml` na tag **v0.3.0** (compilador, VM, simulador, biblioteca
padrão e a ferramenta `tccl`). Referência da linguagem em português:
[docs/pt-BR/language.md](https://github.com/LucasBolla94/tccl/blob/v0.3.0/docs/pt-BR/language.md),
[chamadas entre contratos](https://github.com/LucasBolla94/tccl/blob/v0.3.0/docs/pt-BR/calls.md),
[módulos](https://github.com/LucasBolla94/tccl/blob/v0.3.0/docs/pt-BR/modules.md),
[papéis](https://github.com/LucasBolla94/tccl/blob/v0.3.0/docs/pt-BR/permissions.md),
[upgrades](https://github.com/LucasBolla94/tccl/blob/v0.3.0/docs/pt-BR/upgrades.md) e
[combustível e taxas](https://github.com/LucasBolla94/tccl/blob/v0.3.0/docs/pt-BR/fees.md).
As cópias locais [docs/tccl/TCCL.md](tccl/TCCL.md) e o livro de receitas
[docs/tccl/tccl-cookbook.pdf](tccl/tccl-cookbook.pdf) descrevem a versão 1 da
linguagem e continuam válidas para o que a versão 2 não mudou.

## Conceitos comuns

* **Tempo em blocos.** Prazos são alturas de bloco. **1 bloco = 15 segundos**:
  240 = 1 hora, 5 760 = 1 dia, 40 320 = 1 semana, 172 800 = 30 dias,
  2 102 400 = 1 ano. A carteira converte "daqui a N blocos" em altura absoluta.
  Padrões da carteira: `--deadline-blocks` 40 320 (≈ 7 dias, escrow) e
  `--timeout-blocks` 5 760 (≈ 1 dia, HTLC); `--period-blocks` não tem padrão.
* **Fundos no contrato.** Ao criar, o valor sai do seu saldo e fica guardado no
  contrato (`balance`). Só sai pelas regras do modelo (ou do código TCCL).
* **Id do contrato nativo.** Determinístico: `tagged_hash("contract-id", criador || nonce)`.
  Depois de enviar, veja com `thecoin-wallet tx <txid>` (campo `created`).
* **Depósito de armazenamento.** Todo contrato ocupa estado em todos os nós. Para
  isso, trava-se um **depósito reembolsável** de `storage_deposit_per_kb` por kB
  (mainnet: 0,001 TCN por kB; regtest: 0,0001). Nos nativos, o criador paga na
  criação (quase sempre 1 kB) e recebe de volta quando o contrato termina.
* **Contratos concluídos** (saldo zero) são removidos do estado para mantê-lo
  compacto, e o depósito volta ao criador. O histórico continua nas transações.
  Um multisig só é removido com `multisig-close`.
* **Taxas.** Toda criação ou chamada paga a taxa normal da transação, por
  quem envia (`base_fee + por kB`, e para TCCL também por 1 000 de combustível
  reservado). Ver [WALLET_DEVELOPERS.md §4.4](WALLET_DEVELOPERS.md#44-calcular-a-taxa).
* **Consultar.** Nativos: `thecoin-wallet contract show <id>` ou `GET /api/v1/contract/{id}`.
  TCCL: `thecoin-wallet contract program <endereço>` ou `GET /api/v1/program/{endereço}`.

Opções globais úteis: `--from N` (endereço da carteira), `-y` (sem confirmação),
`--dry-run` (só mostra a transação assinada), `--priority low|normal|high|urgent`,
`--replaceable`, `--network testnet`.

---

## 1. Escrow (garantia)

**Regras**

| Ação | Quem | Resultado |
|---|---|---|
| criar | comprador (pagador) | trava `amount` (+ depósito de armazenamento) |
| `escrow-release` | pagador **ou** árbitro | todo o saldo → vendedor; depósito → criador |
| `escrow-refund` | vendedor **ou** árbitro a qualquer momento; pagador só **após o prazo** | todo o saldo → pagador; depósito → criador |

Restrições: vendedor ≠ pagador; árbitro (opcional) ≠ vendedor e ≠ pagador; prazo no futuro.

**CLI**

```bash
# comprador trava 250 TCN por até 7 dias (40 320 blocos de 15 s), com árbitro
thecoin-wallet contract escrow-create \
  --payee tc1vendedor... --amount 250 --deadline-blocks 40320 --arbiter tc1arbitro...

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
# 12 000 TCN ao longo de 1 ano (2 102 400 blocos), cliff de 3 meses, revogável
thecoin-wallet contract vesting-create \
  --beneficiary tc1funcionario... --amount 12000 \
  --start-in 0 --cliff-blocks 518400 --duration-blocks 2102400 --revocable

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
# 30 TCN por mês (172 800 blocos de 15 s), 12 meses (trava 360 TCN)
thecoin-wallet contract subscription-create \
  --payee tc1saas... --amount 30 --period-blocks 172800 --periods 12

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
# 5 760 blocos de 15 s = 24 horas
thecoin-wallet contract htlc-create --recipient tc1bob... --amount 100 --timeout-blocks 5760
# com hash conhecido (ex.: vindo de outra blockchain), 12 horas
thecoin-wallet contract htlc-create --recipient tc1bob... --amount 100 \
  --hash-lock <sha256_hex> --timeout-blocks 2880

thecoin-wallet contract htlc-redeem <id> --preimage <segredo_hex>
thecoin-wallet contract htlc-refund <id>
```

**Cenário — atomic swap TCN ↔ BTC.** Alice tem TCN e quer BTC; Bob tem BTC.

1. Alice gera um segredo `s` e calcula `H = SHA-256(s)`.
2. Alice cria um HTLC na The Coin: 1 000 TCN para Bob, `hash-lock H`, prazo de **48 h** (11 520 blocos de 15 s).
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
* `multisig-close`: qualquer signatário encerra um cofre **vazio** (saldo zero e
  nenhum pagamento pendente); o registro é apagado e o depósito de
  armazenamento volta ao criador.
* Até 16 pagamentos pendentes por cofre. Sem `multisig-close`, o cofre nunca é removido.

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
thecoin-wallet contract multisig-close <id>              # cofre vazio: encerra e devolve o depósito
```

Resposta real de `GET /api/v1/contract/{id}` de um cofre 2-de-2 na regtest:
`"balance": 500000000, "deposit": 10000` (1 kB × 10 000 motes).

**Cenário — tesouraria de empresa ou associação.** Três sócios controlam o
caixa. Qualquer pagamento precisa de dois deles. Clientes pagam depositando
direto no cofre. Nenhum sócio sozinho consegue mover os fundos, e a perda de uma
chave não bloqueia o dinheiro.

---

## 6. Contratos inteligentes TCCL

TCCL é uma linguagem baseada em indentação, com tipos estáticos (`int`, `bool`,
`text`, `bytes`, `address`, `list[T]`, `map[K, V]` e, na versão 2, `record`,
`enum` e `interface`), sem ponto flutuante e sem
nada ambíguo: o compilador faz parte do consenso. Um contrato declara estado,
eventos, `init`, `action`s (chamadas por transação, podem mover TCN) e
`view`s (leitura gratuita):

```text
contract Counter

state count: int
state last_caller: address

event Increased(by: address, amount: int, total: int)

action increment(amount: int):
    require amount > 0, "amount must be positive"
    require amount <= 100, "at most 100 per call"
    count += amount
    last_caller = caller
    emit Increased(caller, amount, count)

view get() -> int:
    return count
```

A linguagem completa (tipos, funções embutidas, `payable`, `send`, `destroy`,
limites e tabela de combustível) está na
[referência da linguagem](https://github.com/LucasBolla94/tccl/blob/v0.3.0/docs/pt-BR/language.md);
receitas passo a passo em [docs/tccl/tccl-cookbook.pdf](tccl/tccl-cookbook.pdf).
Exemplos prontos em [`examples/`](https://github.com/LucasBolla94/tccl/tree/v0.3.0/examples)
do repositório da linguagem (`counter`, `token`, `crowdfund`, `tip_jar`,
`private_pool`, `shop`, `orders`, …); cópias locais da versão 1 em
[`docs/tccl/examples/`](tccl/examples).

**O que a versão 2 acrescenta.**
A rede compila hoje com `LANGUAGE_VERSION = 2` (`tccl::program`), e ainda executa
programas da versão 1 exatamente como na v0.2.0 — o `ProgramAdmin` de cada
contrato guarda a versão do código publicado (§6.7).

| Recurso | Para quê |
|---|---|
| **Records** | agrupar campos nomeados e tipados (`record Order: buyer: address …`) e guardá-los em `map`/`list` |
| **Enums com transições** | estados nomeados (`Placed`, `Paid`, …) com as mudanças permitidas conferidas pela VM a cada gravação |
| **Papéis e `only`** | `role minter` + `action mint(...) only minter` no lugar de `require caller == dono` espalhado |
| **Interfaces e chamadas entre contratos** | declarar as funções de que se precisa e chamar outro contrato pelo endereço |
| **Módulos e biblioteca padrão** | `use std.token`, `std.items`, `std.payments` — código revisado e congelado, compilado dentro do seu contrato |
| **Upgrades** | trocar o código de um contrato pela autoridade de upgrade, ou torná-lo final (§6.7) |
| **Aritmética e consultas novas** | `mul_div`, `isqrt`, `pow`, `code_hash(addr)`, `is_contract(addr)`, `is_final(addr)` |

**Chamadas entre contratos** são atômicas e acontecem dentro de uma única
transação:

```text
contract Payer

interface Token:
    action transfer(to: address, amount: int) -> bool
    view balance_of(who: address) -> int

action pay(token: address, to: address, amount: int):
    require Token(token).transfer(to, amount), "transfer failed"

view held(token: address) -> int:
    return Token(token).balance_of(self)
```

* **Quem chama:** o contrato chamado vê o contrato que chamou como `caller`;
  `origin` continua sendo quem assinou a transação. Autorize sempre por
  `caller`, nunca por `origin`.
* **Reentrada é sempre recusada:** um contrato que já está rodando na transação
  não pode ser chamado de novo (erro `R018`), e isso não pode ser desligado.
* **Limites:** até **8 contratos** encadeados (`MAX_CONTRACT_DEPTH`) e **16
  chamadas** de função aninhadas (`MAX_CALL_DEPTH`); combustível e memória
  (16 MiB, `MAX_MEMORY_BYTES`) são orçamentos compartilhados por todos.
* **Atomicidade:** qualquer falha em qualquer contrato aborta a transação
  inteira e o nó descarta tudo o que todos escreveram — só a taxa é cobrada.

### 6.1 Desenvolver localmente com `tccl`

O instalador e os pacotes de release incluem a ferramenta `tccl`:

```bash
tccl new meu-contrato                        # projeto novo com contrato e cenário de teste
tccl check counter.tccl                      # compila e mostra a interface
tccl abi counter.tccl                        # interface em JSON
tccl run counter.tccl deploy                 # simulador local (estado em tccl-state.json)
tccl run counter.tccl deploy --final         # publica sem autoridade: código imutável
tccl run counter.tccl call increment 5
tccl run counter.tccl --from bob call increment 500
tccl run counter.tccl view get
tccl run counter_v2.tccl upgrade             # troca o código e imprime o relatório de compatibilidade
tccl run counter.tccl authority none         # renuncia à autoridade (contrato final)
tccl run counter.tccl state                  # estado, versão do código e autoridade de upgrade
tccl test examples                           # roda os cenários .scenario
tccl ring keygen                             # par de chaves de anel (privacidade)
```

Saída real:

```
✔ Counter compiles (484 bytes of source, 323 bytes compiled)
  state: count: int, last_caller: address
  action increment(amount: int)
  view get() -> int
  view last() -> address
event Increased(by: tcr1zqygp3t5ugluq2uqf7gq60mc9z9t09lpkg42sa, amount: 5, total: 5)
ok · fuel used: 1674 · height: 3 · alice balance: 100000000000000 motes
FAILED: requirement failed: at most 100 per call · fuel used: 33 (all changes reverted)
result: 5
```

Opções de `tccl run`: `--state <arquivo>`, `--from <nome>` (contas de teste
`alice`, `bob`… com 1 000 000 TCN; endereços como `@bob`), `--value <valor>`
(`5tcn` ou motes), `--height <n>`, `--contract <hex>`, `--final` (no `deploy`).

### 6.2 Implantar e usar na rede

```bash
# implantar (o combustível é medido por simulação no nó, +30 %)
thecoin-wallet contract deploy counter.tccl
#   Fuel:     ... measured, limit ...
#   Contract address: tc1...

thecoin-wallet contract invoke <endereço> increment 5              # ação (transação)
thecoin-wallet contract invoke <endereço> pledge --value 2.5       # ação payable com TCN
thecoin-wallet contract view <endereço> get                        # view (grátis, sem transação)
thecoin-wallet contract program <endereço>                         # saldo, armazenamento, depósito, versão e funções
thecoin-wallet contract upgrade <endereço> counter_v2.tccl         # troca o código (§6.7)
thecoin-wallet contract authority <endereço> none                  # torna o contrato final (§6.7)
```

Opções de `deploy`/`invoke`: `--value <TCN>` (enviado ao contrato),
`--max-fuel <n>` (sem ela, a carteira simula e usa `consumo × 1,3 + 5 000`),
`--max-deposit <TCN>` (máximo de depósito aceito; padrão 1 TCN). Argumentos são
convertidos pelos tipos declarados: `42`, `2.5tcn`, `true`, `"texto"`, `0xabcd`,
`tc1…`, `[1, 2]`.

`contract program` mostra, além do saldo e do armazenamento, a linha
`Language:  TCCL version <n> · code version <n>` e se o contrato aceita upgrades
(`Upgrades:  allowed by <endereço>`) ou é final
(`Upgrades:  final — this code can never change`).

Exemplo real na regtest (token `token.tccl`, medido com a versão 1 da linguagem):

```
$ thecoin-wallet contract deploy token.tccl
Fuel:     9500 measured, limit 17350
Action:   deploy contract SimpleToken (1701 bytes of TCCL)
Fee:      0.00004619 TCN (1860 bytes, priority Normal)
Contract address: tcr1nlak75fjnu8ez5g4c54yprf22ez02l7rn2ahqh

$ thecoin-wallet contract invoke tcr1nlak75fjnu8ez5g4c54yprf22ez02l7rn2ahqh transfer tcr1fexc… 2500
Fuel:     2225 measured, limit 7892
Preview:  Transfer(from: tcr1fgy8…, to: tcr1fexc…, amount: 2500) (simulated on the current state)
Fee:      0.00001394 TCN (225 bytes, priority Normal)
```

### 6.3 O que acontece numa chamada

* **Endereço:** `program_address(criador, nonce do deploy)`; o código-fonte fica
  na transação e todo nó compila igual. O saldo do contrato é uma conta comum.
* **Combustível:** compilar custa 5 por byte de fonte, carregar custa
  `100 + tamanho/100`, e cada operação tem custo calibrado em ≈ 20 ns de CPU por
  unidade (leitura de armazenamento 250, escrita 400 + 4/byte, `send` 300,
  `ring_verify` 5 000 + 10 000 por membro…). A taxa é cobrada sobre o `max_fuel`
  **reservado** (`fee_per_kfuel` por 1 000); por isso a carteira simula antes.
  A versão 2 acrescenta preços para o que ela acrescentou: chamar outro contrato
  custa `700 + 1 a cada 32 bytes de argumentos` mais a carga do contrato chamado,
  `code_hash`/`is_contract`/`is_final` custam 250, `mul_div`/`isqrt`/`pow` custam
  30, e conferir uma transição de enum custa 1 por nível.
* **Falha = reversão com taxa.** Se o código falhar (`require`, falta de
  combustível, depósito acima do `max_deposit`, reentrada, profundidade…), tudo é
  revertido — valor enviado, escritas, pagamentos e eventos, **em todos os
  contratos que a transação tocou** — mas a taxa é paga. Sem isso,
  qualquer um faria a rede executar código que falha de graça. Vale para `Deploy`,
  `Invoke` e `Upgrade`; `SetUpgradeAuthority` não executa código e, se for
  recusado, a transação simplesmente não entra em bloco. A carteira e a API
  do nó recusam chamadas que já falhariam agora, então isso só acontece quando o
  estado muda entre o envio e o bloco (ou quando alguém envia direto pela rede P2P).
* **Recibo:** `GET /api/v1/tx/{txid}` mostra `success`, `error`, `fuel_used`,
  `logs` (eventos), `return_value` e, no deploy, `program`.

### 6.4 Depósito de armazenamento (reembolsável)

```
depósito necessário = ceil(state_bytes / 1000) × storage_deposit_per_kb
state_bytes         = código compilado + Σ (chave + valor) do armazenamento
```

* No **deploy** o criador trava o depósito do estado inicial (≈ tamanho compilado).
* Quem faz o contrato **crescer** paga a diferença, até o seu `max_deposit`.
* **Um `max_deposit` para a transação inteira.** Como uma transação pode tocar
  vários contratos (chamadas entre contratos), o acerto é feito no fim contra o
  **único** `max_deposit` da transação: cada contrato que cresceu consome parte
  desse orçamento e, se acabar, a transação falha com
  `storage deposit of N motes exceeds max_deposit …`.
* Quem **libera** armazenamento (apaga entradas, zera saldos em mapas) recebe de
  volta a parte proporcional do depósito, na mesma transação.
* `destroy(destino)` apaga o contrato e envia ao destino o saldo **e** todo o depósito.
  Variáveis simples são apagadas automaticamente; listas e mapas precisam ser esvaziados
  antes (senão a chamada falha), então nenhum dado fica para trás sem depósito.

O token do exemplo acima usa 1 376 bytes → 2 kB → depósito de 20 000 motes na
regtest (0,002 TCN na mainnet).

### 6.5 Por que continua barato se o preço da moeda subir

* **Tudo é cotado em motes e é parâmetro de governança.** `base_fee`,
  `fee_per_kb`, `fee_per_kfuel` e `storage_deposit_per_kb` podem ser **reduzidos
  por votação** (detentores + mineradores) se o TCN valorizar. Os limites
  mínimos permitem chegar a 1 mote por kB/por 1 000 de combustível (e 0 para
  `base_fee` e `storage_deposit_per_kb`). Ver [GOVERNANCE.md](GOVERNANCE.md).
* **O depósito não é gasto:** fica travado e volta quando o espaço é liberado ou
  o contrato termina. É custo de capital, não despesa.
* **Congestionamento não enriquece mineradores:** a sobretaxa acima de 1× é
  queimada, então não há incentivo para inflar taxas artificialmente, e o
  multiplicador volta a 1× assim que os blocos ficam abaixo de 50 %.
* **Números de hoje (mainnet, 1,0×):** transferência simples de 159 bytes =
  2 590 motes (0,0000259 TCN); chamada de token (225 bytes, 7 892 de combustível
  reservado) = 11 142 motes (≈ 0,00011 TCN); saque privado com anel de 3 (472 bytes,
  56 885 de combustível) = 62 605 motes (≈ 0,00063 TCN); implantar o token (1 860 bytes, 17 350 de
  combustível) ≈ 36 950 motes (≈ 0,00037 TCN) + 0,002 TCN de depósito reembolsável.

### 6.6 Privacidade por contratos

A The Coin é transparente por padrão; privacidade é **opcional** e feita por
contratos TCCL, usando a função embutida `ring_verify` (assinaturas em anel
**bLSAG** sobre Ristretto255). O exemplo `private_pool.tccl` do repositório da
linguagem implementa a interface padrão **TCCL-PRIV-1**:

1. **Depósito:** cada participante deposita exatamente a denominação do pool
   (10 TCN no exemplo) junto com uma chave pública de anel nova.
2. **Saque:** depois, qualquer pessoa com a chave secreta correspondente saca
   para **qualquer endereço**, assinando com um anel formado por vários depósitos.
   O contrato verifica que *um* deles assinou — nunca qual. A *key image* impede
   sacar o mesmo depósito duas vezes.
3. **Relayer:** outra conta pode enviar a transação de saque e receber uma taxa
   (`fee`), para que o destino não precise ter TCN antes.

```bash
thecoin-wallet privacy keygen --key 0                          # chave pública de anel #0
thecoin-wallet privacy deposit <pool> --key 0                  # deposita a denominação
thecoin-wallet privacy status <pool> --key 0                   # depositado? já sacado?
thecoin-wallet privacy withdraw <pool> --to tc1novo... --key 0 --ring-size 16 \
    --relayer tc1relayer... --fee 0.1                           # saque privado
```

As chaves de anel derivam da mesma frase de recuperação (`m/44'/7333'/0'/7'/índice'`).
Boas práticas: use um endereço de destino novo, envie o saque por um relayer ou
por um endereço sem ligação com o depósito e espere outros depósitos entrarem
no pool antes de sacar. Interface e algoritmo para carteiras:
[WALLET_DEVELOPERS.md §7](WALLET_DEVELOPERS.md#7-privacidade-pools-tccl-priv-1).

### 6.7 Atualizar um contrato, transferir a autoridade e torná-lo final

Código publicado às vezes precisa de correção. Cada contrato TCCL tem no máximo
**uma autoridade de upgrade**, guardada no registro de estado
`0x0A ProgramAdmin { authority, code_version, language, previous_code_hash }`.

> **A regra central: registro ausente = contrato final.** Quando não existe
> `ProgramAdmin` para um endereço, o código daquele contrato nunca mais muda.

| Situação | O que acontece |
|---|---|
| `Deploy` normal | quem publica vira a autoridade; `code_version = 1`, `previous_code_hash` zerado |
| `Deploy` com a flag `FLAG_FINAL_DEPLOY` (= 2) | nenhum `ProgramAdmin` é gravado: o contrato já nasce **final** |
| `Upgrade` | a autoridade troca o código; `code_version += 1` e `previous_code_hash` guarda o hash do código anterior |
| `SetUpgradeAuthority` com um endereço | a autoridade passa a ser outro endereço (por exemplo um multisig ou um contrato de governança) |
| `SetUpgradeAuthority` com `None` | o registro é apagado: o contrato fica **final para sempre** |

**Duas transações novas** (ações do protocolo, [PROTOCOL.md §5](PROTOCOL.md#5-transações)):

```text
Upgrade { contract, source, expected_code_hash, args, max_fuel, max_deposit }
SetUpgradeAuthority { contract, new_authority, expected_code_hash }
```

`expected_code_hash` é o hash do código que está publicado **agora**
(`tagged_hash("program-code", código compilado)`, o campo `code_hash` de
`GET /api/v1/program/{addr}`). Se alguém tiver atualizado o contrato entre o
preparo e o bloco, a transação falha em vez de sobrescrever a mudança de outro.

**O que o nó confere num `Upgrade`**, na ordem:

1. existe `ProgramAdmin` (senão: *"this contract is final: its code can never change"*);
2. quem assina é a autoridade;
3. `expected_code_hash` bate com o código publicado;
4. o novo código compila **na mesma ordem de variáveis de estado** do antigo;
5. a compatibilidade de armazenamento passa (variáveis de estado mantidas com
   nome e tipo compatíveis, campos de records iguais, variantes de enum só
   acrescentadas no fim, versão da linguagem nunca voltando atrás);
6. o código compilado cabe em `MAX_PROGRAM_BYTES` (262 144 bytes).

Depois disso, se o contrato tiver uma função `upgrade()`, ela roda **uma vez**
dentro da mesma transação (é onde se migram dados); se falhar, o upgrade inteiro
é revertido e o código antigo continua. Sem `upgrade()`, o upgrade só é aceito
sem argumentos. O depósito de armazenamento é acertado como em qualquer chamada
(§6.4): código maior consome parte do `max_deposit`, código menor devolve.

**Na carteira:**

```bash
# trocar o código (pede confirmação; --yes-upgrade quando não há como perguntar)
thecoin-wallet contract upgrade <endereço> counter_v2.tccl [args…] [--yes-upgrade]

# passar a autoridade adiante
thecoin-wallet contract authority <endereço> tc1multisig...

# renunciar: o contrato fica final para sempre (pede confirmação)
thecoin-wallet contract authority <endereço> none

# conferir antes de confiar
thecoin-wallet contract program <endereço>
```

Antes de enviar, `contract upgrade` busca o contrato na API, recusa contratos
finais, compila o novo código, confere os argumentos contra a `upgrade()`
declarada, lê o `code_hash` atual e mostra `Contract`, `Authority` e
`Version: n → n+1`.

A carteira de referência publica contratos **atualizáveis**: ela não tem opção
para a flag `FLAG_FINAL_DEPLOY` (só `--replaceable` chega a `flags`), então o
jeito de tornar um contrato imutável com ela é publicar e depois
`contract authority <endereço> none`.

**Para quem usa um contrato:** trate um contrato atualizável como confiança na
autoridade dele — ela pode trocar as regras, inclusive as que guardam fundos.
`contract program` e `GET /api/v1/program/{addr}` dizem sempre quem é a
autoridade e a versão da linguagem do código publicado. Num contrato **final** o
registro não existe mais, então a API responde `upgrade_authority: null` e
`code_version: 1` mesmo que o contrato tenha sido atualizado antes de virar
final — para saber o histórico, olhe as transações do endereço. Dentro de um contrato,
`is_final(addr)` e `code_hash(addr)` permitem exigir dependências imutáveis
(`require is_final(token), "token must be final"`).

---

## Integração para desenvolvedores

* Ações e campos binários: [PROTOCOL.md §5](PROTOCOL.md#5-transações).
* Construção em Rust: `thecoin_wallet::builder::{create_contract, call_contract, deploy, invoke,
  upgrade, set_upgrade_authority}` com `thecoin_core::contracts::{ContractSpec, ContractCall}` e
  `thecoin_core::tccl::Value`.
* Estado legível (incluindo `vested_now`, `claimable_periods_now` e `deposit`): [API.md](API.md#get-apiv1contractid).
* Contratos TCCL: [`GET /api/v1/program/{addr}`](API.md#get-apiv1programaddr) — que agora traz
  `code_hash`, `upgrade_authority` (nulo = contrato final), `code_version` e `language` —,
  [`POST /api/v1/program/{addr}/view`](API.md#post-apiv1programaddrview) e
  [`POST /api/v1/tx/simulate`](API.md#post-apiv1txsimulate).
* Histórico: `GET /api/v1/address/{addr}/txs` inclui criações e chamadas de contratos em que o endereço é parte.
