# Contribuindo com a The Coin

Obrigado por ajudar a construir a The Coin! Este guia explica como preparar o
ambiente, rodar os testes, subir uma rede local e enviar mudanças.

Leitura recomendada antes de mexer no código:
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) e
[`docs/PROTOCOL.md`](docs/PROTOCOL.md).

## 1. Ambiente de desenvolvimento

```bash
# Linux (Debian/Ubuntu)
sudo apt install -y build-essential pkg-config git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup component add rustfmt clippy

git clone https://github.com/the-coin-cloud/thecoin
cd thecoin
cargo build
```

* Rust estável (MSRV declarado em `Cargo.toml`: 1.80).
* Em máquinas com pouca RAM: ative swap e use `CARGO_BUILD_JOBS=1`.
* O perfil `dev` compila dependências com otimização (`opt-level = 2`) para que
  o CoinHash e os testes rodem rápido.

## 2. Qualidade: formatação, lint e testes

Tudo isso roda no CI e precisa passar antes do merge:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bash -n installer/install.sh && bash -n installer/uninstall.sh
```

Benchmarks (ignorados por padrão):

```bash
cargo test -p thecoin-core --release -- --ignored bench_pow --nocapture
cargo test -p thecoin-core --release --test bench -- --ignored --nocapture
```

Toda mudança em regra de consenso deve vir com teste em
`crates/core/tests/execution.rs` (que verifica o **invariante de supply** e o
LtHash a cada bloco). Mudanças de rede devem ser cobertas em
`crates/node/tests/network.rs`.

## 3. Rede local com 2 nós (regtest)

Na regtest a PoW é trivial, a maturidade é de 5 blocos, o halving ocorre a cada
150 blocos e votações duram 20 blocos — ideal para testar tudo em minutos.

```bash
cargo build
export THECOIN_NETWORK=regtest THECOIN_WALLET_PASSWORD=dev THECOIN_WALLET_FAST_KDF=1
W=./target/debug/thecoin-wallet
D=/tmp/thecoin-dev && mkdir -p $D

# carteiras
$W -w $D/alice.json create --words 12
$W -w $D/bob.json create --words 12
ALICE=$($W -w $D/alice.json address); BOB=$($W -w $D/bob.json address)

# nó 1: minera para Alice
./target/debug/thecoind --network regtest --data-dir $D/n1 \
  --listen 127.0.0.1:27333 --rpc 127.0.0.1:27334 --miner-address $ALICE --threads 1 &

# nó 2: não minera, conecta só ao nó 1
./target/debug/thecoind --network regtest --data-dir $D/n2 \
  --listen 127.0.0.1:27433 --rpc 127.0.0.1:27434 --no-mine \
  --peer 127.0.0.1:27333 --connect-only &

sleep 10
$W -w $D/alice.json balance
# envia pelo nó 2 — a transação é propagada ao nó 1, minerada e o bloco volta ao nó 2
$W -w $D/alice.json --node http://127.0.0.1:27434 -y send $BOB 10 --memo "teste"
sleep 5
$W -w $D/bob.json --node http://127.0.0.1:27434 balance
curl -s http://127.0.0.1:27434/api/v1/peers

kill %1 %2
```

`THECOIN_WALLET_FAST_KDF=1` reduz o custo do Argon2 da carteira — **use apenas em testes**.

## 4. Commits

Formato (inspirado em Conventional Commits), em inglês ou português, no imperativo:

```
<escopo>: <resumo curto>

<corpo opcional explicando o porquê>
```

Escopos: `core`, `consensus`, `storage`, `node`, `p2p`, `rpc`, `miner`,
`mempool`, `wallet`, `docs`, `website`, `installer`, `ci`.

Exemplos:

```
wallet: add --expiry flag to send
consensus: add Escrow partial release (activation at height N)
docs: document address history pagination
```

* Um assunto lógico por commit; o código deve compilar e passar nos testes em cada commit.
* Mudanças de consenso usam o escopo `consensus` e citam a seção de `docs/PROTOCOL.md` alterada.

## 5. Branches e Pull Requests

1. Crie um branch a partir de `main`: `feature/<nome>`, `fix/<nome>`, `docs/<nome>`.
2. Mantenha o PR pequeno e focado; descreva **o quê** e **por quê**, como testou e riscos.
3. Atualize a documentação (`docs/`) no mesmo PR quando mudar comportamento, API ou formatos.
4. CI verde é obrigatório. PRs de consenso precisam de **2 revisões** de mantenedores.
5. Merge por *squash* ou *rebase* (histórico linear em `main`).
6. Releases são tags `vX.Y.Z` que disparam `.github/workflows/release.yml`.

## 6. Política para mudanças de consenso

Mudanças que alteram quais blocos/transações são válidos (formatos Borsh,
hashes, PoW, dificuldade, emissão, regras de contratos/governança, parâmetros
fixos) são **hard forks** e seguem regras mais rígidas:

* **Proibido:** alterar o supply máximo de 50 milhões ou a curva de emissão.
* Enums serializados (`TxAction`, `ContractSpec`, `ContractCall`,
  `ContractState`, `ProposalAction`, `GovParamId`, `VoteChoice`, `Message`)
  são **append-only**: nunca reordenar, remover ou inserir no meio.
* Toda regra nova entra atrás de uma **altura de ativação** por rede; blocos
  antigos continuam válidos com as regras antigas.
* Especificação atualizada em `docs/PROTOCOL.md` e vetores de teste quando
  formatos mudarem.
* Passagem pela testnet antes da mainnet.
* Ativação coordenada por proposta de governança `SoftwareUpgrade`
  (ver [`docs/GOVERNANCE.md`](docs/GOVERNANCE.md)), com o hash do release.

Mudanças de política local (mempool, P2P compatível, API, carteira, site)
não exigem hard fork, mas devem manter compatibilidade com nós da versão anterior.

## 7. Segurança

**Não abra issues públicas para vulnerabilidades.** Envie um e-mail para
**security@the-coin.cloud** com:

* descrição e impacto;
* passos para reproduzir ou prova de conceito;
* versão/commit afetados.

Compromisso: confirmação em até 72 h, avaliação e plano de correção, e
divulgação coordenada após a correção estar disponível. Crédito público a
quem reportar, se desejar.

Nunca inclua chaves privadas, frases de recuperação ou dados de usuários em
issues, logs ou commits.

## 8. Licença

Ao contribuir, você concorda que sua contribuição é licenciada sob
**MIT OR Apache-2.0**, como o restante do projeto (`LICENSE-MIT`, `LICENSE-APACHE`).
