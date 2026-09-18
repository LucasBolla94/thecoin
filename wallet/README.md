# wallet.the-coin.cloud — carteira no navegador

Carteira da The Coin que roda inteira no navegador: cria a carteira, guarda a
frase de recuperação criptografada no aparelho, assina as transações localmente
e envia para um nó só o que já está assinado. **Nenhuma chave sai do navegador.**

```
wallet/
  index.html            página única
  connect.js            biblioteca que sites e jogos importam para pedir pagamentos
  assets/
    app.js              telas e fluxos
    thecoin.js          protocolo: chaves, endereços, transações, taxas, valores
    api.js              chamadas ao nó (valores lidos como BigInt)
    vault.js            cofre: frase criptografada com senha (PBKDF2 + AES-GCM)
    style.css           visual, igual ao do site
    vendor/crypto.js    bibliotecas auditadas, empacotadas (não editar à mão)
  nginx/                configuração do site
  tests/                testes contra os vetores oficiais da rede
```

## Desenvolvimento

O navegador não precisa de build: os arquivos são servidos como estão. O Node
serve só para empacotar as bibliotecas e rodar os testes.

```bash
cd tools/wallet-build
npm install
npm test            # vetores de chaves, endereços, transações, taxas e valores
npm run build       # regera assets/vendor/crypto.js (depois de mudar uma versão)
npx prettier --write "../../wallet/**/*.{js,html,css}"
```

Os testes conferem o resultado contra os vetores publicados da carteira de
referência (`cargo test -p thecoin-wallet --test vectors`): mesma frase, mesmas
chaves, mesmos endereços, mesma assinatura e mesmo txid, byte a byte. Se um
deles falhar, **não publique**: a carteira estaria gerando transações que a rede
recusa — ou endereços cujas moedas ninguém consegue gastar.

## Publicação

```bash
sudo scripts/deploy-wallet.sh --setup   # primeira vez: nginx + certificado
sudo scripts/deploy-wallet.sh           # atualizações
```

O nó precisa listar `https://wallet.the-coin.cloud` em `[rpc] cors_origins`
(o `testnet-seed1` já lista). A carteira fala direto com a API pública do nó
pelo navegador.

## Segurança

- A frase fica cifrada com AES-256-GCM, com chave derivada da senha por
  PBKDF2-HMAC-SHA-256 (600 000 rodadas, WebCrypto). Só o texto cifrado vai para
  o `localStorage`.
- Ao travar a carteira, as chaves derivadas são apagadas da memória.
- A página só carrega arquivos do próprio domínio (CSP), não entra em iframe e
  não usa CDN.
- A frase nunca é enviada a lugar nenhum, nem para o nó, nem em URLs.
- Valores são sempre `BigInt` em motes: nada de ponto flutuante com dinheiro.

## Pagamentos a partir de um site ou jogo

```html
<script type="module">
  import { connect, pay } from "https://wallet.the-coin.cloud/connect.js";

  const { address } = await connect(); // o usuário aprova uma vez
  const { txid } = await pay({ to: "tc1…", amount: "2.5", memo: "Espada" });
</script>
```

A carteira abre numa janela, o usuário vê quem está pedindo (a origem real do
site, não o que a página diz ser), aprova ou recusa. O site nunca vê a frase nem
as chaves, e nada é enviado sem o clique do usuário.

## O que ainda não tem (v2)

Contratos nativos e TCCL, pools de privacidade, governança, substituição de taxa
(RBF), importar/exportar o arquivo `keystore` da carteira de linha de comando e
histórico em nós podados (hoje depende de um nó arquivo).
