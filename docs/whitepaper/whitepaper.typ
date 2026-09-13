// The Coin — Whitepaper v0.2
// Compile: typst compile docs/whitepaper/whitepaper.typ docs/whitepaper/the-coin-whitepaper-v0.2.pdf
// The previous version is reproducible with whitepaper-v0.1.typ (+ performance-v0.1.typ).

#import "common.typ": *

#let version = "0.2"

#set document(title: "The Coin — Whitepaper v0.2", author: "The Coin developers", date: datetime(year: 2026, month: 9, day: 13))
#set page(
  paper: "a4",
  margin: (x: 2.2cm, top: 2.4cm, bottom: 2.2cm),
  numbering: "1",
  header: context {
    if counter(page).get().first() > 1 [
      #set text(8.5pt, fill: muted)
      The Coin — Whitepaper v#version #h(1fr) the-coin.cloud
      #line(length: 100%, stroke: 0.4pt + luma(200))
    ]
  },
)
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "pt", region: "br")
#set par(justify: true, leading: 0.62em)
#set heading(numbering: "1.1")
#show heading.where(level: 1): it => {
  v(1.2em)
  text(17pt, fill: accent, weight: "bold", it)
  v(0.4em)
}
#show heading.where(level: 2): it => { v(0.5em); text(12.5pt, fill: accent, it); v(0.2em) }
#show heading.where(level: 3): it => { v(0.3em); text(11pt, it) }
#show raw.where(block: true): it => block(fill: luma(246), inset: 8pt, radius: 3pt, width: 100%, text(0.838em, it))
#show raw.where(block: false): it => box(fill: luma(242), inset: (x: 2pt), outset: (y: 2pt), radius: 2pt, text(9pt, it))
#show link: it => text(fill: accent, it)
#set table(stroke: 0.5pt + luma(190), inset: 5pt, align: left)
#show table.cell.where(y: 0): set text(weight: "bold")
#show table: set par(justify: false)
#show figure.where(kind: table): set figure.caption(position: bottom)
#show figure.where(kind: table): set block(breakable: true)

// ------------------------------------------------------------------ Capa ----
#page(numbering: none, header: none)[
  #v(3.2cm)
  #align(center)[
    #text(40pt, weight: "bold", fill: accent)[The Coin]
    #v(0.2cm)
    #text(15pt)[Uma moeda digital de prova de trabalho, leve e democrática,\ com contratos inteligentes, privacidade opcional e governança on-chain]
    #v(1cm)
    #text(13pt, fill: muted)[Whitepaper — versão #version]
    #v(0.3cm)
    #text(11pt, fill: muted)[13 de setembro de 2026 · #link("https://the-coin.cloud")[the-coin.cloud]]
    #v(0.15cm)
    #text(9pt, fill: muted)[substitui a versão 0.1 (setembro de 2026), que permanece publicada]
    #v(2cm)
    #block(width: 84%, inset: 14pt, stroke: 0.6pt + luma(200), radius: 4pt)[
      #set align(left)
      #set text(10pt)
      *Resumo.* The Coin (TCN) é uma rede monetária aberta e ponto a ponto, projetada para que
      máquinas modestas — um VPS de 1 vCPU e 1 GB de RAM — possam validar a cadeia completa e
      competir de forma justa pelas recompensas de mineração. A prova de trabalho *CoinHash* usa
      Argon2id com 16 MiB de memória, e a dificuldade é recalculada a cada bloco (LWMA-1). A oferta é
      limitada a 50 milhões de TCN, sem pré-mineração, com 93,75% emitidos em ≈ 4,75 anos.
      A versão 0.2 introduz um *modelo de taxas* com base fixa, custo por byte e por combustível e um
      multiplicador de congestionamento cuja sobretaxa é *queimada*; *níveis de prioridade*;
      *confirmação rápida e segura* (substituição por taxa opcional, alertas de gasto duplo, blocos
      compactos, estimador de confirmações e recompensas com liberação escalonada); contratos
      inteligentes na linguagem *TCCL*, com execução determinística medida por combustível e depósito
      de armazenamento reembolsável; *privacidade opcional* por assinaturas em anel dentro de
      contratos; armazenamento mais compacto; e instalação de validador em uma linha. Apresenta ainda
      o plano aprovado de evolução do consenso de PoW para um modelo híbrido e, depois, para prova de
      participação — um plano, não uma funcionalidade desta versão.
    ]
  ]
]

#page(numbering: none, header: none)[
  #outline(title: [Sumário], indent: auto, depth: 2)
]
#counter(page).update(1)

= Introdução

O Bitcoin demonstrou que é possível manter um livro-razão público, resistente à censura e à
falsificação, sem autoridade central. Quinze anos depois, porém, a mineração tornou-se uma
indústria de ASICs especializados e energia barata, e rodar um nó completo exige recursos que
afastam o usuário comum. Redes com contratos inteligentes, por sua vez, frequentemente trocaram a
descentralização por desempenho, exigindo servidores potentes para validar a cadeia.

The Coin parte de uma pergunta simples: *e se a unidade mínima de participação na rede fosse um
VPS barato?* Milhões de servidores virtuais de baixo custo existem no mundo inteiro. Se cada um deles
puder validar blocos, retransmitir transações e minerar com chances proporcionais ao seu trabalho,
a rede se torna mais distribuída geograficamente, mais difícil de capturar e mais democrática.

Os objetivos do projeto são:

- *Leveza:* validar a cadeia completa com 1 vCPU, 1 GB de RAM e poucos GB de disco.
- *Segurança:* regras de consenso simples, determinísticas e auditáveis; nenhuma transação
  confirmada pode ser alterada sem refazer o trabalho acumulado.
- *Mineração justa:* prova de trabalho *memory-hard* amigável a CPUs comuns e dificuldade que reage
  rápido à entrada e saída de máquinas.
- *Política monetária previsível:* 50 milhões de TCN, sem pré-mineração, emissão por halvings.
- *Taxas baratas e resistentes a abuso:* custo proporcional aos recursos usados, que sobe sozinho
  quando a rede é inundada e volta a cair quando a demanda passa.
- *Utilidade:* pagamentos, contratos de pagamento, contratos inteligentes gerais e privacidade
  opcional — sem abrir mão de validação barata.
- *Evolução democrática:* mudanças de parâmetros aprovadas on-chain por detentores *e* mineradores.
- *Abertura:* especificação, API e padrões de carteira documentados para que qualquer pessoa crie
  carteiras, exploradores, contratos e implementações alternativas.

#note[
  *Status.* Este documento descreve a implementação de referência *v0.2.0* em Rust (repositório
  #link("https://github.com/LucasBolla94/thecoin")[`LucasBolla94/thecoin`]). Todos os números de
  protocolo citados foram conferidos no código-fonte (`crates/core/src/params.rs`,
  `execution.rs`, `programs.rs` e demais). Onde houver divergência, o código e a especificação
  `docs/PROTOCOL.md` são normativos. A @sec-roadmap descreve um *plano* de consenso que não está
  implementado nesta versão.
]

= O que mudou na versão 0.2 <sec-changelog>

A tabela a seguir resume as mudanças em relação ao whitepaper v0.1. Cada item é detalhado nas seções
indicadas; o restante do documento mantém o texto da v0.1 onde ele continua válido.

#figure(
  table(
    columns: (auto, 1fr, 1.35fr),
    table.header([Área], [v0.1], [v0.2]),
    [Taxas (@sec-fees)], [`min_fee_per_byte × tamanho`; 100% ao minerador.], [`(base + por kB + por combustível) × congestionamento`; a sobretaxa de congestionamento é queimada; o excedente (gorjeta) vai ao minerador.],
    [Prioridade (@sec-priority)], [Sugestão pela mediana do mempool.], [Níveis `low`, `normal`, `high`, `urgent`; ordenação por taxa por unidade de peso.],
    [Substituição por taxa (@sec-confirm)], [Qualquer transação pendente, com +25%.], [Somente com a flag `replaceable` (opt-in) e +25%; tentativa sem a flag vira *alerta de gasto duplo* propagado na rede.],
    [Formato de transação], [Sem campo de opções.], [Campo `flags` (u8); ações `Deploy` e `Invoke` para TCCL. Transferência simples: 158 → 159 bytes.],
    [Recompensa de bloco], [Maturação única de 100 blocos.], [25% liberados após 100 blocos, 75% após 1 000 blocos (além do limite de reorganização de 720).],
    [Dificuldade (@sec-difficulty)], [LWMA-1 só após 61 blocos.], [Aquecimento a partir de 6 intervalos; variação máxima de 2× por bloco.],
    [Contratos (@sec-tccl)], [Cinco modelos nativos de pagamento.], [\+ linguagem TCCL: compilador no consenso, VM determinística com combustível, eventos e recibos; depósito de armazenamento reembolsável (também nos modelos nativos; `MultisigClose`).],
    [Privacidade (@sec-privacy)], [—], [Assinaturas em anel bLSAG (Ristretto255) acessíveis a contratos; pool de valor fixo; comandos `privacy` na carteira.],
    [Rede P2P], [Anúncio `Inv` e download do bloco completo.], [Blocos compactos com IDs curtos de 6 bytes; mensagens `DoubleSpend`.],
    [Segurança de pagamentos], [Recomendação fixa de 10/60 confirmações.], [Estimador por valor (`/api/v1/security`, `thecoin-wallet confirmations`); alertas (`/api/v1/alerts`).],
    [Armazenamento (@sec-storage)], [Esquema 1; índice de endereços guardava o txid.], [Esquema 2; tabela de recibos; índice de endereços sem valor; poda remove índice de transações e recibos; mempool persistente; `thecoind compact`.],
    [Governança (@sec-gov)], [8 parâmetros.], [12 parâmetros (taxas, combustível, depósito); sinalização por `thecoind --signal` e `thecoin signal`.],
    [Instalação (@sec-install)], [Instalador interativo.], [Uma linha, não interativa (`--yes`); comando auxiliar `thecoin`; ferramenta `tccl` instalada.],
    [API], [Consultas e envio.], [\+ `/tx/simulate`, `/program/{addr}`, `/program/{addr}/view`, `/security`, `/alerts`; `/fees` com níveis de prioridade.],
    [Roteiro (@sec-roadmap)], [Noise, snapshot, VM WebAssembly.], [Plano aprovado PoW → Híbrido → PoS; TCCL substitui a VM WebAssembly prevista.],
  ),
  caption: [Resumo das mudanças v0.1 → v0.2.],
)

= Visão geral do sistema

A rede é formada por nós idênticos (`thecoind`). Cada nó mantém a cadeia de blocos, valida cada
regra de consenso de forma independente, retransmite blocos e transações pela rede ponto a ponto e,
opcionalmente, minera. Neste documento, *validador* é um nó completo que valida tudo e minera — é o
que o instalador de uma linha configura. Carteiras (`thecoin-wallet` ou de terceiros) guardam as
chaves privadas localmente e só enviam transações já assinadas a um nó.

#figure(
  table(
    columns: (auto, 1fr),
    table.header([Componente], [Responsabilidade]),
    [`thecoin-core`], [Regras de consenso puras: tipos, codificação canônica, criptografia, CoinHash, LWMA, emissão, taxas e congestionamento, contratos nativos, execução de contratos TCCL, governança e função de transição de estado. Sem I/O.],
    [`tccl`], [The Coin Cloud Language: analisador, verificador de tipos e compilador, máquina virtual determinística com combustível, assinaturas em anel, simulador e a ferramenta de linha de comando `tccl`.],
    [`thecoin-storage`], [Banco de dados embarcado redb (ACID), blocos, undo e recibos comprimidos com zstd, índices de transações e endereços.],
    [`thecoin-node`], [Gerenciador da cadeia (validação, escolha do fork, reorganizações, poda), mempool persistente com prioridade e alertas de gasto duplo, rede P2P com blocos compactos, minerador de CPU e API REST.],
    [`thecoin-wallet`], [Chaves HD (BIP-39/SLIP-0010), arquivo de carteira criptografado, construção e assinatura de transações, prioridade de taxa, contratos, privacidade, governança, URIs de pagamento, CLI.],
    [Instalador e `thecoin`], [Script de uma linha que instala binários verificados, cria carteira, configura serviço systemd endurecido e inicia a mineração; comando auxiliar para operar o nó.],
    [Site / explorador], [the-coin.cloud: estatísticas ao vivo, explorador de blocos, painel de governança, guia do validador e downloads, alimentado pela API dos nós.],
  ),
  caption: [Componentes da implementação de referência.],
)

O fluxo básico é o mesmo do Bitcoin: usuários assinam transações; nós as validam e as guardam no
*mempool*; mineradores agrupam as transações que pagam mais por recurso usado e procuram um *nonce*
que satisfaça a prova de trabalho; o bloco encontrado é propagado; cada nó o valida integralmente e,
se ele estender a cadeia com mais trabalho acumulado, o adota.

= Modelo de dados

== Unidades

A unidade de conta é o *TCN*. A menor fração é o *mote*: 1 TCN = 100 000 000 motes.
Todos os valores são inteiros de 64 bits sem sinal; ponto flutuante nunca é usado no consenso.
A oferta máxima (5 × 10#super[15] motes) cabe com folga em 64 bits e também é um inteiro seguro
em JavaScript, o que simplifica carteiras web.

== Codificação canônica e hashes

Todas as estruturas de consenso são serializadas com *Borsh*: inteiros little-endian de tamanho
fixo, vetores e strings prefixados por comprimento `u32`, enums com tag `u8`. Cada objeto tem
exatamente uma codificação válida, o que impede ambiguidades na assinatura e no hash.

Todo hash do protocolo usa BLAKE3 com separação de domínio:
$ H_"tag"(x) = "BLAKE3"("\"TheCoin:\"" || "tag" || 0"x"00 || x) $
Tags distintas (`txid`, `block`, `tx-sign`, `merkle-leaf`, `address`, ...) garantem que um valor
calculado para um propósito jamais colida com outro.

== Contas e endereços

The Coin usa o *modelo de contas*: cada endereço possui saldo e *nonce* (número de transações já
enviadas). Comparado ao modelo UTXO, o estado é menor, a validação de contratos é direta e
carteiras leves precisam consultar um único registro.

O endereço é formado pelos 20 primeiros bytes de $H_"address"$(chave pública Ed25519) e
representado em *bech32m* (BIP-350), com prefixo por rede: `tc1…` (mainnet), `tct1…` (testnet) e
`tcr1…` (regtest). O checksum cobre o prefixo, então um endereço de outra rede ou com erro de
digitação é rejeitado. Contratos TCCL também têm endereço (derivado de criador e nonce) e saldo
próprio, como qualquer conta.

== Transações

#figure(
  table(
    columns: (auto, auto, 1fr),
    table.header([Campo], [Tipo], [Descrição]),
    [`version`], [u8], [Versão do formato (1).],
    [`chain_id`], [u32], [Identificador da rede — impede reutilizar uma transação em outra rede.],
    [`flags`], [u8], [Opções. Bit 0 = `replaceable` (aceita substituição por taxa). Bits desconhecidos invalidam a transação.],
    [`nonce`], [u64], [Deve ser igual ao nonce atual da conta; impede replay.],
    [`fee`], [u64], [Taxa em motes; deve cobrir a taxa mínima (@sec-fees). O excedente é gorjeta de prioridade.],
    [`expiry_height`], [u64], [Última altura em que pode ser incluída (0 = sem validade).],
    [`action`], [enum], [Transferência, lote, criar/chamar contrato nativo, propor, votar, publicar (`Deploy`) ou chamar (`Invoke`) contrato TCCL.],
    [`public_key`], [32 bytes], [Chave pública Ed25519 do remetente.],
    [`signature`], [64 bytes], [Assinatura Ed25519 sobre $H_"tx-sign"$(body).],
  ),
  caption: [Estrutura de uma transação.],
)

O identificador é txid = $H_"txid"$(transação serializada). A verificação Ed25519 é
*estrita* (RFC 8032, rejeitando chaves de ordem pequena e assinaturas não canônicas), eliminando a
maleabilidade: o txid só muda se o dono da chave assinar outra transação.

Uma propriedade útil para carteiras: como `fee` tem largura fixa, o tamanho da transação não
depende do valor da taxa. Basta assinar uma vez com taxa zero para descobrir o tamanho exato e então
assinar com a taxa final. Uma transferência simples sem memo ocupa *159 bytes*.

As transações nativas (transferências, lotes, contratos de pagamento e governança) são *atômicas*:
ou são totalmente válidas e aplicadas, ou não podem entrar em um bloco — não existe "falhou mas
cobrou taxa". A exceção deliberada são as transações de contratos TCCL (`Deploy`, `Invoke`): se o
código do contrato falhar, a taxa é cobrada e todos os demais efeitos são revertidos; do contrário,
qualquer um poderia fazer a rede executar código que falha sem pagar nada (@sec-tccl).

== Blocos

#figure(
  table(
    columns: (auto, auto, 1fr),
    table.header([Campo], [Tamanho], [Descrição]),
    [`version`], [4], [Versão do cabeçalho (1).],
    [`height`], [8], [Altura do bloco (gênese = 0).],
    [`prev_hash`], [32], [Hash do cabeçalho anterior.],
    [`tx_root`], [32], [Raiz Merkle dos txids.],
    [`state_root`], [32], [Compromisso do estado global *após* aplicar o bloco (LtHash).],
    [`timestamp`], [8], [Horário Unix em segundos.],
    [`target`], [32], [Alvo da prova de trabalho (inteiro de 256 bits big-endian).],
    [`nonce`], [8], [Campo livre para a busca da prova de trabalho.],
    [`miner`], [20], [Endereço que recebe recompensa e taxas.],
    [`signal`], [4], [Bits de sinalização de governança dos mineradores.],
  ),
  caption: [Cabeçalho de bloco: tamanho fixo de 180 bytes.],
)

O identificador do bloco é $H_"block"$(cabeçalho), barato de calcular. A prova de trabalho é um
hash separado e caro (CoinHash). Não existe transação *coinbase*: a recompensa é definida pelo campo
`miner` e registrada no estado. Um bloco tem dois limites, ambos parâmetros de governança: tamanho
serializado (`max_block_bytes`, 1 MB) e soma do combustível reservado pelas transações de contrato
(`max_block_fuel`, 50 milhões).

A árvore Merkle usa folhas e nós com tags distintas e promove o último elemento de níveis ímpares
sem duplicá-lo, eliminando a ambiguidade conhecida do Bitcoin (CVE-2012-2459).

= Consenso por prova de trabalho

== CoinHash

$ "CoinHash"("cabeçalho") = "Argon2id"("senha" = "cabeçalho", "sal" = "\"TheCoin/PoW/v1..\"", m = "16 MiB", t = 1, p = 1) $

O resultado de 32 bytes, lido como inteiro big-endian, deve ser menor ou igual ao `target` do
cabeçalho. Argon2id (RFC 9106 [2]) é o vencedor da Password Hashing Competition e é *memory-hard*: cada
tentativa exige preencher 16 MiB de memória em ordem dependente dos dados. As consequências são:

- *CPUs comuns são competitivas.* O gargalo é a largura de banda de memória, não o número de
  transistores de lógica; GPUs e ASICs têm vantagem muito menor do que em SHA-256.
- *Verificação barata para nós modestos.* Um hash custa milissegundos, e cada nó verifica apenas um
  hash por bloco.
- *Memória previsível.* Cada thread de mineração usa 16 MiB; um VPS de 1 GB minera sem pressão.

#figure(
  table(
    columns: (auto, auto, auto),
    table.header([Memória Argon2id], [Tempo por hash], [Hashes/s por núcleo]),
    [4 MiB], [2,41 ms], [415],
    [8 MiB], [5,22 ms], [192],
    [*16 MiB (escolhido)*], [*12,04 ms*], [*83*],
    [32 MiB], [25,16 ms], [40],
  ),
  caption: [Medições em um VPS OVH de 2 vCPU (Haswell). 16 MiB equilibra resistência a hardware especializado e custo de verificação.],
)

Com 83 hashes/s, verificar a prova de trabalho de um ano inteiro de blocos (525 960 blocos) custa
cerca de 1,75 hora de CPU num único núcleo; o nó verifica blocos recebidos em paralelo em todos os
núcleos durante a sincronização, mantendo a ordem de aplicação.

== Dificuldade justa (LWMA-1) <sec-difficulty>

Redes jovens de máquinas pequenas sofrem grandes variações de *hashrate* à medida que mineradores
entram e saem. The Coin recalcula o alvo a cada bloco com a média móvel linearmente ponderada
LWMA-1 (Zawy [7]), com janela $N = 60$ blocos e tempo-alvo $T = 60$ s. Para o bloco de altura $h$,
usa-se $n = min(N, h - 1)$ intervalos:

$ t_j = min(6T, max(s_j, s_(j-1) + 1) - s_(j-1)) $
$ "alvo"^* = (sum_(j=1)^n "alvo"_j) / n dot (sum_(j=1)^n j dot t_j) / k, quad k = n(n+1)T/2 $
$ "alvo"_h = min("pow_limit", max("alvo"_(h-1) / 2, min(2 dot "alvo"_(h-1), "alvo"^*))) $

Três regras tornam o ajuste *justo*:

+ *Reação rápida.* Blocos recentes pesam mais: se uma máquina grande entra ou sai, a dificuldade
  acompanha em minutos. Ninguém ganha uma rajada de blocos baratos ao ligar muito *hashrate* de uma
  vez, e os pequenos mineradores não ficam presos a uma dificuldade alta depois que ele sai.
+ *Aquecimento.* Na v0.1 os primeiros 61 blocos usavam o alvo da gênese. Na v0.2 o ajuste começa
  assim que existem *6 intervalos* (`LWMA_MIN_WINDOW`), usando todos os intervalos disponíveis até
  completar a janela — uma rede recém-lançada reage após poucos blocos.
+ *Limite de 2× por bloco.* O novo alvo nunca é mais que 2× mais fácil ou mais difícil que o do bloco
  anterior. Isso suaviza a janela curta do aquecimento e limita o efeito de timestamps manipulados,
  que já são contidos pela limitação de $t_j$ a $[1, 6T]$ e pela regra da mediana.

O cálculo usa aritmética inteira de 512 bits e o alvo é limitado pelo *pow limit* da rede.

#note[
  *Justiça × eficiência.* Um ASIC de SHA-256 é muito mais eficiente em energia por hash do que uma
  CPU — mas essa eficiência concentra a mineração em quem tem acesso a fábricas de chips e energia
  subsidiada. The Coin escolhe conscientemente a *justiça*: uma função cujo custo é dominado por
  memória, de modo que o hardware que já existe em todo lugar (CPUs de servidores e VPS) continue
  competitivo, e um ajuste de dificuldade que não premia quem entra e sai em rajadas. O preço pago é
  modesto e medido: ≈ 12 ms de verificação por bloco e 16 MiB por thread de mineração.
]

== Regras de tempo

- O timestamp deve ser maior que a mediana dos 11 blocos anteriores (*median time past*).
- O timestamp não pode estar mais de 180 s à frente do relógio local do nó.

== Escolha da cadeia e finalidade

Cada nó segue a cadeia válida com *maior trabalho acumulado*
($sum 2^256 slash ("alvo" + 1)$); em empate, a primeira vista. Uma reorganização desconecta
blocos usando dados de *undo* e conecta os blocos do novo ramo, tudo em uma única transação atômica
do banco de dados — se qualquer bloco do novo ramo for inválido, nada muda.

Como proteção adicional de uma rede em crescimento, reorganizações com mais de *720 blocos*
(≈ 12 horas) são recusadas, e checkpoints podem ser fixados no código a partir de alturas já
consolidadas (a lista está vazia no lançamento). Isso limita o dano de um ataque de 51% de curta
duração a janelas recentes. A finalidade na v0.2 é probabilística; a finalidade econômica por
votação de validadores faz parte do plano da @sec-roadmap.

= Política monetária

== Emissão

A emissão segue o modelo do Bitcoin, comprimido para o horizonte de estabilização desejado:

$ "subsídio"(h) = cases(0 & "se" h = 0, "40 TCN" >> floor((h - 1) \/ "625 000") & "caso contrário") $

Com blocos de 60 s são produzidos ≈ 525 960 blocos por ano, e um halving ocorre a cada
≈ 434 dias (1,19 ano). A soma geométrica limita a oferta:

$ sum "subsídios" < 40 times "625 000" times 2 = "50 000 000 TCN" $

Por causa do truncamento inteiro dos halvings, o total emitido é 49 999 999,91875 TCN (valor
impresso por `thecoind params`) — o limite nunca é ultrapassado, e cada nó rejeita um bloco que
faria a emissão acumulada exceder 50 milhões.

#let eras = range(0, 8)
#figure(
  table(
    columns: (auto, auto, auto, auto, auto, auto),
    align: (center, right, right, right, right, right),
    table.header([Era], [Recompensa], [Emitido na era], [Acumulado], [% da oferta], [Fim (anos)]),
    ..eras.map(e => {
      let r = 40 / calc.pow(2, e)
      let emitted = r * 625000
      let cum = 50000000 * (1 - 1 / calc.pow(2, e + 1))
      let pct = 100 * (1 - 1 / calc.pow(2, e + 1))
      let years = (e + 1) * 625000 * 60 / 31557600
      (
        [#(e + 1)],
        [#fmtnum(r, digits: if e < 4 { 1 } else { 3 }) TCN],
        [#fmtnum(emitted / 1000000, digits: 3) M],
        [#fmtnum(cum / 1000000, digits: 3) M],
        [#fmtnum(pct, digits: 2)%],
        [#fmtnum(years, digits: 2)],
      )
    }).flatten()
  ),
  caption: [Cronograma de emissão. As quatro primeiras eras formam o ciclo de estabilização (≈ 4,75 anos, 93,75% da oferta).],
)

#figure(
  {
    let w = 13cm
    let hgt = 5cm
    let years = 12.0
    let pts = range(0, 121).map(i => {
      let y = i / 10.0
      let blocks = y * 31557600 / 60
      let eras-done = calc.floor(blocks / 625000)
      let rem = blocks - eras-done * 625000
      let cum = 50000000 * (1 - 1 / calc.pow(2, eras-done)) + rem * 40 / calc.pow(2, eras-done)
      (w * y / years, hgt - hgt * cum / 50000000)
    })
    box(width: w + 1.4cm, height: hgt + 1.1cm, inset: (left: 1.2cm, bottom: 0.9cm))[
      #place(line(start: (0pt, hgt), end: (w, hgt), stroke: 0.6pt))
      #place(line(start: (0pt, 0pt), end: (0pt, hgt), stroke: 0.6pt))
      #place(line(start: (0pt, 0pt), end: (w, 0pt), stroke: (paint: luma(180), dash: "dashed")))
      #place(polygon(stroke: 1.4pt + accent, fill: none, ..pts, (w, pts.last().at(1)), (w, hgt), (0pt, hgt)))
      #place(dx: w * 4.753 / years, line(start: (0pt, 0pt), end: (0pt, hgt), stroke: (paint: orange, dash: "dotted")))
      #place(dx: w * 4.753 / years + 3pt, dy: hgt * 0.55, text(8pt, fill: orange)[fim do ciclo de\ estabilização])
      #for y in (0, 2, 4, 6, 8, 10, 12) {
        place(dx: w * y / years - 3pt, dy: hgt + 4pt, text(8pt)[#y])
      }
      #for m in (0, 25, 50) {
        place(dx: -1.1cm, dy: hgt - hgt * m / 50 - 5pt, text(8pt)[#m M])
      }
      #place(dx: w / 2 - 1cm, dy: hgt + 16pt, text(8.5pt)[anos após o lançamento])
    ]
  },
  kind: image,
  caption: [Oferta emitida ao longo do tempo (limite de 50 milhões de TCN).],
)

== Por que um ciclo de cinco anos

O Bitcoin leva mais de uma década para emitir a maior parte da oferta. Uma rede nova e pequena
precisa atrair participantes rapidamente e, ao mesmo tempo, chegar logo a uma oferta estável que não
dependa de inflação alta. Em ≈ 4,75 anos, 93,75% das moedas estão distribuídas a quem efetivamente
protegeu a rede. A partir daí, a emissão continua caindo pela metade a cada ≈ 1,19 ano (≈ 1,56 milhão
de TCN na quinta era) e a segurança passa gradualmente a ser financiada pelas taxas — ou, se a
comunidade aprovar, por stake (@sec-roadmap).

== Liberação da recompensa, queima e oferta circulante <sec-cooldown>

- *Liberação escalonada (cooldown).* A recompensa de um bloco (subsídio + taxas do minerador) fica
  pendente no estado. *25%* tornam-se gastáveis após *100 blocos* (`coinbase_maturity`) e os *75%*
  restantes após *1 000 blocos* (`reward_unlock_blocks`, ≈ 16,7 horas). O prazo final foi escolhido
  maior que a reorganização máxima (720): recompensas de blocos que ainda poderiam ser revertidos
  nunca estão totalmente gastáveis (@sec-confirm).
- *Queima de taxas.* A parte da taxa que corresponde à sobretaxa de congestionamento é destruída
  (@sec-fees).
- *Queima de depósitos.* Depósitos de propostas de governança que não atingem quórum são queimados.
- A oferta circulante é `emitido − queimado`, contabilizada no registro global do estado.
- Não há pré-mineração, alocação para fundadores ou reserva de desenvolvimento: todo TCN nasce da
  prova de trabalho.

= Taxas <sec-fees>

== O modelo

Cada transação consome três recursos escassos de todos os nós: *processamento e verificação*
(uma assinatura, um registro de conta), *bytes* (banda e disco para sempre, nos nós arquivo) e, se
executar contrato, *combustível* (tempo de CPU da máquina virtual). A taxa mínima cobra cada um
separadamente e multiplica o total pelo congestionamento atual da rede:

$ "taxa"_"mín" = ceil(("base_fee" + ceil("tamanho" dot "fee_per_kb" / 1000) + ceil("max_fuel" dot "fee_per_kfuel" / 1000)) dot "congestion_bp" / 10000) $

onde `tamanho` é o tamanho serializado em bytes, `max_fuel` é o combustível *reservado* pela
transação (zero para transações nativas) e `congestion_bp` é o multiplicador em pontos-base
(10 000 = 1×). Tudo é aritmética inteira em motes. Com o multiplicador em 1×, a taxa mínima é a
*taxa base* $B$; a diferença $"taxa"_"mín" - B$ é a *sobretaxa de congestionamento*.

#figure(
  table(
    columns: (auto, auto, 1fr),
    align: (left, right, left),
    table.header([Destino], [Valor], [Por quê]),
    [Queimado], [$"taxa"_"mín" - B$], [Mineradores não lucram lotando blocos com transações próprias para empurrar as taxas para cima: a sobretaxa que eles próprios pagariam seria destruída.],
    [Minerador], [$"fee" - ("taxa"_"mín" - B)$], [A taxa base mais toda a gorjeta (o que se paga acima do mínimo) remuneram quem inclui a transação.],
  ),
  caption: [Divisão da taxa paga (`fee`). O recibo de cada transação registra `fee` e `burned`.],
)

== Multiplicador de congestionamento

Ao final de cada bloco, o nó mede a ocupação do bloco — o *maior* entre a fração de bytes usada
(`tamanho / max_block_bytes`) e a fração de combustível reservada (`combustível / max_block_fuel`) —
e ajusta o multiplicador para o bloco seguinte, mirando *50%* de ocupação:

$ c_(h+1) = "limitar"(c_h plus.minus floor((c_h dot |o - 5000|) / (8 dot 5000)), space 1 times, space 1000 times) $

com a ocupação $o$ em pontos-base (0 a 10 000). Um bloco totalmente cheio aumenta o multiplicador em
*12,5%*; um bloco vazio o reduz em 12,5%; ocupações intermediárias movem proporcionalmente, e 50%
exatos o mantêm. A faixa é de *1×* (`CONGESTION_MIN_BP` = 10 000) a *1 000×*
(`CONGESTION_MAX_BP` = 10 000 000). O mecanismo é da mesma família do EIP-1559 do Ethereum [12],
adaptado para dois recursos e com a taxa base (não só a sobretaxa) paga ao minerador.

#figure(
  {
    let w = 12.5cm
    let hgt = 4.4cm
    let c = 10000
    let prev = (0pt, hgt)
    let segs = ()
    let marks = (:)
    for i in range(120) {
      c = next-cong(c, if i < 60 { 10000 } else { 0 })
      let pt = (w * (i + 1) / 120, hgt - hgt * calc.log(c / 10000, base: 10) / 3)
      segs.push((prev, pt))
      prev = pt
    }
    box(width: w + 1.6cm, height: hgt + 1.2cm, inset: (left: 1.3cm, bottom: 0.9cm))[
      #place(rect(width: w / 2, height: hgt, fill: rgb("#fef2f2"), stroke: none))
      #place(line(start: (0pt, hgt), end: (w, hgt), stroke: 0.6pt))
      #place(line(start: (0pt, 0pt), end: (0pt, hgt), stroke: 0.6pt))
      #for (a, b) in segs { place(line(start: a, end: b, stroke: 1.4pt + accent)) }
      #for (m, lbl) in ((0, [1×]), (1, [10×]), (2, [100×]), (3, [1000×])) {
        place(dy: hgt - hgt * m / 3, line(start: (-3pt, 0pt), end: (w, 0pt), stroke: (paint: luma(215), thickness: 0.4pt)))
        place(dx: -1.15cm, dy: hgt - hgt * m / 3 - 5pt, text(8pt, lbl))
      }
      #for b in (0, 20, 40, 60, 80, 100, 120) {
        place(dx: w * b / 120 - 4pt, dy: hgt + 4pt, text(8pt)[#b])
      }
      #place(dx: 6pt, dy: 4pt, text(8pt, fill: burn)[60 blocos 100% cheios])
      #place(dx: w * 0.62, dy: hgt * 0.8, text(8pt, fill: muted)[60 blocos vazios])
      #place(dx: w / 2 - 1.2cm, dy: hgt + 16pt, text(8.5pt)[blocos])
    ]
  },
  kind: image,
  caption: [Multiplicador de congestionamento (escala logarítmica) simulado com a regra de consenso: 6 blocos cheios dobram a taxa, 20 a multiplicam por 10, 59 atingem o teto de 1 000×; 52 blocos vazios a trazem de volta a 1×.],
)

== Por que este modelo

- *Protege contra inundação.* Um atacante que tenta ocupar a rede com transações próprias paga uma
  taxa que cresce 12,5% a cada bloco cheio — em uma hora de blocos cheios, cada transação custa
  1 000× mais — e a sobretaxa é queimada, então nem um minerador atacante recupera o gasto.
- *Favorece usuários reais.* Quem precisa pagar agora paga gorjeta e é priorizado (@sec-priority);
  quem pode esperar aguarda poucos blocos até o multiplicador cair. Transações que ficaram abaixo do
  mínimo por causa de uma alta de congestionamento *permanecem* no mempool e voltam a ser válidas
  quando a taxa cai, em vez de serem descartadas.
- *Cobra o recurso certo.* Uma transferência não paga pelo combustível de contratos; um contrato
  pesado não se esconde atrás de poucos bytes.
- *Continua barata.* Os valores são expressos em motes e são parâmetros de governança com limites
  rígidos (@sec-gov): se o TCN se valorizar, detentores e mineradores podem reduzir `base_fee`,
  `fee_per_kb` e `fee_per_kfuel` sem nenhuma atualização de software.

== Exemplos com os parâmetros padrão da mainnet

Padrões: `base_fee` = 1 000 motes (0,00001 TCN), `fee_per_kb` = 10 000 motes (10 motes por byte),
`fee_per_kfuel` = 1 000 motes por 1 000 de combustível, `storage_deposit_per_kb` = 100 000 motes
(0,001 TCN). Os valores abaixo são calculados neste documento pelas mesmas fórmulas inteiras do código.

#let tr = 159
#figure(
  table(
    columns: (auto, auto, auto, auto, auto),
    align: (left, right, right, right, right),
    table.header([Congestionamento], [Taxa mínima], [Queimado], [Taxa `normal` (1,25×)], [Minerador recebe]),
    ..(10000, 20000, 100000, 10000000).map(cg => {
      let req = req-fee(MAIN, tr, 0, cg)
      let b = base-fee(MAIN, tr, 0)
      let paid = wallet-fee(MAIN, tr, 0, cg, 12500)
      (
        [#fmtnum(calc.quo(cg, 10000))×],
        [#fmtnum(req) motes],
        [#fmtnum(req - b)],
        [#fmtnum(paid) (#tcn(paid))],
        [#fmtnum(paid - (req - b))],
      )
    }).flatten()
  ),
  caption: [Transferência simples de 159 bytes: $B = 1000 + ceil(159 times 10000 slash 1000) = 2590$ motes = 0,0000259 TCN.],
)

*Outros casos a 1× de congestionamento:*

- Transferência com memo de 32 bytes (191 bytes): #fmtnum(req-fee(MAIN, 191, 0, 10000)) motes.
- Pagamento em lote para 128 destinatários (3 719 bytes): #fmtnum(req-fee(MAIN, 3719, 0, 10000))
  motes no total, ≈ #fmtnum(req-fee(MAIN, 3719, 0, 10000) / 128) motes por destinatário.
- *Publicação de contrato.* O contrato de exemplo `token.tccl` (1 701 bytes de código-fonte) gera uma
  transação `Deploy` de *1 860 bytes*. Compilar custa 5 de combustível por byte de fonte (8 505) e o
  `init` consome 995: *9 500 de combustível* medidos. A carteira reserva o medido + 30% + 5 000, ou
  seja, `max_fuel` = 17 350. Taxa mínima: $1000 + 18600 + 17350 = 36950$ motes
  (#tcn(req-fee(MAIN, 1860, 17350, 10000))); com prioridade `normal`,
  #fmtnum(wallet-fee(MAIN, 1860, 17350, 10000, 12500)) motes (#tcn(wallet-fee(MAIN, 1860, 17350, 10000, 12500))).
- *Depósito de armazenamento do mesmo contrato.* Código compilado + armazenamento inicial = 1 311
  bytes → $ceil(1311 slash 1000) times 100000 = 200000$ motes = #tcn(storage-deposit(MAIN, 1311)),
  *devolvidos* quando o armazenamento diminui ou o contrato é destruído.

#note[
  *Conferência em regtest.* A rede local de testes usa parâmetros 10× menores (`base_fee` 100,
  `fee_per_kb` 1 000, `fee_per_kfuel` 100, depósito 10 000 por kB). A mesma publicação medida em
  regtest custou #fmtnum(wallet-fee(REG, 1860, 17350, 10000, 12500)) motes
  (#tcn(wallet-fee(REG, 1860, 17350, 10000, 12500))) com prioridade `normal` e depósito de
  #tcn(storage-deposit(REG, 1311)) — exatamente o que as fórmulas acima preveem.
]

#figure(
  {
    let w = 13cm
    let items = (
      (1000, luma(120), [taxa base]),
      (18600, accent, [bytes]),
      (17350, rgb("#0891b2"), [combustível]),
      (36950, burn, [sobretaxa (queimada)]),
      (18475, orange, [gorjeta `normal`]),
    )
    let total = items.fold(0, (a, it) => a + it.at(0))
    stack(
      spacing: 6pt,
      stack(dir: ltr, ..items.map(it => rect(width: w * it.at(0) / total, height: 0.75cm, fill: it.at(1), stroke: 0.3pt + white))),
      stack(dir: ltr, spacing: 10pt, ..items.map(it => box[#box(width: 7pt, height: 7pt, fill: it.at(1)) #text(8pt)[#it.at(2): #fmtnum(it.at(0))]])),
      text(8pt, fill: muted)[Total pago #fmtnum(total) motes · queimado 36 950 · minerador 55 425],
    )
  },
  kind: image,
  caption: [Composição da taxa de publicação do `token.tccl` com congestionamento em 2× e prioridade `normal`.],
)

== Prioridade: pagar mais para confirmar antes <sec-priority>

Mineradores montam blocos escolhendo primeiro as transações com maior *taxa por unidade de peso*,
respeitando a ordem de nonce de cada remetente:

$ "peso" = "tamanho em bytes" + "max_fuel" / 100 quad quad "prioridade" = "fee" / "peso" $

(100 unidades de combustível equivalem a 1 byte.) A carteira oferece quatro níveis, calculados pelo nó
a partir do mempool (`GET /api/v1/fees`) e aplicados como multiplicador sobre a taxa mínima atual:

#figure(
  table(
    columns: (auto, 1.2fr, 1fr),
    table.header([Nível], [Multiplicador], [Quando usar]),
    [`low`], [1× (mínimo)], [Blocos não estão cheios; não há pressa.],
    [`normal` (padrão)], [1,25×], [Continua válida mesmo se o congestionamento subir no bloco seguinte (alta máxima de 12,5% por bloco).],
    [`high`], [≥ 2× — taxa por peso necessária para caber em um bloco inteiro de transações pendentes, + 0,1×], [Passar à frente da maioria das transações em espera.],
    [`urgent`], [≥ 2 × `high` — taxa para ficar no primeiro quarto do bloco, + 0,25×], [Próximo bloco com probabilidade muito alta.],
  ),
  caption: [Níveis de prioridade (`thecoin-wallet --priority low|normal|high|urgent`).],
)

Com o mempool vazio, `high` = 2× e `urgent` = 4× a taxa mínima. Numa transferência típica a 1×, isso
significa 0,0000259 / 0,0000324 / 0,0000518 / 0,0001036 TCN. O comando `thecoin-wallet fees` mostra os
valores atuais.

= Confirmação rápida e segura <sec-confirm>

Pagamentos do dia a dia precisam ser aceitos em segundos; pagamentos grandes precisam ser
irreversíveis. A v0.2 ataca os dois lados.

== Substituição por taxa opcional

Uma transação enviada com a flag `replaceable` (`--replaceable` na carteira) pode ser trocada no
mempool por outra do mesmo remetente e nonce com taxa *pelo menos 25% maior* (como no BIP-125 [13]; `thecoin-wallet bump-fee <txid>`).
Sem a flag, a primeira versão vista é definitiva para os nós: uma segunda transação
com o mesmo nonce é recusada. Quem recebe um pagamento vê a flag e sabe se pode confiar na transação
ainda não confirmada ou se deve esperar confirmação — o contrário do RBF implícito da v0.1, em que
todo pagamento pendente podia ser substituído.

== Alertas de gasto duplo

Quando um nó recebe uma segunda transação, com o mesmo remetente e nonce de uma pendente não
substituível, ele registra a tentativa e envia a mensagem P2P `DoubleSpend` com *as duas transações
assinadas*. Cada nó que a recebe verifica que ambas são válidas e do mesmo remetente e nonce — a prova
não pode ser forjada sem a chave do remetente — e a repassa. Em segundos toda a rede sabe da tentativa:
lojistas consultam `GET /api/v1/alerts` (ou `thecoin-wallet alerts`), e a consulta da transação passa a
indicar o conflito. Cada nó guarda até 512 alertas recentes.

== Blocos compactos

Um novo bloco é anunciado como *bloco compacto* (no estilo do BIP-152 [14]): cabeçalho + um identificador
curto de 6 bytes por transação, calculado com o hash do bloco como sal (colisões não podem ser
pré-calculadas). O receptor reconstrói o bloco com as transações que já tem no mempool e pede apenas as
que faltam (`GetBlockTxs` → `BlockTxs`); em caso de divergência, baixa o bloco completo. Um bloco cheio
de 6 288 transferências (≈ 1 MB) é anunciado com ≈ 38 kB. O nó valida o cabeçalho e a prova de
trabalho de um bloco compacto antes de alocar qualquer coisa para suas transações.

*Por que isso importa para a segurança:* propagação mais rápida reduz o tempo em que dois
mineradores encontram blocos concorrentes sem saber um do outro — menos blocos órfãos, menos
reorganizações de 1 bloco e menos vantagem para mineradores com melhor conectividade.

== Quantas confirmações esperar

Reverter um pagamento com $N$ confirmações exige que o atacante refaça a prova de trabalho desses $N$
blocos mais rápido que o resto da rede e abra mão das recompensas que a cadeia honesta pagaria. O nó
recomenda o menor $N$ tal que o valor em jogo seja pelo menos o *dobro* do pagamento:

$ N = "limitar"(ceil(2 dot "valor" / "subsídio do próximo bloco"), space 1, space 720) $

disponível em `GET /api/v1/security?amount=<motes>` e `thecoin-wallet confirmations <valor>`.

#let confs(a, sub) = calc.min(calc.max(ceil-div(2 * a, sub), 1), 720)
#figure(
  table(
    columns: (auto, auto, auto, auto),
    align: (right, right, right, right),
    table.header([Valor do pagamento], [Confirmações (era 1, 40 TCN)], [Tempo aproximado], [Era 2 (20 TCN)]),
    ..(1, 20, 100, 1000, 5000, 10000, 20000).map(a => {
      let n = confs(a, 40)
      let mins = n
      (
        [#fmtnum(a) TCN],
        [#fmtnum(n)],
        [#if mins < 60 [#mins min] else [#fmtnum(mins / 60, digits: 1) h]],
        [#fmtnum(confs(a, 20))],
      )
    }).flatten()
  ),
  caption: [Confirmações recomendadas pelo estimador. Acima de 720 blocos (≈ 12 h) a cadeia nunca reorganiza.],
)

== Reorganização máxima e liberação escalonada da recompensa

Duas regras de consenso completam a proteção:

- *Reorganização máxima de 720 blocos.* Nenhum nó abandona mais de 720 blocos de sua cadeia, qualquer
  que seja o trabalho do ramo concorrente. Um pagamento com 720 confirmações é final para todos os nós
  que o viram.
- *Cooldown da recompensa (@sec-cooldown).* Só 25% da recompensa de um bloco é gastável após 100
  blocos; o resto só após 1 000, depois do limite de reorganização. Um minerador que tenta reescrever
  a cadeia arrisca recompensas ainda bloqueadas, e não consegue vender imediatamente o que minera
  para financiar ou lucrar com o ataque.

#figure(
  {
    let w = 13cm
    let hgt = 2.2cm
    let maxb = 1100
    let x(b) = w * b / maxb
    box(width: w + 0.6cm, height: hgt + 1.6cm, inset: (bottom: 1.2cm, top: 0.2cm))[
      #place(dy: 0pt, rect(width: x(100), height: hgt, fill: accent.lighten(35%), stroke: none))
      #place(dx: x(100), dy: hgt * 0.25, rect(width: x(900), height: hgt * 0.75, fill: accent.lighten(35%), stroke: none))
      #place(dx: x(100), dy: 0pt, rect(width: x(1000) , height: hgt * 0.25, fill: rgb("#d1fae5"), stroke: none))
      #place(dx: x(1000), dy: 0pt, rect(width: x(100), height: hgt, fill: rgb("#d1fae5"), stroke: none))
      #place(line(start: (0pt, hgt), end: (w, hgt), stroke: 0.6pt))
      #place(dx: x(720), line(start: (0pt, -4pt), end: (0pt, hgt), stroke: (paint: orange, thickness: 1.2pt, dash: "dashed")))
      #place(dx: x(720) + 3pt, dy: -2pt, text(8pt, fill: orange)[limite de reorganização (720)])
      #place(dx: 4pt, dy: hgt * 0.4, text(8pt, fill: white)[100%])
      #place(dx: x(300), dy: hgt * 0.55, text(8.5pt, fill: white)[75% bloqueado])
      #place(dx: x(300), dy: hgt * 0.02, text(8pt, fill: accent)[25% gastável])
      #place(dx: x(1010), dy: hgt * 0.4, text(8pt, fill: accent)[100%\ gastável])
      #for b in (0, 100, 500, 720, 1000) {
        place(dx: x(b) - 5pt, dy: hgt + 4pt, text(8pt)[#b])
      }
      #place(dx: w / 2 - 2.5cm, dy: hgt + 18pt, text(8.5pt)[blocos após o bloco minerado])
    ]
  },
  kind: image,
  caption: [Liberação da recompensa de um bloco na mainnet (subsídio + taxas do minerador); escuro = bloqueado, claro = gastável.],
)

= Estado global e armazenamento compacto <sec-storage>

== Registros de estado

O estado é um mapa chave-valor com prefixos: contas (saldo, nonce, bloqueio de votos), contratos
nativos, propostas, votos, recompensas pendentes, o registro global (emissão, queima, parâmetros de
governança, propostas em votação e o multiplicador de congestionamento) e, desde a v0.2, três prefixos
para contratos TCCL: metadados, código compilado e armazenamento. Contas vazias não são armazenadas;
contratos concluídos ou destruídos são apagados. O estado cresce com o número de contas e contratos
ativos, não com o histórico — e o armazenamento de contratos é coberto por depósito.

== Compromisso homomórfico de estado (LtHash)

Todo cabeçalho contém a raiz do estado após o bloco. Recalcular uma árvore Merkle de todo o estado a
cada bloco seria caro para máquinas pequenas. The Coin usa *LtHash16*: cada registro
$(k, v)$ é expandido por BLAKE3-XOF em um vetor de 2048 inteiros de 16 bits, e o compromisso é a
soma, módulo $2^16$, dos vetores de todos os registros:

$ L("estado") = sum_((k,v) in "estado") "XOF"(k, v) mod 2^16, quad "state_root" = H_"state-root"(L) $

Inserir ou remover um registro é somar ou subtrair um vetor, então atualizar o compromisso custa
proporcionalmente aos registros alterados pelo bloco. A segurança se apoia no problema de reticulados
*Short Integer Solution*. Qualquer divergência — bug, corrupção de disco ou manipulação — faz o nó
rejeitar o bloco imediatamente, e ao iniciar o nó confere que o compromisso salvo corresponde ao topo.

== Banco de dados (esquema 2)

#figure(
  table(
    columns: (auto, auto, 1fr),
    table.header([Tabela], [Chave], [Valor]),
    [`headers`, `main`], [hash / altura], [Cabeçalhos com trabalho acumulado; cadeia principal.],
    [`blocks`], [hash], [Corpo do bloco, zstd.],
    [`state`], [chave de estado], [Registro de estado (Borsh).],
    [`undo`], [altura], [Dados para reorganização, zstd; só os últimos 736 blocos.],
    [`txindex`], [txid], [Altura (8) + posição (4).],
    [`addrindex`], [endereço + altura + posição], [*Vazio* na v0.2 (a v0.1 guardava o txid de 32 bytes).],
    [`receipts`], [txid], [*Novo:* recibo da transação, zstd.],
    [`meta`], [nome], [Topo, compromisso de estado, versão do esquema.],
  ),
  caption: [Tabelas do banco `chain.redb`.],
)

- *redb*: banco embarcado em Rust puro, com transações ACID e árvores B copy-on-write. Cada bloco —
  estado, undo, índices, recibos, topo e compromisso — é gravado em *uma* transação: uma queda de
  energia nunca deixa um bloco pela metade.
- *Recibos*: para cada transação confirmada o nó guarda taxa, parte queimada, endereços afetados,
  contrato criado, sucesso ou erro, combustível usado, eventos emitidos e valor de retorno. Exploradores
  e carteiras mostram o resultado real de cada transação sem reexecutá-la.
- *Índice de endereços compacto*: a entrada é apenas a chave ordenada; o txid é lido do bloco na
  consulta. Isso eliminou 32 bytes por par (endereço, transação).
- *Poda*: nós podados mantêm os últimos `prune_keep` corpos de blocos (mínimo 1 000), cabeçalhos e
  estado. Ao podar um bloco o nó remove também suas entradas no índice de transações e seus recibos, e
  nós podados não mantêm o índice de endereços, que apontaria para corpos apagados.
- *Mempool persistente*: transações pendentes sobrevivem a reinicializações (`mempool.dat`).
- *Compactação*: `thecoind compact` (com o nó parado) devolve ao sistema o espaço pré-alocado.
- *Cache configurável* (64 MB por padrão).

Os números medidos e as projeções de disco estão na @sec-perf.

= Transações e pagamentos

- *Transferência* com *memo* de até 256 bytes (número de pedido, fatura, mensagem).
- *Pagamento em lote* para até 128 destinatários em uma única transação (folha de pagamento,
  pools, exchanges).
- *Validade* opcional (`expiry_height`) para que cobranças antigas não sejam executadas tarde demais.
- *Prioridade* e *substituição por taxa opcional* (@sec-priority, @sec-confirm).
- *Simulação antes de assinar*: `POST /api/v1/tx/simulate` executa a transação sobre o estado atual e
  devolve validade, taxa mínima, combustível usado, eventos e retorno — a carteira usa isso para medir o
  combustível e para nunca enviar uma chamada de contrato que falharia.
- *Solicitações de pagamento* padronizadas por URI, prontas para QR code:

```
thecoin:tc1q...?amount=12.5&memo=Pedido%20%23123&label=Loja
```

= Contratos de pagamento nativos

Para os casos mais comuns, a rede oferece *modelos nativos de contratos de pagamento*. Eles não são
Turing-completos, não têm combustível, laços ou reentrância, e cada chamada tem custo fixo — portanto
são seguros e baratos de validar em máquinas pequenas. Os fundos ficam no registro do contrato e só
saem pelas regras abaixo.

#figure(
  table(
    columns: (auto, 1fr, 1fr),
    table.header([Modelo], [Regras], [Casos de uso]),
    [*Escrow*], [Pagador bloqueia o valor. Pagador ou árbitro liberam ao recebedor; recebedor ou árbitro reembolsam; após o prazo o pagador reembolsa sozinho.], [Comércio eletrônico, serviços, compra entre desconhecidos.],
    [*Vesting*], [Libera linearmente entre início e fim, nada antes do *cliff*. Se revogável, o criador recupera a parte não liberada.], [Salários, bônus, distribuição a equipes.],
    [*Assinatura*], [Pagamento pré-pago de N períodos; o recebedor saca um período imediatamente e mais um a cada `period_blocks`; o pagador cancela e recupera os períodos futuros.], [SaaS, mensalidades, aluguel.],
    [*HTLC*], [Paga ao destinatário quem revelar a pré-imagem com SHA-256 igual ao *hash lock* até o prazo; depois, o remetente recupera.], [Trocas atômicas com Bitcoin e outras redes, canais de pagamento.],
    [*Multisig*], [Cofre controlado por M de N signatários (até 16); gastos propostos e aprovados on-chain. *Novo:* `MultisigClose` apaga um cofre vazio.], [Tesouraria de empresas e associações, custódia compartilhada.],
  ),
  caption: [Modelos nativos de contratos de pagamento.],
)

O identificador de um contrato nativo é $H_"contract-id"$(criador ‖ nonce), conhecido antes mesmo
da confirmação. Na v0.2, criar um contrato nativo também trava um *depósito de armazenamento*
proporcional ao tamanho do registro (`storage_deposit_per_kb`), devolvido ao criador quando o contrato
termina (saldo zerado) ou, no caso do multisig, quando é fechado por um signatário.

= Contratos inteligentes TCCL <sec-tccl>

A v0.1 previa uma VM WebAssembly para contratos gerais. A v0.2 entrega, em vez disso, uma linguagem
própria, pequena e verificável: *TCCL — The Coin Cloud Language*. A escolha é deliberada: uma
linguagem de alto nível com regras estritas e uma VM interpretada simples são mais fáceis de auditar e
de reimplementar do que um motor WebAssembly completo, e cabem no orçamento de máquinas modestas.

== Princípios de projeto

- *Tipos estritos.* Toda constante, variável de estado, parâmetro e variável local declara o tipo
  (`int`, `bool`, `text`, `bytes`, `address`, `list[T]`, `map[K, V]`). Cada expressão tem exatamente
  um tipo; não há conversões implícitas; funções com retorno retornam em todos os caminhos. `int` é um
  inteiro de 128 bits com toda aritmética verificada: estouro aborta a chamada.
- *Funções com papéis explícitos.* `init` roda na publicação; `action` muda estado e é chamada por
  transação; `view` é somente leitura, gratuita pela API, e o compilador garante que não altera estado,
  não envia TCN nem emite eventos; `fn` é auxiliar interna. Só funções `payable` recebem TCN.
- *Determinismo.* Não há ponto flutuante, relógio nem aleatoriedade; o contexto é só `caller`,
  `value`, `balance`, `height` e `self`. A mesma chamada sobre o mesmo estado dá o mesmo resultado em
  todos os nós.
- *Sem chamadas entre contratos.* Um contrato pode enviar TCN a qualquer endereço, mas não chama outro
  contrato — reentrância é impossível por construção.
- *O compilador faz parte do consenso.* A transação `Deploy` carrega o *código-fonte*; cada nó o
  compila de forma idêntica e guarda o programa compilado. O código-fonte fica na cadeia para sempre
  (o contrato registra o txid de publicação e o hash BLAKE3 do fonte): qualquer pessoa pode ler e
  verificar exatamente o que um contrato faz.
- *Combustível.* Cada operação consome combustível; a transação declara `max_fuel` e paga por ele
  (@sec-fees). Acabou o combustível, a chamada aborta.
- *Falha paga taxa e reverte.* Toda execução roda numa camada de estado filha. Se o contrato falhar
  (`require` falso, falta de combustível, estouro, erro de compilação), essa camada é descartada: o
  valor enviado, as escritas, os envios e os eventos são revertidos; só a taxa é cobrada e o recibo
  registra o erro. O mempool simula cada chamada e *recusa* as que falhariam, para que o usuário não
  pague por um erro previsível.

== Exemplo

Contrato completo `crates/tccl/examples/tip_jar.tccl`, reproduzido literalmente:

```
# A tip jar: anyone can send TCN with a message, only the owner withdraws.
contract TipJar

state owner: address
state total_received: int
state tips: int

event Tip(from: address, amount: int, message: text)
event Withdrawn(to: address, amount: int)

init():
    owner = caller

action tip(message: text) payable:
    require value >= TCN / 100, "minimum tip is 0.01 TCN"
    require len(message) <= 140, "message too long"
    total_received += value
    tips += 1
    emit Tip(caller, value, message)

action withdraw(amount: int):
    require caller == owner, "only the owner can withdraw"
    require amount > 0 and amount <= balance, "invalid amount"
    send(owner, amount)
    emit Withdrawn(owner, amount)

view stats() -> list[int]:
    return [total_received, tips, balance]
```

Fluxo típico, do simulador local à rede:

```
tccl check tip_jar.tccl                  # compila e mostra a interface
tccl run tip_jar.tccl deploy             # simulador local com contas de teste
thecoin-wallet contract deploy tip_jar.tccl
thecoin-wallet contract invoke <endereço> tip "obrigado!" --value 0.05
thecoin-wallet contract view <endereço> stats
```

O repositório traz ainda `counter`, `token`, `crowdfund` e `private_pool`. O guia de receitas em inglês
*TCCL Cookbook* (`docs/tccl/tccl-cookbook.pdf`) cobre a linguagem em detalhe.

== Recibos e eventos

Cada transação confirmada gera um *recibo* (@sec-storage) com sucesso ou erro, combustível usado,
valor de retorno e os *eventos* emitidos (`emit`, até 64 por chamada), cada um com o endereço do
contrato, o nome e os campos tipados. Eventos são a interface para carteiras, exploradores e
aplicações: um lojista acompanha `Tip` sem ler o armazenamento do contrato.

== Custos e limites

#grid(
  columns: (1fr, 1fr),
  gutter: 12pt,
  figure(
    table(
      columns: (1fr, auto),
      align: (left, right),
      table.header([Operação], [Combustível]),
      [Instrução / expressão], [2 / 1],
      [Chamada de função], [20],
      [Leitura de armazenamento], [250],
      [Escrita], [400 + 4/byte],
      [Hash (SHA-256, BLAKE3)], [60 + 20/64 bytes],
      [Verificar Ed25519], [3 500],
      [Verificar assinatura em anel], [5 000 + 10 000/membro],
      [`send` / `emit` / `destroy`], [300 / 100 / 1 000],
      [Compilar (publicação)], [5/byte de fonte],
      [Carregar contrato (chamada)], [100 + 1/100 bytes],
    ),
    caption: [Tabela de combustível (consenso).],
  ),
  figure(
    table(
      columns: (1fr, auto),
      align: (left, right),
      table.header([Limite], [Valor]),
      [Código-fonte], [48 000 bytes],
      [Transação `Deploy`], [64 000 bytes],
      [Programa compilado], [262 144 bytes],
      [Combustível por transação], [10 000 000],
      [Combustível por bloco (padrão)], [50 000 000],
      [Argumentos por chamada], [32],
      [Eventos por chamada], [64],
      [Tamanho de um valor], [64 KiB],
      [Itens de lista em memória], [4 096],
      [Profundidade de chamadas], [16],
      [Membros de um anel], [64],
      [Operadores encadeados / profundidade], [64 / 128],
      [Aninhamento de blocos e tipos], [32],
    ),
    caption: [Limites da linguagem e da VM.],
  ),
)

A taxa é calculada sobre o combustível *reservado* (`max_fuel`), que também conta para o limite do
bloco; por isso a carteira mede o consumo real por simulação e reserva só uma margem.

Os preços foram *calibrados por medição* (`crates/tccl/tests/fuel_bench.rs` e
`crates/node/tests/fuel_storage_bench.rs`, com 400 000 entradas reais no banco): cada operação custa
cerca de 20 ns de CPU por unidade de combustível numa VPS de 2 vCPU. Assim, mesmo um bloco
*completamente cheio* (50 milhões) com a pior operação possível executa em cerca de 1,2 s — pouco diante
dos 60 s de intervalo — e ninguém consegue produzir blocos lentos de validar sem pagar por isso.

== Depósito de armazenamento reembolsável

Estado que fica para sempre em todos os nós tem um custo que uma taxa única não cobre. Por isso todo
contrato mantém um *depósito* de `storage_deposit_per_kb` por kB de estado (código compilado + chaves e
valores do armazenamento), arredondado para cima em kB:

- Quem faz o estado *crescer* paga a diferença, até o limite `max_deposit` que declarou na transação
  (se exceder, a chamada falha e é revertida).
- Quem faz o estado *diminuir* recebe de volta a parte proporcional:
  $"reembolso" = "depósito" dot ("bytes"_"antes" - "bytes"_"depois") / "bytes"_"antes"$.
- `destroy(to)` apaga o contrato e envia a `to` o saldo restante *e todo o depósito*.

O efeito é um incentivo econômico para limpar estado — apagar entradas de um mapa devolve dinheiro — e
uma garantia de que o crescimento do estado é sempre pago por quem o causa, não pelos operadores de nós.

== Liberdade total para desenvolvedores

Publicar um contrato não exige permissão, lista de aprovação ou taxa especial: qualquer código que o
compilador aceite pode ser publicado por qualquer pessoa, e qualquer pessoa pode chamá-lo. O protocolo
não tem chaves de administrador sobre contratos; as regras de cada contrato são as que estão no seu
código-fonte público. Tokens, financiamento coletivo, cofres, jogos, votações e pools de privacidade
são aplicações, não mudanças de protocolo. Os limites existem apenas para proteger os nós — combustível,
tamanho e depósito — e são iguais para todos.

= Privacidade opcional por contratos <sec-privacy>

A camada base da The Coin é *transparente*: saldos, remetentes, destinatários e valores são públicos,
como no Bitcoin, o que facilita auditoria, conformidade e verificação da oferta. A privacidade é
*opcional* e construída em contratos, com uma primitiva criptográfica disponível na TCCL.

== Assinaturas em anel (bLSAG)

`ring_verify(anel, mensagem, assinatura, key_image)` verifica uma assinatura em anel ligável no
esquema *bLSAG* (Back's Linkable Spontaneous Anonymous Group, como no Monero [15]), sobre o grupo de ordem
prima *Ristretto255* [16] (sem as armadilhas de cofator da curva 25519), com todos os hashes separados por
domínio (`crates/tccl/src/ring.rs`):

- A assinatura prova "o signatário possui a chave secreta de *um* destes $n$ membros" sem revelar qual.
  Ela tem $32 (n + 1)$ bytes e está ligada à mensagem e ao conjunto exato de membros.
- A *key image* $I = x dot H_p(P)$ é única por chave secreta: a mesma chave produz sempre a mesma
  imagem, em qualquer anel. Um contrato que guarda as imagens usadas impede que a mesma chave assine
  duas vezes — sem que a imagem revele qual membro a produziu.

== Pool de valor fixo

O exemplo `crates/tccl/examples/private_pool.tccl` implementa a interface padrão *TCCL-PRIV-1*
usada pela carteira:

#figure(
  {
    let bx(title, body, fill: rgb("#ecfdf5")) = box(width: 100%, inset: 7pt, radius: 3pt, fill: fill, stroke: 0.6pt + accent)[
      #set text(8.5pt)
      #set align(left)
      #set par(justify: false)
      *#title* \ #body
    ]
    let arrow = align(horizon + center, text(14pt, fill: accent)[→])
    grid(
      columns: (1fr, auto, 1fr, auto, 1fr, auto, 1fr),
      gutter: 4pt,
      bx([1. Depósito], [Alice, Bob, Carol… enviam exatamente 10 TCN com uma chave pública de anel nova. A lista de chaves é pública.]),
      arrow,
      bx([2. Espera], [Mais depósitos entram. Quanto mais membros e mais tempo, maior o anonimato.], fill: luma(245)),
      arrow,
      bx([3. Saque], [Assinatura em anel sobre $k$ depósitos (2 a 32) + key image, ligada a `to`, `relayer` e `fee`.]),
      arrow,
      bx([4. Pagamento], [Contrato confere a assinatura e a imagem não usada; envia 10 TCN − `fee` a um endereço novo e `fee` ao relayer.], fill: rgb("#fff7ed")),
    )
  },
  kind: image,
  caption: [Fluxo do pool de privacidade de valor fixo.],
)

Trecho literal da função de saque:

#[
#set text(9.2pt)
```
action withdraw(to: address, relayer: address, fee: int, members: list[int], signature: bytes, key_image: bytes):
    require len(members) >= MIN_RING and len(members) <= MAX_RING, "ring size must be 2 to 32"
    require fee >= 0 and fee < DENOMINATION, "invalid relayer fee"
    require not used.has(key_image), "this deposit was already withdrawn"
    let ring: list[bytes] = []
    for i in members:
        require i >= 0 and i < len(keys), "unknown deposit index"
        ring.push(keys[i])
    let message: bytes = withdraw_message(to, relayer, fee)
    require ring_verify(ring, message, signature, key_image), "invalid ring signature"
    used[key_image] = true
    withdrawn += 1
    send(to, DENOMINATION - fee)
    if fee > 0:
        send(relayer, fee)
    emit Withdrawn(len(members))
```
]

*Valor fixo* elimina a correlação por valor: todo depósito e todo saque são de 10 TCN. *Relayers*
resolvem o problema do endereço novo sem saldo: qualquer terceiro pode enviar a transação de saque
(pagando a taxa de rede) e receber `fee` do próprio saque. Como a mensagem assinada inclui `to`,
`relayer` e `fee`, o relayer não pode desviar o valor nem alterar sua remuneração. Na carteira:
`thecoin-wallet privacy keygen | deposit | withdraw | status`; as chaves de anel derivam da mesma frase de
recuperação, e o saque escolhe membros aleatórios (16 por padrão).

== Limites do anonimato

#warnbox[
  - *O conjunto de anonimato é o anel, não o pool.* Um saque com anel de 16 esconde o depositante entre
    16 depósitos — não entre todos. Use anéis maiores (até 32 neste pool) quando o pool permitir.
  - *Correlação por tempo.* Sacar logo após depositar, quando há poucos depósitos novos, reduz o
    anonimato na prática. Espere o pool crescer e evite padrões (sempre à mesma hora, logo após
    receber).
  - *Quem envia a transação de saque é público.* Se você enviar o saque do mesmo endereço que
    depositou, a ligação é óbvia; a carteira avisa. Use um relayer ou um endereço sem relação.
  - *Metadados fora da cadeia.* O IP do nó que primeiro retransmite a transação, o uso posterior dos
    fundos junto com fundos identificados e a reutilização de endereços podem revelar a ligação.
  - *Depósitos e valores continuam visíveis.* O pool esconde *qual* depósito corresponde a *qual*
    saque; não esconde que alguém usou o pool.
]

A camada base permanece transparente e auditável; a privacidade é uma escolha do usuário, com custo
e limites conhecidos, e novas construções (outros valores, outros esquemas) podem surgir como
contratos sem mudar o protocolo.

= Governança bicameral <sec-gov>

The Coin foi pensada para evoluir sem donos. Mudanças de parâmetros são decididas on-chain por um
sistema *bicameral*: uma proposta só passa se as duas câmaras aprovarem.

+ *Câmara dos detentores.* Qualquer endereço vota com um peso em TCN. As moedas usadas no voto
  ficam *bloqueadas* até o fim da votação e não podem ser movidas — portanto não podem ser
  transferidas para outro endereço e usadas de novo. Cada endereço vota uma vez por proposta.
+ *Câmara dos mineradores.* Cada proposta em votação recebe um bit do campo `signal` (até 32
  propostas simultâneas); mineradores que apoiam a proposta ligam o bit nos blocos que produzem.

#figure(
  table(
    columns: (auto, 1fr),
    table.header([Critério], [Regra (em pontos-base, 10 000 = 100%)]),
    [Quórum], [votos totais (sim + não + abstenção) ≥ `quorum_bp` × oferta circulante],
    [Aprovação dos detentores], [sim ≥ `approval_bp` × (sim + não)],
    [Aprovação dos mineradores], [blocos sinalizando ≥ `miner_approval_bp` × blocos da votação],
    [Duração], [`vote_period` blocos],
    [Ativação], [`activation_delay` blocos após aprovação],
    [Depósito], [`proposal_deposit`, devolvido se houver quórum, queimado caso contrário],
  ),
  caption: [Regras de aprovação de propostas.],
)

*Ciclo de vida:* `Votação` → ao fim, `Aprovada` ou `Rejeitada` → após o atraso, `Ativada`. O atraso
dá tempo para operadores de nós atualizarem o software quando necessário.

== Parâmetros governáveis

#figure(
  table(
    columns: (auto, auto, auto),
    align: (left, right, right),
    table.header([Parâmetro], [Padrão mainnet], [Limites rígidos (mín.–máx.)]),
    [`max_block_bytes`], [1 000 000 (1 MB)], [250 000 – 8 000 000],
    [`max_block_fuel`], [50 000 000], [5 000 000 – 500 000 000],
    [`base_fee`], [1 000 motes (0,00001 TCN)], [0 – 10 000 000],
    [`fee_per_kb`], [10 000 motes/kB], [1 – 100 000 000],
    [`fee_per_kfuel`], [1 000 motes/1 000 comb.], [1 – 100 000 000],
    [`storage_deposit_per_kb`], [100 000 motes/kB (0,001 TCN)], [0 – 1 000 000 000],
    [`proposal_deposit`], [100 TCN], [1 – 1 000 000 TCN],
    [`vote_period`], [20 160 blocos (≈ 14 dias)], [1 440 – 201 600],
    [`quorum_bp`], [1 000 (10%)], [100 – 5 000 (1% – 50%)],
    [`approval_bp`], [6 667 (66,67%)], [5 001 – 9 500],
    [`miner_approval_bp`], [6 000 (60%)], [5 000 – 9 500],
    [`activation_delay`], [2 880 blocos (≈ 2 dias)], [720 – 43 200],
  ),
  caption: [Os 12 parâmetros de governança da v0.2 (`crates/core/src/params.rs`). Testnet usa os mesmos limites; seus padrões diferem em depósito (10 TCN), votação (1 440), quórum (5%), mineradores (50%) e ativação (720).],
)

Uma proposta `SetParam` com valor fora dos limites é recusada na criação, e o valor é conferido de novo
na ativação. Propostas de *texto* registram decisões da comunidade, e propostas de *atualização de
software* registram a versão aprovada e o hash da release; nós desatualizados passam a exibir um aviso.
É por esse mecanismo que as fases do plano de consenso seriam ativadas (@sec-roadmap).

== Sinalização de mineradores

Um minerador declara quais propostas apoia pelo identificador (64 caracteres hexadecimais):

```
thecoind --signal <id>       # repetível (ou mining.signal no thecoind.toml)
thecoin signal <id>          # nó instalado: grava a configuração e reinicia
thecoin unsignal <id>
thecoin signals              # propostas abertas e as que seus blocos apoiam
```

O nó liga o bit correspondente apenas enquanto a proposta está em votação; bits não atribuídos tornam o
bloco inválido.

#warnbox[
  *O que nunca pode mudar por votação:* o limite de 50 milhões de TCN, o cronograma de emissão, o
  algoritmo de prova de trabalho e as regras de validade de transações. Alterá-los exigiria que cada
  operador instalasse voluntariamente um software diferente — exatamente como no Bitcoin.
]

Nenhum grupo decide sozinho: grandes detentores não mudam a rede sem os mineradores, e grandes
mineradores não mudam a rede sem os detentores. O site the-coin.cloud exibe cada proposta, o
andamento das duas câmaras, o quórum e o tempo restante, lendo diretamente a API dos nós.

= Rede ponto a ponto

- *Transporte:* TCP. Cada mensagem é um *frame* com magic da rede (4 bytes), tamanho, checksum e
  payload Borsh; frames acima de 9 MiB ou malformados derrubam a conexão.
- *Handshake:* troca de `Version` (gênese, altura, serviços, nonce anti-autoconexão) e `Verack`.
  Nós de outra rede ou gênese são desconectados.
- *Sincronização:* localizadores de blocos (densos no topo, esparsos para trás) → até 500 hashes
  por `Inv` → `GetData` → blocos. A prova de trabalho dos blocos recebidos é verificada em paralelo e
  os blocos são aplicados em ordem por uma thread dedicada.
- *Propagação:* transações por `Inv` apenas a quem ainda não as conhece; novos blocos como *blocos
  compactos* (`CompactBlock`, `GetBlockTxs`, `BlockTxs`); o mempool de um novo par é solicitado na
  conexão.
- *Alertas:* `DoubleSpend` carrega duas transações conflitantes assinadas; é verificado e repassado
  uma única vez por par de transações.
- *Descoberta:* seeds DNS (`seed1.the-coin.cloud`, `seed2.the-coin.cloud`), troca de endereços
  e um gerenciador de endereços persistente com *back-off* exponencial.
- *Proteção contra abuso:* limites de conexões de entrada (32) e por IP, filas de escrita
  limitadas (pares lentos são desconectados), pontuação de mau comportamento com banimento de 24 h,
  limites estruturais de mensagens (até 65 536 IDs por bloco compacto), timeouts de handshake e ping.

= Nó, mineração e instalação <sec-install>

== Modelo de execução

O nó foi desenhado para não travar a máquina nem a si mesmo:

- rede e API usam um runtime assíncrono Tokio com apenas 2 threads;
- blocos são validados por *uma* thread dedicada, com fila, isolando trabalho pesado da rede;
- leituras (API, P2P) usam transações MVCC do banco e não bloqueiam a escrita;
- threads de mineração rodam com prioridade mínima e, por padrão, usam todos os núcleos menos um;
- o minerador monta o bloco pela ordem de prioridade do mempool, respeitando os limites de bytes e
  de combustível, e reconstrói o bloco-modelo a cada novo topo ou lote de transações;
- a mineração pausa enquanto o nó sincroniza;
- a API REST tem limite de concorrência, de corpo (256 KiB) e de tempo por requisição.

== Virar validador com uma linha

```
curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes
```

Sem `--yes`, o instalador é interativo e pergunta antes de criar a carteira. Ele:

+ faz verificações prévias: sistema e arquitetura (x86_64 ou ARM64), systemd, RAM e disco,
  sincronização do relógio e firewall; oferece criar 1 GB de swap em máquinas com menos de 2 GB sem swap;
+ baixa `thecoind`, `thecoin-wallet` e `tccl` e *verifica o SHA-256*, recorrendo à compilação do
  código-fonte quando não há binário;
+ cria o usuário de sistema sem privilégios `thecoin` e `/var/lib/thecoin`;
+ cria a carteira que receberá as recompensas (ou usa `--miner-address`). No modo não interativo, a
  senha é aleatória e fica num arquivo legível só pelo root, e a frase de recuperação é mostrada ao final;
+ gera `/etc/thecoin/thecoind.toml` e um serviço systemd endurecido (`ProtectSystem=strict`,
  `NoNewPrivileges`, sem capabilities), abre a porta P2P no ufw quando ativo;
+ instala o comando `thecoin` e inicia o nó, que começa a sincronizar e minerar.

Rodar de novo atualiza os binários mantendo configuração e carteira.

#figure(
  table(
    columns: (auto, auto, 1fr),
    table.header([Opção], [Variável], [Efeito]),
    [`--network mainnet|testnet`], [`THECOIN_NETWORK`], [Rede (padrão mainnet).],
    [`--miner-address <tc1…>`], [`THECOIN_MINER_ADDRESS`], [Recompensas para um endereço existente, sem criar carteira.],
    [`--no-mine`], [`THECOIN_NO_MINE=1`], [Nó completo sem minerar.],
    [`--threads <n>`], [`THECOIN_THREADS`], [Threads de mineração (0 = núcleos − 1).],
    [`--version <x.y.z|latest>`], [`THECOIN_VERSION`], [Versão a instalar.],
    [`--from-source`], [`THECOIN_FROM_SOURCE=1`], [Compilar em vez de baixar.],
    [`--public-api`], [`THECOIN_PUBLIC_API=1`], [API em 0.0.0.0 (padrão: só 127.0.0.1).],
    [`--yes`, `-y`], [`THECOIN_YES=1`], [Não interativo: nunca pergunta, aceita padrões.],
  ),
  caption: [Opções do instalador (`installer/install.sh`).],
)

== O comando `thecoin`

#figure(
  table(
    columns: (auto, 1fr),
    table.header([Comando], [Função]),
    [`thecoin status`], [Estado do nó, sincronização, pares, mineração e saldo.],
    [`thecoin logs`], [Acompanha os logs do serviço.],
    [`thecoin address` / `balance`], [Endereço de mineração e saldo da carteira.],
    [`thecoin mnemonic`], [Mostra a frase de recuperação (guarde em segredo).],
    [`thecoin restart`], [Reinicia o serviço.],
    [`thecoin signals` / `signal <id>` / `unsignal <id>`], [Sinalização de governança (@sec-gov).],
    [`thecoin update`], [Atualiza para a última versão mantendo configuração e carteira.],
    [`thecoin uninstall [--purge]`], [Remove o nó (`--purge` apaga dados da cadeia e configuração; arquivos de carteira nunca são apagados).],
    [`thecoin version`], [Versões instaladas.],
  ),
  caption: [Comando auxiliar para operar um nó instalado. Operações de carteira: `thecoin-wallet --help`.],
)

== Requisitos

#figure(
  table(
    columns: (auto, auto, auto),
    table.header([Recurso], [Mínimo], [Recomendado]),
    [CPU], [1 vCPU x86_64 ou ARM64], [2+ vCPU],
    [Memória], [1 GB (com swap)], [2 GB],
    [Disco], [5 GB (nó podado)], [20 GB SSD (arquivo: ver @sec-perf)],
    [Rede], [porta TCP 7333 aberta], [IP público estável],
    [Sistema], [Linux com systemd], [Ubuntu/Debian LTS],
  ),
  caption: [Requisitos de um nó minerador na fase de prova de trabalho.],
)

= Carteira e padrões para desenvolvedores

A carteira de referência usa exclusivamente padrões abertos, para que qualquer carteira compatível
derive os mesmos endereços a partir das mesmas palavras:

- *BIP-39*: frase de recuperação de 12 ou 24 palavras em inglês.
- *SLIP-0010 Ed25519*: caminho `m/44'/7333'/conta'/0'/índice'` (todos endurecidos). As chaves de anel
  de privacidade também derivam da frase.
- *Arquivo de carteira v1*: JSON com a frase cifrada por ChaCha20-Poly1305 com chave derivada da
  senha por Argon2id (64 MiB, 3 passes), gravado com permissão 0600.
- *Transações*: codificação Borsh documentada byte a byte, com vetores de teste; taxa calculada com as
  mesmas funções de consenso (`required_fee`).
- *Comandos*: `send`, `batch`, `pay` (URI), `bump-fee`, `confirmations`, `alerts`, `fees`, `contract …`
  (nativos e TCCL), `privacy …`, `gov …`, com as opções globais `--priority`, `--replaceable` e
  `--dry-run`.

A biblioteca `thecoin-core` não tem I/O e compila para qualquer plataforma suportada pelo Rust,
inclusive WebAssembly, o que permite carteiras web e móveis reutilizarem exatamente as regras de
consenso. A documentação do repositório (`docs/WALLET_DEVELOPERS.md`, `docs/PROTOCOL.md`,
`docs/API.md`, `docs/CONTRACTS.md`) descreve tudo o que é necessário para criar novas carteiras.

= API e integração com o site

Cada nó expõe uma API REST JSON (`/api/v1`) com status da rede (incluindo o multiplicador de
congestionamento), oferta, taxas e níveis de prioridade, blocos, transações com recibos e eventos,
endereços e histórico, contratos nativos, contratos TCCL (`/program/{addr}` e consultas `view`
gratuitas), simulação de transações, estimador de confirmações, alertas de gasto duplo, propostas de
governança e parâmetros, mempool, pares e mineração. Valores são inteiros em motes, hashes em
hexadecimal e endereços em bech32m.

A API vem ligada apenas em `127.0.0.1`. Para o site the-coin.cloud, hospedado em outra máquina, os
nós seed expõem a API somente ao IP do servidor web, e o nginx do site faz *proxy* com TLS, cache
curto, limite de requisições e *failover* entre os dois nós. O explorador, o painel de governança e as
estatísticas da página inicial consomem essa API — não há banco de dados central: os dados exibidos
são os mesmos que qualquer pessoa pode verificar rodando o próprio nó.

= Segurança

#figure(
  table(
    columns: (auto, 1fr),
    table.header([Ameaça], [Mitigação]),
    [Alterar transações confirmadas], [Cada bloco compromete transações (Merkle), estado (LtHash) e o bloco anterior; reescrever o histórico exige refazer todo o trabalho acumulado.],
    [Gasto duplo / ataque de 51%], [Cadeia de maior trabalho; reorganizações acima de 720 blocos recusadas; checkpoints; recompensa liberada 25% após 100 blocos e 75% após 1 000; estimador de confirmações por valor; alertas de gasto duplo; substituição só com opt-in.],
    [Inundação de transações], [Taxa base por transação e por byte; multiplicador de congestionamento até 1 000× com sobretaxa queimada; mempool limitado com expulsão por prioridade.],
    [Contratos abusivos], [Combustível pago e limitado por transação e por bloco; limites de memória, profundidade e tamanho; depósito de armazenamento; sem chamadas entre contratos (sem reentrância); falha reverte tudo menos a taxa.],
    [Inflação indevida], [Emissão calculada deterministicamente e verificada por todos os nós; teto de 50M checado em cada bloco; aritmética com verificação de overflow (também na VM).],
    [Replay de transações], [Nonce por conta e `chain_id` por rede.],
    [Maleabilidade], [Codificação canônica e Ed25519 estrito.],
    [Manipulação de timestamp], [Median time past, deriva máxima de 180 s, limites do LWMA e variação máxima de 2× por bloco.],
    [Negação de serviço], [Validação barata antes da cara e PoW verificada antes de aceitar blocos; pool de órfãos limitado; forks mais antigos que a profundidade máxima descartados; filas de envio limitadas; pontuação e banimento de pares; limites da API; alertas falsos de gasto duplo impossíveis (exigem duas assinaturas válidas).],
    [Eclipse / Sybil na rede], [Conexões de saída escolhidas pelo próprio nó, múltiplas seeds, limite por IP, gerenciador de endereços persistente; alturas anunciadas por pares tratadas como alegações.],
    [Captura da governança], [Duas câmaras independentes, moedas de voto bloqueadas, quórum, depósitos anti-spam, limites rígidos dos parâmetros, supply e emissão imutáveis.],
    [Roubo de chaves], [Nós não precisam de chaves de usuários; o instalador guarda apenas a carteira de recompensas, cifrada com Argon2id + ChaCha20-Poly1305 (no modo não interativo, a senha fica num arquivo legível só pelo root); serviço isolado por systemd.],
    [Corrupção de dados], [Transações ACID, compromisso de estado verificado a cada bloco e na inicialização.],
  ),
  caption: [Modelo de ameaças e mitigações.],
)

#warnbox[
  Uma rede de prova de trabalho pequena é mais vulnerável a ataques de maioria do que redes
  estabelecidas. A resposta de curto prazo são as salvaguardas acima; a de longo prazo é crescimento —
  cada VPS minerando aumenta o custo de um ataque — e, se aprovada, a finalidade por stake da fase
  híbrida (@sec-roadmap).
]

= Desempenho e escalabilidade <sec-perf>

#include "performance.typ"

= Roadmap de consenso: PoW → Híbrido → PoS <sec-roadmap>

#warnbox[
  *Plano, não implementação.* Esta seção apresenta o plano aprovado para discussão pública em
  `docs/ROADMAP.md`. *Nada dele está implementado na v0.2.* Cada transição é uma atualização de consenso
  e só acontece com aprovação da governança bicameral (detentores *e* mineradores) e com as condições
  objetivas listadas abaixo.
]

A The Coin nasce em prova de trabalho e evolui, em três fases, para prova de participação com
finalidade rápida. A ordem não é por moda: cada mecanismo é usado quando é o mais seguro para o momento
da rede.

#figure(
  table(
    columns: (auto, 1fr, 1fr, 1fr),
    table.header([], [Fase 1 — PoW], [Fase 2 — Híbrido], [Fase 3 — PoS]),
    [Quem produz blocos], [mineradores (CoinHash)], [mineradores (CoinHash)], [validadores com stake, em rodízio],
    [Quem finaliza], [ninguém (probabilístico, reorg máx. 720)], [validadores votam checkpoints a cada 120 blocos], [validadores (BFT) a cada bloco],
    [Tempo até irreversível], [de 1 a 720 blocos (≈ 12 h), conforme o valor], [≤ 2 épocas (≈ 4 h), depois nunca reverte], [≈ 2 blocos],
    [Tempo de bloco], [60 s], [60 s], [10 s (definido na atualização da fase 3)],
    [Recompensa], [100% mineradores], [dividida (começa 90/10, no máximo 50/50)], [100% validadores],
    [Mais cedo possível], [bloco 0], [bloco 1 250 000 (≈ 2,4 anos)], [bloco 3 125 000 (≈ 5,9 anos)],
  ),
  caption: [As três fases do plano de consenso (`docs/ROADMAP.md`).],
)

#figure(
  {
    let w = 13cm
    let maxy = 8.0
    let x(y) = w * y / maxy
    let row = 0.62cm
    let era-end = (1, 2, 3, 4, 5, 6).map(e => e * 625000 * 60 / 31557600)
    box(width: w + 0.4cm, height: 4 * row + 1.6cm, inset: (bottom: 1.1cm))[
      #place(dy: 0pt, rect(width: x(maxy), height: row - 3pt, fill: accent.lighten(55%), stroke: none))
      #place(dx: 4pt, dy: 3pt, text(8pt)[*Fase 1 — PoW* (produção de blocos por mineração)])
      #place(dx: x(era-end.at(1)), dy: row, rect(width: x(maxy - era-end.at(1)), height: row - 3pt, fill: rgb("#fed7aa"), stroke: (paint: orange, dash: "dashed", thickness: 0.6pt)))
      #place(dx: x(era-end.at(1)) + 4pt, dy: row + 3pt, text(8pt)[*Fase 2 — Híbrido* (finalidade PoS), a partir do bloco 1 250 000])
      #place(dx: x(era-end.at(4)), dy: 2 * row, rect(width: x(maxy - era-end.at(4)), height: row - 3pt, fill: rgb("#bfdbfe"), stroke: (paint: rgb("#2563eb"), dash: "dashed", thickness: 0.6pt)))
      #place(dx: x(era-end.at(4)) + 4pt, dy: 2 * row + 3pt, text(8pt)[*Fase 3 — PoS*])
      #place(dx: x(era-end.at(4)) - 4.6cm, dy: 2 * row + 3pt, box(width: 4.4cm, align(right, text(8pt, fill: muted)[a partir do bloco 3 125 000 →])))
      #place(dy: 3 * row + 4pt, line(start: (0pt, 0pt), end: (w, 0pt), stroke: 0.6pt))
      #for (i, e) in era-end.enumerate() {
        place(dx: x(e), dy: -2pt, line(start: (0pt, 0pt), end: (0pt, 3 * row + 8pt), stroke: (paint: luma(170), thickness: 0.4pt, dash: "dotted")))
        place(dx: x(e) - 12pt, dy: 3 * row + 8pt, text(7pt, fill: muted)[#fmtnum(100 * (1 - 1 / calc.pow(2, i + 1)), digits: if i < 2 { 0 } else { 1 })%])
      }
      #for y in range(0, 9) {
        place(dx: x(y) - 2pt, dy: 3 * row + 19pt, text(8pt)[#y])
      }
      #place(dx: w / 2 - 3.2cm, dy: 3 * row + 31pt, text(8pt)[anos após o lançamento · % da oferta emitida ao fim de cada era])
    ]
  },
  kind: image,
  caption: [Alturas *mínimas* das fases (tracejado = só com aprovação e condições cumpridas).],
)

== Por que esta ordem

+ *PoW primeiro — distribuição justa.* No começo ninguém tem moedas. Em PoS puro, quem começa com
  moedas controlaria a rede para sempre. PoW com CoinHash distribui a emissão a quem contribui com
  CPU, inclusive VPS pequenas, sem pré-mineração.
+ *Híbrido depois — segurança extra sem trocar o motor.* Quando ≈ 75% da emissão já foi distribuída
  (fim da era 2), existe base ampla de detentores. Os validadores passam a *finalizar* checkpoints: um
  ataque de 51% de *hashrate* não consegue mais reverter blocos finalizados. O PoW continua produzindo
  blocos, então um erro no PoS não para a rede.
+ *PoS no final — rapidez e eficiência.* Com ≈ 97% da emissão distribuída (fim da era 5), a recompensa
  de bloco já é pequena e a segurança paga pela emissão cairia. A partir daí é mais seguro e barato
  proteger a rede com stake que pode ser punido, com confirmação em segundos — mantendo hardware
  modesto.

== Fase 1 — PoW (atual)

Já implementado na v0.2: CoinHash e LWMA-1 com aquecimento e limite de 2× (@sec-difficulty); recompensa
com cooldown de 25% / 75% (@sec-cooldown); reorganização máxima de 720 blocos, suporte a checkpoints (lista preenchida a partir do lançamento), alertas de
gasto duplo e estimador de confirmações (@sec-confirm); governança com sinalização de mineradores e
propostas de atualização de software — o mecanismo que ativará as próximas fases.

== Fase 2 — Híbrido: PoW + finalidade PoS

*Ativação:* proposta de atualização de software aprovada pela governança *e* todas as condições:
altura ≥ 1 250 000; implementação com ≥ 6 meses de testnet sem falha de segurança e auditoria externa;
stake registrado ≥ 10% da oferta circulante, em ≥ 50 validadores.

- *Stake:* nova ação `Stake { amount, validator_key }`; mínimo de 1 000 TCN (parâmetro de governança).
  Pequenos detentores *delegam* a um validador sem entregar as chaves. Saída com período de
  desbloqueio de 43 200 blocos (30 dias).
- *Épocas* de 120 blocos (≈ 2 h). Ao fim de cada época os validadores assinam (Ed25519) um voto para o
  checkpoint da época. Quando votos que somam ≥ 2/3 do stake justificam dois checkpoints seguidos, o
  primeiro fica *finalizado* — o modelo *Casper FFG* [17].
- *Regra de escolha:* a cadeia deve conter o último checkpoint finalizado; entre as que o contêm, vence a
  de maior trabalho acumulado. Blocos finalizados nunca são revertidos.
- *Recompensa:* começa em 90% mineradores / 10% validadores e move 10 pontos percentuais por era, até
  no máximo 50/50 (se a fase 3 começar na altura mínima, a divisão chega a 70/30). A emissão total e o teto de 50 000 000 TCN não mudam.
- *Punição (slashing):* votar em dois checkpoints conflitantes na mesma época, ou votos que se "cercam",
  queima 10% do stake e remove o validador. Validadores inativos perdem recompensa, sem queima — a rede
  nunca para, porque o PoW continua.
- *Anti-concentração:* o peso de voto de um validador é limitado a 5% do total; stake acima disso não
  aumenta poder nem recompensa.
- *Ataques de longo alcance:* nós novos sincronizam a partir de um checkpoint finalizado recente,
  distribuído no software e no site (*weak subjectivity*).

== Fase 3 — PoS com finalidade rápida

*Ativação:* nova proposta aprovada *e* todas as condições: altura ≥ 3 125 000; ≥ 2 anos de Fase 2 sem
nenhuma falha de finalidade; stake ≥ 33% da oferta circulante, em ≥ 100 validadores independentes;
coeficiente de Nakamoto ≥ 7 (são necessários ≥ 7 validadores para somar 1/3 do stake).

- *Produção:* líderes escolhidos por stake com *VRF* [20] (sorteio verificável e imprevisível), rodízio a
  cada slot de 10 s. O tempo de bloco é uma constante de consenso, não um parâmetro de votação comum: a
  mesma atualização multiplica por 6 todos os intervalos contados em blocos (halving, maturação,
  desbloqueio de recompensas, votação), para que o cronograma de emissão em tempo e o teto não mudem.
- *Finalidade:* BFT no estilo HotStuff/Tendermint [18, 19] — um bloco com votos de ≥ 2/3 do stake é final em
  ≈ 2 blocos. Até 1/3 de validadores maliciosos não reverte nada.
- *PoW removido:* o último bloco PoW vira o checkpoint de gênese da fase. Mineradores migram fazendo
  stake das recompensas acumuladas; o instalador de uma linha passa a configurar um validador de stake
  em vez de um minerador.
- *Recompensa:* a emissão restante e as gorjetas vão para validadores e delegadores; a queima da
  sobretaxa de congestionamento continua.
- *Hardware:* continua modesto (mínimo 1 vCPU / 1 GB; recomendado 2 vCPU / 4 GB / SSD). Um bloco
  totalmente cheio executa em ≈ 1,2 s numa VPS de 2 vCPU; para blocos de 10 s os limites por bloco são
  reduzidos na mesma proporção, e a governança só aumenta limites com medições publicadas.
- *Contratos TCCL* continuam iguais: VM, taxas e depósitos não dependem do mecanismo de consenso.

== O que não muda em nenhuma fase

- O teto de 50 000 000 TCN e o cronograma de emissão.
- Todo o histórico desde o bloco 0 continua público e verificável.
- Endereços, carteiras, contratos e a linguagem TCCL.
- Nenhuma fase é ativada sem votação dos detentores e sinalização da rede.

== Riscos e tratamento

#figure(
  table(
    columns: (auto, 1fr),
    table.header([Risco], [Tratamento]),
    [Bug no novo consenso], [A fase híbrida mantém o PoW produzindo blocos; testnet e auditoria antes da ativação.],
    [Stake concentrado em corretoras], [Limite de 5% de peso por validador, delegação livre, coeficiente de Nakamoto como condição.],
    ["Nada em jogo" / votos duplos], [Slashing de 10% com prova enviada por qualquer nó.],
    [Ataque de longo alcance], [Checkpoints finalizados e sincronização a partir de checkpoint recente.],
    [Mineradores contra a transição], [A ativação exige também sinalização dos mineradores.],
  ),
  caption: [Riscos da transição de consenso.],
)

= Lançamento da rede e roteiro de software <sec-roteiro>

== Plano de lançamento

+ Publicar código, binários com checksum, instalador e este whitepaper.
+ Operar uma *testnet* pública para validação por desenvolvedores.
+ Colocar no ar os dois nós seed (`seed1` e `seed2.the-coin.cloud`) e o site em máquina separada.
+ Fixar o timestamp da gênese da mainnet (13 de setembro de 2026, 00:00 UTC) e iniciar a mineração nos
  nós seed.
+ Abrir o instalador ao público; cada novo nó sincroniza e passa a minerar automaticamente.
+ Após as primeiras semanas, adicionar checkpoints de alturas consolidadas em novas versões.

== Roteiro

- *Entregue na v0.2* — novo modelo de taxas e prioridade, confirmação segura, TCCL (que substitui a VM
  WebAssembly prevista para a v0.3), privacidade por contratos, armazenamento esquema 2, instalação em
  uma linha.
- *Adiado da v0.2* — criptografia do transporte P2P (Noise), sincronização rápida por *snapshot* de
  estado verificado pelo `state_root`, carteira gráfica e web, provas Merkle para clientes leves.
- *Próximas versões* — os itens adiados acima; canais de pagamento sobre HTLC, integração com exchanges e
  ferramentas para comércio (do roteiro da v0.1).
- *Consenso* — Fases 2 e 3 conforme a @sec-roadmap, somente com aprovação da governança.
- *Contínuo* — auditorias externas, programa de recompensa por falhas, diversificação de
  implementações.

= Conclusão

The Coin combina a robustez comprovada da prova de trabalho e de uma política monetária fixa com
escolhas técnicas voltadas a máquinas modestas: uma prova de trabalho memory-hard que valoriza CPUs
comuns, dificuldade que reage em minutos, validação barata com compromisso de estado homomórfico,
armazenamento compacto e um nó que não disputa recursos com o próprio sistema. A versão 0.2 torna a
rede útil em escala: taxas de cerca de 0,00003 TCN por transferência em uso normal, que crescem exponencialmente sob
ataque, confirmação rápida com proteções explícitas contra gasto duplo, contratos inteligentes com regras
de segurança impostas pelo compilador, privacidade opcional com limites claramente declarados e instalação de um validador com
um único comando. O caminho para a finalidade por stake está traçado, com condições objetivas e sempre
sujeito à decisão conjunta de detentores e mineradores. O código é aberto, os padrões são documentados e
qualquer pessoa pode rodar um nó, publicar um contrato ou propor melhorias.

#heading(numbering: none)[Apêndice A — Parâmetros das redes]

#table(
  columns: (auto, auto, auto, auto),
  table.header([Parâmetro], [Mainnet], [Testnet], [Regtest]),
  [Magic P2P], [`TCN1`], [`TCNT`], [`TCNR`],
  [chain_id], [`0x54430001`], [`0x54430002`], [`0x54430003`],
  [Portas P2P / API], [7333 / 7334], [17333 / 17334], [27333 / 27334],
  [Prefixo de endereço], [`tc`], [`tct`], [`tcr`],
  [CoinHash], [16 MiB, t=1], [16 MiB, t=1], [64 KiB, t=1],
  [Alvo da gênese / limite], [$2^256 slash 2^13$ / $2^256 slash 2^8$], [$2^256 slash 2^10$ / $2^256 slash 2^6$], [máximo, sem ajuste],
  [Tempo de bloco / janela LWMA (mín.)], [60 s / 60 (6)], [60 s / 60 (6)], [60 s / —],
  [Recompensa inicial], [40 TCN], [40 TCN], [40 TCN],
  [Halving], [625 000 blocos], [625 000 blocos], [150 blocos],
  [Liberação da recompensa 25% / 75%], [100 / 1 000], [100 / 1 000], [5 / 12],
  [Reorganização máxima], [720], [720], [720],
  [Bloco máximo / combustível (padrão)], [1 MB / 50 M], [1 MB / 50 M], [1 MB / 50 M],
  [`base_fee` / `fee_per_kb` / `fee_per_kfuel`], [1 000 / 10 000 / 1 000], [1 000 / 10 000 / 1 000], [100 / 1 000 / 100],
  [`storage_deposit_per_kb`], [100 000], [100 000], [10 000],
  [Congestionamento], [1× – 1 000×], [1× – 1 000×], [1× – 1 000×],
  [Depósito de proposta], [100 TCN], [10 TCN], [1 TCN],
  [Votação / ativação], [20 160 / 2 880], [1 440 / 720], [20 / 5],
  [Quórum / aprovação / mineradores], [10% / 66,67% / 60%], [5% / 66,67% / 50%], [10% / 66,67% / 50%],
)

#heading(numbering: none)[Apêndice B — Referências]

#set text(9.5pt)
+ S. Nakamoto. _Bitcoin: A Peer-to-Peer Electronic Cash System_, 2008.
+ A. Biryukov, D. Dinu, D. Khovratovich, S. Josefsson. _RFC 9106: Argon2 Memory-Hard Function for Password Hashing and Proof-of-Work Applications_, 2021.
+ J. O'Connor, J.-P. Aumasson, S. Neves, Z. Wilcox-O'Hearn. _BLAKE3: one function, fast everywhere_, 2020.
+ S. Josefsson, I. Liusvaara. _RFC 8032: Edwards-Curve Digital Signature Algorithm (EdDSA)_, 2017.
+ M. Bellare, D. Micciancio. _A New Paradigm for Collision-free Hashing: Incrementality at Reduced Cost_, EUROCRYPT 1997.
+ K. Lewi, W. Kim, I. Maykov, S. Weis. _Securing Update Propagation with Homomorphic Hashing_ (LtHash), 2019.
+ Zawy12. _LWMA difficulty algorithm_, 2018–2019.
+ P. Wuille. _BIP-350: Bech32m format for v1+ witness addresses_, 2020.
+ M. Palatinus et al. _BIP-39: Mnemonic code for generating deterministic keys_, 2013.
+ SatoshiLabs. _SLIP-0010: Universal private key derivation from master private key_, 2016.
+ NEAR Protocol. _Borsh: Binary Object Representation Serializer for Hashing_.
+ V. Buterin, E. Conner, R. Dudley, M. Slipper, I. Norden, A. Bakhta. _EIP-1559: Fee market change for ETH 1.0 chain_, 2019.
+ D. A. Harding, P. Todd. _BIP-125: Opt-in Full Replace-by-Fee Signaling_, 2015.
+ M. Corallo. _BIP-152: Compact Block Relay_, 2016.
+ A. Back. _Ring signature efficiency_ (bLSAG), 2015; S. Noether, A. Mackenzie. _Ring Confidential Transactions_, Ledger 2016.
+ H. de Valence, J. Grigg, G. Tankersley, F. Valsorda, I. Lovecruft, M. Hamburg. _The ristretto255 and decaf448 Groups_, RFC 9496, 2023.
+ V. Buterin, V. Griffith. _Casper the Friendly Finality Gadget_, 2017.
+ M. Yin, D. Malkhi, M. K. Reiter, G. Golan Gueta, I. Abraham. _HotStuff: BFT Consensus with Linearity and Responsiveness_, PODC 2019.
+ E. Buchman, J. Kwon, Z. Milosevic. _The latest gossip on BFT consensus_ (Tendermint), 2018.
+ S. Micali, M. Rabin, S. Vadhan. _Verifiable Random Functions_, FOCS 1999.
+ The Coin. `docs/ROADMAP.md`, `docs/PROTOCOL.md`, `docs/CONTRACTS.md`, `docs/GOVERNANCE.md` e _TCCL Cookbook_ (`docs/tccl/tccl-cookbook.pdf`), repositório `LucasBolla94/thecoin`.
