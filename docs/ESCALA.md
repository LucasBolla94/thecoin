# Estudo: blocos órfãos, armazenamento e confirmação rápida

Objetivo da moeda: **minerável por computadores comuns**, **um nó completo roda numa VPS de
2 vCPU e 4 GB**, e **pagamentos rápidos o bastante para jogos e aplicações web**.

Este documento mede o problema, compara as soluções conhecidas, escolhe um caminho e registra o que
foi implementado. Todos os números vieram de medições nesta máquina (Intel Haswell, 2 vCPU,
3,8 GB) ou de contas simples a partir delas.

---

## 1. Ponto de partida medido

| Medida | Valor | Onde |
|---|---|---|
| Tamanho de um bloco vazio | 184 bytes | `EMPTY_BLOCK_BYTES` |
| Disco real por bloco vazio (com banco e índices) | ≈ 436 bytes | `storage_growth` |
| Disco real por transação (com índice de endereços) | ≈ 430 bytes | `storage_growth` |
| Disco real por transação (sem índice) | ≈ 348 bytes | `storage_growth` |
| Verificação de um bloco (RandomX, modo leve) | ≈ 30 ms | `pow_bench` |
| Execução de um bloco cheio de contratos (pior caso) | ≈ 1,2 s | `fuel_bench` |
| Propagação de um bloco (blocos compactos, ~10 kB) | 0,3 a 1 s na internet | literatura + tamanho medido |

---

## 2. Blocos órfãos

### 2.1 O problema

Dois mineradores acham um bloco quase ao mesmo tempo; só um entra na cadeia. O outro vira **órfão**
e seu minerador não recebe nada. A probabilidade é `1 − e^(−p/T)`, com `p` = tempo de propagação e
`T` = tempo de bloco:

| Tempo de bloco | p = 0,3 s | p = 1 s | p = 2 s |
|---|---|---|---|
| 60 s (hoje) | 0,5 % | 1,7 % | 3,3 % |
| 15 s | 2,0 % | 6,4 % | 12,5 % |
| 10 s | 3,0 % | 9,5 % | 18,1 % |

O dano real não é a perda média, é a **assimetria**: uma fazenda com 30 % do poder da rede sabe dos
próprios blocos na hora e só perde nos 70 % restantes; o minerador caseiro perde sempre. Com blocos
rápidos e sem tratamento, a mineração escorre para os grandes — o contrário do objetivo do projeto.

### 2.2 Soluções conhecidas

| Abordagem | Como funciona | Prós | Contras |
|---|---|---|---|
| **Blocos lentos** (Bitcoin, 10 min) | evita colisões | simples | confirmação lenta demais para jogos |
| **Tios / uncles** (Ethereum PoW) | o bloco perdedor entra na cadeia como "tio" e recebe parte da recompensa | remove a assimetria; testado por anos com blocos de 13 s | recompensa precisa sair de algum lugar |
| **GHOSTDAG** (Kaspa) | blocos viram um grafo; nada é descartado | 1 bloco/s, nenhum trabalho perdido | reescrita completa do consenso, mais disco e banda |
| **Bitcoin-NG** | um líder eleito por PoW emite microblocos | latência baixa | líder pode censurar; forks de microbloco |

### 2.3 Decisão

**Tios, com recompensa tirada do próprio subsídio do bloco.**

- Até 2 tios por bloco, com no máximo 6 blocos de idade e nunca repetidos.
- O tio precisa ter prova de trabalho válida e o pai dele precisa ser um ancestral do bloco que o inclui.
- **Recompensa:** cada tio recebe `subsídio × (7 − idade) / 24` — 25 % do subsídio quando é do bloco
  anterior, 4 % quando tem 6 blocos — **descontado do subsídio do bloco**. A emissão total por bloco
  nunca aumenta, então o teto de 100 000 000 TCN e o cronograma de emissão não mudam.
- **Trabalho acumulado:** o trabalho dos tios entra no peso da cadeia, o que também encarece
  reorganizações.

- **Por que o minerador inclui tios:** o trabalho deles entra no peso da cadeia, então um bloco com
  tios tem mais chance de vencer a corrida do que um sem.

Efeito: com 6 % de órfãos, o minerador caseiro recupera até 25 % do que perderia, e o peso extra
protege o próprio bloco de quem o incluiu.

---

## 3. Confirmação rápida ("igual a Solana")

### 3.1 O que dá e o que não dá

Solana confirma em ~0,4 s com um líder único e servidores de 128 GB de RAM e rede de 1 Gb/s. Numa
rede feita para VPS pequenas isso não se copia. Mas o que o usuário sente — "meu pagamento valeu" —
pode ser resolvido em três camadas.

### 3.2 Camada 1 — blocos de 15 s

Quatro vezes mais rápido que hoje. Com tios (§2), sem o custo de centralização. Disco: veja §4.

### 3.3 Camada 2 — finalidade assinada pelos mineradores (novo)

Hoje, para ter certeza, espera-se N confirmações. A proposta:

1. Cada nó minerador tem uma **chave de assinatura** e publica a chave pública no bloco que minera.
2. O conjunto de assinantes é quem minerou os **últimos 200 blocos**, com peso igual ao número de
   blocos que minerou.
3. Cada minerador assina o **bloco anterior ao topo** (já confirmado por um bloco de trabalho, então
   todos concordam sobre ele) e espalha a assinatura. Cada nó assina **uma única vez por altura**:
   depois de uma reorganização ele simplesmente não vota de novo naquela altura.
4. Quando as assinaturas somam **≥ 2/3 do peso**, e a janela tem pelo menos **4 mineradores
   diferentes**, o bloco vira **final**: os nós recusam qualquer reorganização que o remova.

Resultado: pagamento **irreversível em torno de 30 segundos** (dois blocos), em vez de 5 a 100
confirmações.
Custo para uma VPS: ~200 verificações Ed25519 por bloco, menos de 5 ms.

Propriedades:
- **Segurança:** quem controla 2/3 do poder de mineração já controlaria a rede de qualquer forma.
  Contra quem tem menos que isso, a finalidade elimina reorganizações baratas.
- **Liveness:** se os mineradores sumirem, nada trava — a cadeia continua pelo peso de trabalho, só
  sem o carimbo de finalidade.
- **Equívocos:** assinar dois blocos da mesma altura é prova pública de má-fé; a chave é ignorada
  pelos nós e o minerador perde o direito de assinar.
- **Redes pequenas:** com menos de 4 mineradores distintos na janela não existe finalidade — é o que
  impede um minerador solitário de "finalizar" a própria cadeia e recusar as dos outros.
- Inspiração: ChainLocks (Dash), aqui sem precisar de masternodes nem de stake.

### 3.4 Camada 3 — canais de sessão para jogos (TCCL)

Para um jogo, 15 s ainda é muito. A resposta certa é não tocar a rede a cada jogada:

1. Jogador e jogo abrem um canal com depósito, num contrato TCCL.
2. Cada jogada é um estado assinado pelos dois, trocado **fora da rede** (latência de milissegundos).
3. No fim, um dos dois registra o último estado assinado; o contrato paga conforme o resultado.
4. Se alguém sumir ou tentar registrar um estado velho, existe uma janela de disputa em que o outro
   apresenta o estado mais recente.

Isso é o que dá "sensação de instantâneo" de verdade, sem taxa por jogada e sem carregar a rede.

---

## 4. Armazenamento

### 4.1 Projeções (com os números medidos)

Blocos vazios, por ano:

| Tempo de bloco | Blocos/ano | Disco/ano |
|---|---|---|
| 60 s | 525 600 | 0,23 GB |
| 15 s | 2 102 400 | 0,92 GB |
| 10 s | 3 153 600 | 1,37 GB |

Transações, por ano:

| Uso | Transações/ano | Com índice | Sem índice |
|---|---|---|---|
| 0,1 tx/s | 3,2 M | 1,4 GB | 1,1 GB |
| 1 tx/s | 31,5 M | 13,6 GB | 11,0 GB |
| 10 tx/s | 315 M | 136 GB | 110 GB |

**Conclusão importante:** o tempo de bloco quase não pesa (0,9 GB/ano); quem enche o disco é o volume
de transações. Então blocos de 15 s são baratos, e o esforço de armazenamento deve ir para as
transações.

### 4.2 Medidas adotadas

1. **Poda ligada por padrão** em nós normais (implementado): guarda uma semana de blocos
   (40 320 blocos de 15 s) e o estado completo. Um nó comum fica na casa de poucos GB para sempre.
   Nós de arquivo, como exploradores, instalam com `--archive` e guardam tudo.
2. **Sincronização por estado verificado (próxima etapa).** A raiz de estado no cabeçalho usa LtHash, que é
   *homomórfica*: dá para baixar o estado inteiro de um parceiro, recalcular a raiz e comparar com o
   cabeçalho. Um nó novo entra na rede em minutos, sem baixar o histórico, e **sem confiar em ninguém**.
3. **Índice de endereços opcional:** economiza 19 % e só serve a exploradores e carteiras.
4. **Recibos e índices podados** junto com os blocos antigos (já implementado).
5. **Canais de sessão (§3.4)** tiram do disco justamente o que mais cresceria: as jogadas.

---

## 5. Efeito sobre a mineração em máquinas comuns

- **RandomX** (já implementado) mantém CPU e GPU parelhas.
- **Tios** removem a vantagem das fazendas com blocos rápidos.
- **Finalidade assinada** não usa stake: quem minera assina, então não favorece quem tem mais moedas.
- **Modo leve de verificação** (256 MiB) mantém o nó rodando numa VPS pequena; minerar em modo
  rápido (2 GiB) é opcional e vale principalmente para desktops.

---

## 6. Ordem de implementação

| Etapa | Estado |
|---|---|
| 1. RandomX no lugar do Argon2id | **pronto** |
| 2. Blocos de 15 s com emissão recalibrada (20 TCN por bloco, eras de ≈ 1,19 ano, teto de 100 000 000 TCN) | **pronto** |
| 3. Tios: cabeçalho, validação, recompensa e peso | **pronto** |
| 4. Finalidade assinada pelos mineradores | **pronto** |
| 5. Poda ligada por padrão (uma semana de blocos) | **pronto** |
| 6. Sincronização por estado verificado (LtHash) | próxima |
| 7. TCCL v2 e canais de sessão para jogos | em andamento |

---

## 7. O que foi descartado, e por quê

- **GHOSTDAG:** resolve órfãos melhor que tios, mas exige reescrever o consenso, o armazenamento e o
  explorador. Fica registrado como caminho futuro se a rede precisar de mais de 1 bloco a cada 10 s.
- **Prova de participação (PoS) agora:** entregaria finalidade rápida, mas o poder iria para quem tem
  mais moedas, e hoje quase ninguém tem — fere o objetivo de distribuição justa. Continua no
  roteiro (`docs/ROADMAP.md`) como fase futura.
- **Blocos de 10 s ou menos:** o ganho de latência é pequeno perto da finalidade assinada (§3.3) e o
  custo de banda e órfãos cresce rápido.
