#import "common.typ": *

As medições de CPU abaixo foram feitas com a implementação de referência (compilação `release`)
num VPS OVH de 2 vCPU Haswell, 3,7 GB de RAM e disco SSD de rede — exatamente o tipo de máquina
para a qual a rede foi projetada. As medições de processamento vêm da v0.1 (o caminho de validação de
transferências não mudou de forma relevante); as de disco foram refeitas na v0.2, com recibos e o novo
índice de endereços.

#figure(
  table(
    columns: (1fr, auto, auto),
    table.header([Medição], [Resultado], [Versão]),
    [Transferência simples (sem memo)], [159 bytes], [0.2],
    [Cabeçalho de bloco], [180 bytes], [0.1],
    [CoinHash (16 MiB) por tentativa], [12,0 ms · 83–87 H/s por thread], [0.1],
    [Verificação sem estado + Ed25519 estrito], [≈ 14 400 transações/s por núcleo], [0.1],
    [Aplicar bloco de 790 kB com 5 000 transferências (inclui assinaturas)], [0,39 s (≈ 12 900 tx/s)], [0.1],
    [Atualizar o LtHash para 5 002 registros alterados], [62 ms], [0.1],
    [Processar e gravar bloco vazio (validação + banco)], [0,39 ms], [0.1],
    [Memória residente do nó minerando (1 thread)], [≈ 42 MB], [0.1],
    [Disco por bloco vazio em regime (cabeçalho, índices)], [≈ 765 bytes (≈ 400 MB/ano)], [0.1],
    [Disco por transação, nó arquivo *com* índice de endereços — dados], [≈ 430 bytes], [0.2],
    [#h(1em) espaço alocado pelo redb (antes de `thecoind compact`)], [≈ 792 bytes], [0.2],
    [Disco por transação, nó arquivo *sem* índice de endereços — dados], [≈ 348 bytes], [0.2],
    [#h(1em) espaço alocado pelo redb (antes de `thecoind compact`)], [≈ 633 bytes], [0.2],
  ),
  caption: [Desempenho medido da implementação de referência. Disco: 100 000 transferências para contas novas, em regime (dados de undo já descartados), teste `crates/node/tests/storage_growth.rs`.],
)

== Capacidade de processamento

Com o limite padrão de 1 MB por bloco (`max_block_bytes`) e blocos de 60 s, cabem
$floor(("1 000 000" - 184) \/ 159) = "6 288"$ transferências por bloco — cerca de
*105 transações por segundo* sustentadas na camada base, ou ≈ 9,05 milhões por dia. Um bloco cheio é
validado em menos de meio segundo por um único núcleo modesto, menos de 1% do intervalo entre
blocos: mesmo com a rede no limite, a validação não compete com a mineração nem torna o nó lento. O
limite de bytes é um parâmetro de governança (250 kB a 8 MB); contratos têm ainda o limite de
combustível por bloco (`max_block_fuel`, 50 milhões por padrão, entre 5 e 500 milhões).

== Armazenamento: o que ocupa espaço

- *Estado:* proporcional às contas ativas (≈ 60 bytes por conta) e ao armazenamento dos contratos —
  este último sempre coberto por depósito reembolsável (@sec-tccl). Não cresce com o histórico.
- *Cabeçalhos:* ≈ 765 bytes por bloco com índices, ≈ 400 MB por ano, sempre mantidos, pois provam a
  cadeia de trabalho.
- *Corpos de blocos:* só existem quando há transações e são comprimidos com zstd; nós podados mantêm
  apenas os mais recentes (padrão `prune_keep` = 10 000 blocos, mínimo 1 000).
- *Recibos:* um por transação confirmada (taxa, parte queimada, endereços afetados, eventos),
  comprimidos com zstd. A poda remove os recibos e as entradas do índice de transações junto com o
  corpo do bloco.
- *Índice de endereços:* opcional (`storage.address_index`); na v0.2 cada entrada é só a chave
  (endereço + altura + posição, 32 bytes) e o valor é vazio — o txid é lido do próprio bloco. Nós
  podados desativam o índice automaticamente.
- *Dados de undo:* mantidos apenas para os últimos 736 blocos (720 + margem de 16).

#note[
  *Dados × espaço alocado.* O redb é um banco copy-on-write que pré-aloca páginas para crescer sem
  fragmentar. Por isso o arquivo `chain.redb` ocupa mais do que os dados que contém (≈ 792 contra
  ≈ 430 bytes por transação com índice). O excedente é recuperável a qualquer momento com o nó parado:
  `thecoind compact`. As projeções abaixo usam os dois valores: *compactado* (dados) e *sem compactar*
  (pior caso).
]

== Projeções de disco

Seja $D$ o disco disponível, $b$ os bytes por transação e $H approx "525 960" times "765 B" approx "0,40 GB/ano"$
o custo fixo dos cabeçalhos (valor medido na v0.1, conservador). Então:

$ N_"tx" = D / b quad quad "anos"(T) = D / ("365,25" dot T dot b + H) $

onde $T$ é o número de transações por dia. Estado e dados de undo (algumas centenas de MB para
milhões de contas) não estão incluídos.

#let bpt = (
  ([Arquivo + índice, compactado], 430),
  ([Arquivo + índice, sem compactar], 792),
  ([Arquivo sem índice, compactado], 348),
  ([Arquivo sem índice, sem compactar], 633),
)

#figure(
  table(
    columns: (1fr, auto, auto, auto, auto),
    align: (left, right, right, right, right),
    table.header([Configuração], [bytes/tx], [20 GB], [50 GB], [100 GB]),
    ..bpt.map(((label, b)) => (
      label,
      fmtnum(b),
      ..(20, 50, 100).map(d => [#fmtnum(d * 1e9 / b / 1e6, digits: 1) mi]),
    )).flatten()
  ),
  caption: [Quantas transações cabem no disco ($N_"tx" = D slash b$, em milhões), ignorando cabeçalhos.],
)

#let H = 525960 * 765
#let years(d, t, b) = d * 1e9 / (365.25 * t * b + H)
#let full-day = 6288 * 1440

#figure(
  table(
    columns: (auto, 1fr, auto, auto, auto),
    align: (right, left, right, right, right),
    table.header([Transações/dia], [Configuração], [20 GB], [50 GB], [100 GB]),
    ..(1000, 10000, 100000, 1000000).map(t => (
      bpt.enumerate().map(((i, (label, b))) => (
        if i == 0 { table.cell(rowspan: 4, align: horizon + right)[#fmtnum(t)] },
        label,
        ..(20, 50, 100).map(d => { let y = years(d, t, b); if y < 1 [#fmtnum(y * 365.25) dias] else [#fmtnum(y, digits: 1) anos] }),
      ).filter(x => x != none)).flatten()
    )).flatten(),
    table.cell(colspan: 2)[Blocos sempre cheios (#fmtnum(full-day) tx/dia), arquivo + índice compactado],
    ..(20, 50, 100).map(d => [#fmtnum(years(d, full-day, 430) * 365.25, digits: 0) dias]),
  ),
  caption: [Anos até encher o disco de um nó arquivo, $"anos"(T) = D slash ("365,25" T b + H)$.],
)

A leitura prática: um nó arquivo com 50 GB guarda cerca de 116 milhões de transferências (compactado)
e acompanha uma rede de 10 000 transações por dia por mais de 25 anos. Mesmo a 100 000 transações
por dia — ordem de grandeza de uma rede nacional de pagamentos pequena — 100 GB duram mais de 6
anos. A capacidade máxima da camada base (blocos sempre cheios) consumiria ≈ 3,9 GB por dia num nó
arquivo com índice; esse cenário é tratado pelo multiplicador de congestionamento (taxas sobem
exponencialmente) e pela poda.

*Nó podado.* Um minerador comum não precisa do histórico: com poda, o disco fica limitado a
cabeçalhos (≈ 0,4 GB/ano) + estado + os últimos `prune_keep` corpos de bloco. No pior caso (blocos de
1 MB sempre cheios) são ≈ 10 GB para 10 000 blocos, ou ≈ 1 GB com o mínimo de 1 000; com uso real,
muito menos. Um disco de 5–10 GB atende um nó podado por muitos anos; os nós arquivo (seeds e os que
alimentam o site) guardam o histórico completo, que permanece público e verificável.

== Mempool persistente e sincronização

Ao desligar, o nó grava as transações pendentes em `mempool.dat` (formato versionado, escrita atômica
via arquivo temporário) e, ao iniciar, revalida cada uma contra o estado atual antes de readmiti-la:
uma reinicialização ou atualização não faz transações de usuários desaparecerem.

Um novo nó baixa blocos em lotes de 500, verifica a prova de trabalho em paralelo em todos os núcleos
e aplica cada bloco em ordem, conferindo a raiz de estado. O custo dominante é a prova de trabalho
(12 ms por bloco por núcleo): um ano de cadeia exige ≈ 1,75 hora de CPU, ou ≈ 53 minutos numa máquina
de 2 núcleos. A sincronização por *snapshot* de estado, prevista originalmente para a 0.2, foi
adiada (@sec-roteiro).
