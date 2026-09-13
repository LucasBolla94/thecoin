Todas as medições abaixo foram feitas com a implementação de referência v0.1 (compilação
`release`) num VPS OVH de 2 vCPU Haswell, 3,7 GB de RAM e disco SSD de rede — exatamente o tipo de
máquina para a qual a rede foi projetada.

#figure(
  table(
    columns: (1fr, auto),
    table.header([Medição], [Resultado]),
    [Transferência simples (sem memo)], [158 bytes],
    [Cabeçalho de bloco], [180 bytes],
    [CoinHash (16 MiB) por tentativa], [12,0 ms · 83–87 H/s por thread],
    [Verificação sem estado + Ed25519 estrito], [≈ 14 400 transações/s por núcleo],
    [Aplicar bloco de 790 kB com 5 000 transferências (inclui assinaturas)], [0,39 s (≈ 12 900 tx/s)],
    [Atualizar o LtHash para 5 002 registros alterados], [62 ms],
    [Processar e gravar bloco vazio (validação + banco)], [0,39 ms],
    [Montar, validar e gravar bloco com 500 transferências], [≈ 104 ms],
    [Memória residente do nó minerando (1 thread)], [≈ 42 MB],
    [Disco por bloco vazio em regime (cabeçalho, índices)], [≈ 765 bytes (≈ 400 MB/ano)],
    [Disco por transação, nó arquivo com índice de endereços (300 mil tx medidas)], [≈ 886 bytes],
    [Disco por transação, nó arquivo sem índice de endereços], [≈ 437 bytes],
  ),
  caption: [Desempenho medido da implementação de referência.],
)

== Capacidade

Com o limite padrão de 1 MB por bloco e blocos de 60 s, cabem cerca de 6 300 transferências por
minuto — aproximadamente *105 transações por segundo* de capacidade sustentada na camada base,
várias vezes a do Bitcoin. Um bloco cheio é validado em menos de meio segundo por um único núcleo
modesto, ou seja, menos de 1% do intervalo entre blocos: mesmo com a rede no limite, a validação não
compete com a mineração nem torna o nó lento. O limite de tamanho é um parâmetro de governança
(250 kB a 8 MB) e pode acompanhar a demanda real e a capacidade dos nós.

== Crescimento dos dados

- *Estado:* proporcional às contas ativas (≈ 60 bytes por conta), não ao histórico.
- *Cabeçalhos:* ≈ 400 MB por ano, sempre mantidos, pois provam a cadeia de trabalho.
- *Corpos de blocos:* só existem quando há transações (blocos vazios não ocupam espaço) e são
  comprimidos com zstd; nós podados mantêm apenas os mais recentes.
- *Dados de undo:* mantidos apenas para os últimos 736 blocos.
- *Índices:* o índice de endereços é opcional (`storage.address_index`); nós que não servem
  exploradores podem desativá-lo.

Um nó minerador comum pode rodar podado e sem índice de endereços por muitos anos num disco de
5–10 GB; os nós arquivo (como os seeds e os que alimentam o site) guardam o histórico completo, que
permanece público e verificável.

== Sincronização

Um novo nó baixa blocos em lotes de 500, verifica a prova de trabalho em paralelo em todos os
núcleos e aplica cada bloco em ordem, conferindo a raiz de estado. O custo dominante é a prova de
trabalho (12 ms por bloco por núcleo): um ano de cadeia exige ≈ 1,75 hora de CPU, ou ≈ 53 minutos
numa máquina de 2 núcleos. A versão 0.2 adicionará sincronização por *snapshot* de estado,
verificável pelo `state_root` dos cabeçalhos, para que novos nós fiquem prontos em minutos.
