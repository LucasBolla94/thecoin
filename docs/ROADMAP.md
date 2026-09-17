# Roadmap de consenso: PoW → Híbrido PoW/PoS → PoS

> Status: a **fase 1 está em produção** e já inclui RandomX, blocos de 15 s, tios e
> finalidade assinada pelos mineradores (veja o estudo [ESCALA.md](ESCALA.md)).
> As fases 2 e 3 continuam sendo **plano para discussão pública**, não implementado.
> Cada transição é uma atualização de consenso e só acontece com aprovação da
> governança bicameral (detentores **e** mineradores), conforme `docs/GOVERNANCE.md`.

A The Coin nasce em prova de trabalho (PoW) e evolui, em três fases, para prova de
participação (PoS) com finalidade rápida. A ordem não é por moda: cada mecanismo é
usado quando é o mais seguro para o momento da rede.

| | Fase 1 — PoW | Fase 2 — Híbrido | Fase 3 — PoS |
|---|---|---|---|
| Quem produz blocos | mineradores (CoinHash/RandomX) | mineradores (CoinHash/RandomX) | validadores com stake, em rodízio |
| Quem finaliza | os próprios mineradores (2/3 da janela de 200 blocos) | validadores votam checkpoints a cada época | validadores (BFT) a cada bloco |
| Tempo até irreversível | ~30 s quando há finalidade assinada; sem ela, de 1 bloco a 2 880 blocos (~12 h) conforme o valor | ≤ 2 épocas (~4 h), depois nunca reverte | ~2 blocos |
| Tempo de bloco | 15 s | 15 s | 10 s (definido na atualização da fase 3) |
| Recompensa | 100% mineradores (parte vai para os tios) | dividida (começa 90/10, até no máximo 50/50) | 100% validadores |
| Mais cedo possível | bloco 0 | bloco 5 000 000 (~2,4 anos) | bloco 12 500 000 (~5,9 anos) |

As alturas vêm direto da emissão (`crates/core/src/emission.rs`): cada era tem
`halving_interval = 2 500 000` blocos e, a 15 s por bloco, dura ≈ 434 dias
(≈ 1,19 ano). As eras duram o mesmo tempo de antes dos blocos de 15 s — mudou a
contagem em blocos —, mas a recompensa por minuto dobrou (80 TCN), e com ela o teto
(100 000 000 TCN).

## Por que esta ordem

1. **PoW primeiro — distribuição justa.** No começo ninguém tem moedas. Em PoS puro
   quem começa com moedas (fundadores) controlaria a rede para sempre. PoW com CoinHash
   (**RandomX**, com chave que roda a cada época de 2 048 blocos) distribui a emissão a
   quem contribui com CPU, inclusive VPS pequenas: o RandomX executa um programa
   aleatório feito das operações em que a CPU é boa, então uma placa de vídeo não
   ganha quase nada. Não há pré-mineração.
2. **Híbrido depois — segurança extra sem trocar o motor.** Quando 75% da emissão
   já foi distribuída (fim da era 2, bloco 5 000 000), existe base ampla de detentores.
   Os validadores passam a **finalizar** checkpoints: um ataque de 51% de hashrate não
   consegue mais reverter blocos finalizados. PoW continua produzindo blocos, então um
   erro no PoS não para a rede.
3. **PoS no final — rapidez e eficiência.** Com ~97% da emissão distribuída (fim da
   era 5, bloco 12 500 000), a recompensa de bloco já é pequena; a segurança do PoW
   (paga pela emissão) cairia. A partir daí é mais seguro e barato proteger a rede com
   stake que pode ser punido (slashing), com confirmação em segundos, no estilo Solana
   — mas mantendo o requisito de hardware baixo (mínimo 1 vCPU / 1 GB de RAM;
   recomendado 2 vCPU / 2 GB), porque a rede é feita para VPS simples.

## Fase 1 — PoW (atual)

Já implementado:

- **CoinHash = RandomX** com chave por época
  (`key = tagged_hash("randomx-key", chain_id ‖ época)`, época = altura / 2 048):
  verificar usa o modo leve (cache de 256 MiB compartilhado, ≈ 30 ms por bloco) e
  minerar pode usar o modo rápido (dataset de 2 GiB) quando há memória sobrando
  (`[mining] mode = auto|fast|light`). GPU ≈ CPU, que é o que mantém a mineração
  distribuída.
- **Blocos de 15 s** com eras de mesma duração (≈ 1,19 ano) e teto de 100 000 000 TCN:
  20 TCN por bloco (80 TCN por minuto), halving a cada 2 500 000 blocos.
- **Tios**: um bloco que perde a corrida por milissegundos entra na cadeia seguinte
  (até 2 por bloco, no máximo 6 blocos de idade), seu minerador recebe
  `subsídio × (7 − idade)/24` — descontado do subsídio, sem mexer no teto — e o
  trabalho dele conta no peso da cadeia. É o que impede que blocos rápidos empurrem
  a mineração para as fazendas ([ESCALA.md §2](ESCALA.md)).
- **Finalidade assinada pelos mineradores**: quem minerou os últimos 200 blocos
  assina o bloco anterior ao topo; com 2/3 do peso da janela e ao menos 4 mineradores
  distintos, o bloco fica **final** e nenhuma reorganização o remove
  ([ESCALA.md §3.3](ESCALA.md)). Pagamento irreversível em torno de 30 segundos.
- LWMA-1 por bloco com aquecimento e limite de ±2× por bloco: dificuldade justa
  (reage em minutos à entrada/saída de máquinas) e sem vantagem para quem manipula
  timestamps.
- Recompensa com **cooldown**: 25% liberados após 400 blocos (~100 min), o restante
  após 4 000 blocos (~16,7 h). Um minerador que tenta reorganizar a cadeia arrisca
  recompensas ainda bloqueadas.
- Reorganização máxima de 2 880 blocos (~12 h), suporte a checkpoints (a lista é
  preenchida a partir do lançamento da rede principal), alertas de gasto duplo e
  estimador de confirmações (`thecoin-wallet confirmations <valor>`).
- **Poda ligada por padrão** (uma semana de corpos de bloco, 40 320 blocos); nós de
  arquivo instalam com `--archive`.
- Governança com sinalização de mineradores e propostas de atualização de software —
  o mecanismo que ativará as próximas fases.

Próximos passos ainda dentro da fase 1 ([ESCALA.md §6](ESCALA.md)):

- **sincronização por estado verificado (LtHash)**: entrar na rede em minutos, sem
  baixar o histórico e sem confiar em ninguém;
- **canais de sessão em TCCL** para jogos: jogadas assinadas fora da rede, com
  liquidação em um contrato — a única forma de dar resposta em milissegundos sem
  centralizar a rede.

## Fase 2 — Híbrido PoW + finalidade PoS

**Ativação**: proposta `software_upgrade` aprovada pela governança **e** todas as condições:

- altura ≥ 5 000 000 (fim da era 2, ≈ 2,4 anos de rede a 15 s por bloco);
- a implementação passou ≥ 6 meses na testnet sem falha de segurança e por auditoria externa;
- stake registrado para a fase ≥ 10% da oferta circulante, em ≥ 50 validadores.

**Como funciona**

- **Stake**: nova ação `Stake { amount, validator_key }`; stake mínimo de 1 000 TCN
  (parâmetro de governança). Pequenos detentores **delegam** para um validador sem
  entregar as chaves. Saída com período de desbloqueio de 172 800 blocos (30 dias).
- **Épocas** de 480 blocos (~2 h). Ao fim de cada época os validadores assinam um voto
  (Ed25519) para o checkpoint da época. Quando votos que somam ≥ 2/3 do stake
  justificam dois checkpoints seguidos, o primeiro fica **finalizado** (modelo Casper FFG).
  Isso **não substitui** a finalidade assinada pelos mineradores da fase 1: ela continua
  valendo bloco a bloco; a camada PoS acrescenta um carimbo econômico por época.
- **Regra de escolha**: a cadeia deve conter o último checkpoint finalizado; entre as
  que contêm, vence a de maior trabalho acumulado. Blocos finalizados nunca são revertidos.
- **Recompensa**: começa 90% mineradores / 10% validadores e move 10 pontos percentuais
  por era, até no máximo 50/50 (se a fase 3 começar na altura mínima, a divisão chega a
  70/30). A emissão total e o teto de 100 000 000 TCN não mudam.
- **Punição (slashing)**: votar em dois checkpoints conflitantes na mesma época, ou
  votos que se "cercam", queima 10% do stake e remove o validador. Validadores
  inativos perdem recompensa (sem queima) — a rede nunca para porque o PoW continua.
- **Anti-concentração**: o peso de voto de um validador é limitado a 5% do total;
  stake acima disso não aumenta poder nem recompensa.
- **Ataques de longo alcance**: nós novos sincronizam a partir de um checkpoint
  finalizado recente (distribuído no software e no site) — *weak subjectivity*.

## Fase 3 — PoS com finalidade rápida

**Ativação**: nova proposta aprovada pela governança **e** todas as condições:

- altura ≥ 12 500 000 (fim da era 5, ≈ 5,9 anos de rede a 15 s por bloco);
- ≥ 2 anos de Fase 2 sem nenhuma falha de finalidade;
- stake ≥ 33% da oferta circulante, em ≥ 100 validadores independentes;
- coeficiente de Nakamoto ≥ 7 (são necessários ≥ 7 validadores para somar 1/3 do stake).

**Como funciona**

- **Produção**: líderes escolhidos por stake com VRF (sorteio verificável e
  imprevisível), rodízio a cada slot de 10 s. O tempo de bloco é uma constante de consenso
  (não um parâmetro de votação comum): saindo de 15 s para 10 s, a mesma atualização
  multiplica por 1,5 todos os intervalos contados em blocos (halving, maturação,
  desbloqueio de recompensas, votação, reorganização máxima), para que o cronograma de
  emissão *em tempo* e o teto não mudem.
- **Finalidade**: BFT no estilo HotStuff/Tendermint — um bloco com votos de ≥ 2/3 do
  stake é final em ~2 blocos. Até 1/3 de validadores maliciosos não reverte nada.
- **PoW removido**: o último bloco PoW vira o checkpoint de gênese da fase. Mineradores
  migram fazendo stake das recompensas acumuladas; o instalador de uma linha
  (`install.sh`) passa a configurar um validador em vez de um minerador.
- **Recompensa**: a emissão restante e as gorjetas vão para validadores e delegadores.
  A queima da sobretaxa de congestionamento continua.
- **Hardware**: continua modesto (mínimo 1 vCPU / 1 GB de RAM; recomendado 2 vCPU /
  2 GB / SSD). Um bloco totalmente cheio executa em ~1,2 s numa VPS de 2 vCPU (preços
  de combustível calibrados por benchmark); para blocos de 10 s os limites por bloco são
  reduzidos na mesma proporção, e a governança só aumenta limites com medições
  publicadas. Sem PoW, o cache de 256 MiB do RandomX deixa de ser necessário.
- **Contratos TCCL** continuam iguais: a VM, as taxas e os depósitos de armazenamento
  não dependem do mecanismo de consenso.

## O que NÃO muda em nenhuma fase

- Teto de 100 000 000 TCN e o cronograma de emissão.
- Todo o histórico desde o bloco 0 continua público e verificável.
- Endereços, carteiras, contratos e a linguagem TCCL.
- Nenhuma fase é ativada sem votação dos detentores e sinalização da rede.

## Riscos e como são tratados

| Risco | Tratamento |
|---|---|
| Bug no novo consenso | Fase híbrida mantém PoW produzindo blocos; testnet + auditoria antes |
| Stake concentrado em corretoras | limite de 5% de peso por validador, delegação livre, coeficiente de Nakamoto como condição |
| "Nada em jogo" / votos duplos | slashing de 10% com prova enviada por qualquer nó |
| Ataque de longo alcance | checkpoints finalizados + sincronização a partir de checkpoint recente |
| Mineradores contra a transição | a ativação exige também sinalização de mineradores |
