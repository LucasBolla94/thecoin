# the-coin.cloud — site oficial

Site estático (HTML + CSS + JavaScript puro, sem build e sem CDNs externas) da
rede **The Coin**: página inicial com estatísticas ao vivo, guia de instalação,
explorer de blocos (contratos TCCL, taxas, alertas de gasto duplo),
governança, guia de contratos inteligentes TCCL e hub de desenvolvedores.

```
website/
├── index.html          # landing + estatísticas ao vivo
├── download.html       # instalação do nó e carteira
├── validator.html      # guia do validador para iniciantes (instalação em uma linha)
├── explorer.html       # explorer (SPA: #/block/…, #/tx/…, #/address/… (contas e contratos TCCL), #/contract/…, #/mempool)
├── governance.html     # propostas, apuração bicameral, parâmetros (#/proposal/<id>)
├── tccl.html           # guia de contratos inteligentes TCCL
├── docs.html           # hub de desenvolvedores + resumo da API
├── assets/
│   ├── config.js       # lista de bases da API (failover)
│   ├── api.js          # fetch com failover, formatação exata de valores (BigInt)
│   ├── style.css       # tema claro/escuro automático, responsivo
│   ├── home.js  explorer.js  governance.js
└── nginx/
    ├── the-coin.cloud.conf        # servidor HTTPS + proxy da API
    └── snippets/thecoin-proxy.conf
```

## Redesign (September 2026)

The public website defaults to English, including explorer/governance labels,
number formatting, and API outage messages. No build is required.

- `whitepaper.html` introduces the paper in English and links to the unchanged
  original Portuguese PDF, with separate read and download actions.
- `assets/site.js` controls the mobile menu, motion toggle, and four-step
  transaction demonstration. Illustrations are explicitly labeled, never live data.
- Motion honors `prefers-reduced-motion`; the journey also supports manual steps.
- Inter is served locally; its OFL license is in `assets/inter-LICENSE.txt`.
- API errors show an unavailable state, with retry actions in the explorer and
  governance. The homepage clears stale statistics when a refresh fails.
- Static assets use a version query on entry pages to avoid the previous CSS/JS
  remaining cached after deployment. Increment it when publishing future changes.

## Arquitetura

```
navegador ──HTTPS──▶ nginx (máquina do site)
                        ├── /            arquivos estáticos
                        ├── /install.sh  instalador do nó
                        ├── /releases/   binários + SHA256SUMS
                        ├── /whitepaper/ PDFs (v0.2 atual, v0.1 anterior)
                        ├── /tccl/       cookbook PDF + examples/*.tccl
                        └── /api/        ──HTTP──▶ seed1.the-coin.cloud:7334
                                                    seed2.the-coin.cloud:7334  (failover)
```

O navegador só fala com `the-coin.cloud` (mesma origem, sem CORS). O nginx
repassa `/api/` para a API REST dos nós seed, com rate limit, cache de 5 s e
failover automático entre os nós.

## Deploy

### 1. Nós seed (cada uma das máquinas que rodam `thecoind`)

No `thecoind.toml` de cada seed, exponha a API na rede:

```toml
[rpc]
listen = "0.0.0.0:7334"
cors_origins = ["https://the-coin.cloud"]
```

E **libere a porta 7334 somente para o IP da máquina do site** (a porta P2P
7333 continua aberta para todos):

```bash
sudo ufw allow 7333/tcp
sudo ufw allow from <IP_DO_SITE> to any port 7334 proto tcp
sudo ufw enable
sudo systemctl restart thecoind
```

Crie os registros DNS `seed1.the-coin.cloud` e `seed2.the-coin.cloud` apontando
para os IPs dos nós (os mesmos nomes são usados como seeds P2P pelo software).

### 2. Máquina do site

```bash
sudo apt install nginx certbot python3-certbot-nginx

# arquivos
sudo mkdir -p /var/www/the-coin.cloud /var/www/certbot
sudo cp -r website/*.html website/assets /var/www/the-coin.cloud/
sudo mkdir -p /var/www/the-coin.cloud/{releases,whitepaper,tccl/examples}
sudo cp installer/install.sh installer/uninstall.sh installer/thecoin /var/www/the-coin.cloud/   # instalador
sudo cp docs/whitepaper/*.pdf /var/www/the-coin.cloud/whitepaper/                              # v0.2 e anteriores
sudo cp docs/tccl/tccl-cookbook.pdf /var/www/the-coin.cloud/tccl/
sudo cp crates/tccl/examples/*.tccl /var/www/the-coin.cloud/tccl/examples/
# binários de release: releases/latest/thecoin-<target>.tar.gz + .sha256 (layout usado pelo install.sh)
# atalho: no repositório, `scripts/package.sh <target>` e depois `scripts/publish-site.sh`
# montam tudo em dist/site/, pronto para `rsync -av dist/site/ servidor:/var/www/the-coin.cloud/`

# nginx
sudo cp website/nginx/snippets/thecoin-proxy.conf /etc/nginx/snippets/
sudo cp website/nginx/the-coin.cloud.conf /etc/nginx/sites-available/the-coin.cloud
sudo mkdir -p /var/cache/nginx/thecoin_api

# certificado (antes de ativar o bloco 443, ou use --standalone)
sudo certbot certonly --standalone -d the-coin.cloud -d www.the-coin.cloud

sudo ln -s /etc/nginx/sites-available/the-coin.cloud /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
```

Se os nós tiverem outros nomes/IPs, edite o bloco `upstream thecoin_nodes`.
Adicionar mais nós à lista aumenta a disponibilidade da API.

### 3. Conferir

```bash
curl -s https://the-coin.cloud/api/v1/status | head
```

Abra `https://the-coin.cloud/explorer.html` — os blocos, as taxas e os alertas
devem aparecer e atualizar a cada 15 s. Busque o endereço de um contrato TCCL
para ver as funções e chamar views (POST `/api/v1/program/<endereço>/view`).

## Configuração da API no front-end

`assets/config.js` define `window.THECOIN_API`, uma lista de bases tentadas em
ordem (a primeira que responder passa a ser a preferida). O padrão `/api/v1`
usa o proxy do nginx. Para usar nós públicos diretamente, adicione URLs HTTPS
completas — nesse caso esses nós precisam permitir a origem do site em
`rpc.cors_origins`.

Para testar localmente contra um nó qualquer, sem editar arquivos:

```bash
cd website && python3 -m http.server 8000
# abra: http://localhost:8000/explorer.html?api=http://127.0.0.1:7334/api/v1
# (fica salvo no navegador; use ?api=reset para voltar ao padrão)
```

Um nó regtest para desenvolvimento:

```bash
thecoin-wallet --network regtest create
thecoind --network regtest --miner-address $(thecoin-wallet --network regtest address)
# API em http://127.0.0.1:27334/api/v1
```

## Convenções

- Valores vêm da API em **motes** (inteiros; 1 TCN = 100.000.000 motes) e são
  formatados com `BigInt` — nunca com ponto flutuante.
- Todo conteúdo vindo da rede (memos, títulos de propostas, URLs) é escapado
  antes de ir para o HTML; links externos só são criados para `http(s)://`.
- A CSP do nginx permite apenas recursos da própria origem (estilos inline são
  liberados porque as barras de progresso usam `style="width:…"`). Não há
  scripts inline nem handlers `onclick` em atributos.
- Formulários que chamam a API (views de contratos, estimador de confirmações)
  escrevem os resultados com `textContent`.
