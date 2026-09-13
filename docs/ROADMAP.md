# Roadmap de consenso: PoW → Híbrido PoW/PoS → PoS

> Status: **plano aprovado para discussão pública** (não implementado na 0.2).
> Cada transição é uma atualização de consenso e só acontece com aprovação da
> governança bicameral (detentores **e** mineradores), conforme `docs/GOVERNANCE.md`.

A The Coin nasce em prova de trabalho (PoW) e evolui, em três fases, para prova de
participação (PoS) com finalidade rápida. A ordem não é por moda: cada mecanismo é
usado quando é o mais seguro para o momento da rede.

| | Fase 1 — PoW | Fase 2 — Híbrido | Fase 3 — PoS |
|---|---|---|---|
| Quem produz blocos | mineradores (CoinHash) | mineradores (CoinHash) | validadores com stake, em rodízio |
| Quem finaliza | ninguém (probabilístico, reorg máx. 720) | validadores votam checkpoints a cada 120 blocos | validadores (BFT) a cada bloco |
| Tempo até irreversível | de 1 bloco (valores pequenos) a 720 blocos (~12 h), conforme o valor | ≤ 2 épocas (~4 h), depois nunca reverte | ~2 blocos |
| Tempo de bloco | 60 s | 60 s | 10 s (definido na atualização da fase 3) |
| Recompensa | 100% mineradores | dividida (começa 90/10, até no máximo 50/50) | 100% validadores |
| Mais cedo possível | bloco 0 | bloco 1 250 000 (~2,4 anos) | bloco 3 125 000 (~5,9 anos) |

## Por que esta ordem

1. **PoW primeiro — distribuição justa.** No começo ninguém tem moedas. Em PoS puro
   quem começa com moedas (fundadores) controlaria a rede para sempre. PoW com CoinHash
   (Argon2id, 16 MiB, resistente a ASIC/GPU) distribui a emissão a quem contribui
   com CPU, inclusive VPS pequenas. Não há pré-mineração.
2. **Híbrido depois — segurança extra sem trocar o motor.** Quando ~75% da emissão
   já foi distribuída (fim da era 2), existe base ampla de detentores. Os validadores
   passam a **finalizar** checkpoints: um ataque de 51% de hashrate não consegue mais
   reverter blocos finalizados. PoW continua produzindo blocos, então um erro no
   PoS não para a rede.
3. **PoS no final — rapidez e eficiência.** Com ~97% da emissão distribuída (fim da
   era 5), a recompensa de bloco já é pequena; a segurança do PoW (paga pela emissão)
   cairia. A partir daí é mais seguro e barato proteger a rede com stake que pode ser
   punido (slashing), com confirmação em segundos, no estilo Solana — mas mantendo o
   requisito de hardware baixo (mínimo 1 vCPU / 1 GB; recomendado 2 vCPU / 4 GB), porque a
   rede é feita para VPS simples.

## Fase 1 — PoW (atual)

Já implementado na 0.2:

- CoinHash (Argon2id 16 MiB), LWMA-1 por bloco com aquecimento e limite de ±2× por
  bloco: dificuldade justa (reage em minutos à entrada/saída de máquinas) e sem
  vantagem para quem manipula timestamps.
- Recompensa com **cooldown**: 25% liberados após 100 blocos, o restante após 1 000
  blocos. Um minerador que tenta reorganizar a cadeia arrisca recompensas ainda
  bloqueadas.
- Reorganização máxima de 720 blocos, suporte a checkpoints (a lista é preenchida a partir
  do lançamento da rede principal), alertas de gasto duplo e estimador de confirmações
  (`thecoin-wallet confirmations <valor>`).
- Governança com sinalização de mineradores e propostas de atualização de software —
  o mecanismo que ativará as próximas fases.

## Fase 2 — Híbrido PoW + finalidade PoS

**Ativação**: proposta `software_upgrade` aprovada pela governança **e** todas as condições:

- altura ≥ 1 250 000;
- a implementação passou ≥ 6 meses na testnet sem falha de segurança e por auditoria externa;
- stake registrado para a fase ≥ 10% da oferta circulante, em ≥ 50 validadores.

**Como funciona**

- **Stake**: nova ação `Stake { amount, validator_key }`; stake mínimo de 1 000 TCN
  (parâmetro de governança). Pequenos detentores **delegam** para um validador sem
  entregar as chaves. Saída com período de desbloqueio de 43 200 blocos (30 dias).
- **Épocas** de 120 blocos (~2 h). Ao fim de cada época os validadores assinam um voto
  (Ed25519) para o checkpoint da época. Quando votos que somam ≥ 2/3 do stake
  justificam dois checkpoints seguidos, o primeiro fica **finalizado** (modelo Casper FFG).
- **Regra de escolha**: a cadeia deve conter o último checkpoint finalizado; entre as
  que contêm, vence a de maior trabalho acumulado. Blocos finalizados nunca são revertidos.
- **Recompensa**: começa 90% mineradores / 10% validadores e move 10 pontos percentuais
  por era, até no máximo 50/50 (se a fase 3 começar na altura mínima, a divisão chega a
  70/30). A emissão total e o teto de 50 000 000 TCN não mudam.
- **Punição (slashing)**: votar em dois checkpoints conflitantes na mesma época, ou
  votos que se "cercam", queima 10% do stake e remove o validador. Validadores
  inativos perdem recompensa (sem queima) — a rede nunca para porque o PoW continua.
- **Anti-concentração**: o peso de voto de um validador é limitado a 5% do total;
  stake acima disso não aumenta poder nem recompensa.
- **Ataques de longo alcance**: nós novos sincronizam a partir de um checkpoint
  finalizado recente (distribuído no software e no site) — *weak subjectivity*.

## Fase 3 — PoS com finalidade rápida

**Ativação**: nova proposta aprovada pela governança **e** todas as condições:

- altura ≥ 3 125 000;
- ≥ 2 anos de Fase 2 sem nenhuma falha de finalidade;
- stake ≥ 33% da oferta circulante, em ≥ 100 validadores independentes;
- coeficiente de Nakamoto ≥ 7 (são necessários ≥ 7 validadores para somar 1/3 do stake).

**Como funciona**

- **Produção**: líderes escolhidos por stake com VRF (sorteio verificável e
  imprevisível), rodízio a cada slot de 10 s. O tempo de bloco é uma constante de consenso
  (não um parâmetro de votação comum): a mesma atualização multiplica por 6 todos os
  intervalos contados em blocos (halving, maturação, desbloqueio de recompensas, votação),
  para que o cronograma de emissão *em tempo* e o teto não mudem.
- **Finalidade**: BFT no estilo HotStuff/Tendermint — um bloco com votos de ≥ 2/3 do
  stake é final em ~2 blocos. Até 1/3 de validadores maliciosos não reverte nada.
- **PoW removido**: o último bloco PoW vira o checkpoint de gênese da fase. Mineradores
  migram fazendo stake das recompensas acumuladas; o instalador de uma linha
  (`install.sh`) passa a configurar um validador em vez de um minerador.
- **Recompensa**: a emissão restante e as gorjetas vão para validadores e delegadores.
  A queima da sobretaxa de congestionamento continua.
- **Hardware**: continua modesto (mínimo 1 vCPU / 1 GB; recomendado 2 vCPU / 4 GB / SSD).
  Um bloco totalmente cheio executa em ~1,2 s numa VPS de 2 vCPU (preços de combustível
  calibrados por benchmark); para blocos de 10 s os limites por bloco são reduzidos na
  mesma proporção, e a governança só aumenta limites com medições publicadas.
- **Contratos TCCL** continuam iguais: a VM, as taxas e os depósitos de armazenamento
  não dependem do mecanismo de consenso.

## O que NÃO muda em nenhuma fase

- Teto de 50 000 000 TCN e o cronograma de emissão.
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
