# Roteiro da troca: Braniac em Python -> B.R.A.N.I (Rust)

Este roteiro NAO foi executado. Cada passo diz como conferir e como desfazer. Faca na ordem e um passo por vez.
Rota para ler no navegador: `http://127.0.0.1:8865/api/troca/roteiro`.

## Leia antes (a PONTE temporaria: loja e Link)

- **Elo com a loja e Link com o Aliado/Parceiro passam por uma PONTE em Python** (`ferramentas/ponte/ponte.py`). O B.R.A.N.I nao tem WebRTC, ntfy,
  AES-GCM nem Ed25519; a ponte usa so a parte de REDE do codigo antigo como biblioteca (`rede`, `par_webrtc`, `rede_jarvis`, `link/no`,
  `link/confianca`, `link/identidade`, `link/avisos`, `link/fila`) e entrega tudo ao B.R.A.N.I, que e quem pensa e responde. Sem Chrome,
  sem atendimento, sem servidor 8765, sem rotinas, sem voz; uns 60 MB de memoria. O Python antigo nao e alterado (o estado da ponte fica em `dados/`).
- As chaves `loja` e `link` agora ligam (`POST /api/troca {"loja": true}`): ligada = o B.R.A.N.I sobe a ponte e aceita o trafego daquele elo;
  desligada = as rotas respondem "desligado" (503) e a ponte nao responde ao par. As duas desligadas = ponte parada.
- **So um dono do elo por vez.** A ponte RECUSA subir (sai com codigo 3 e o motivo aparece em `GET /api/ponte` -> `ultimo_erro`) se o servidor
  antigo estiver no ar (porta 8765 ou processo `braniac.server`) ou se ja houver outra ponte (`dados/ponte.lock`). Por isso a ordem abaixo e: parar o Python primeiro.
- O que a loja faz: so conversa (sem ferramentas, sem dados pessoais, sem decidir dinheiro; o texto dela e dado, nunca ordem). Se o Jarvis prometer
  avisar o dono, a promessa vira aviso de verdade (barramento + WhatsApp do dono pelo despachante, conforme as chaves). Historico em `dados/par_conversas.jsonl`.
  Nomes (assistente, par, loja, agente da loja) vem de `pessoal.json`, chave `"elo"`; sem ela valem os padroes.
- O que o Link faz: mensagem do par vira caixa + recado ao Jarvis + comandos de aprovar/reprovar/postar cidade (`VIDEO APROVADO ...`); consulta de
  cidades e receber a pasta do predio do par passam pelas regras `link_*`; o dono fala ao par por `POST /api/par/link/falar` (sai pela ponte).
  **Ainda NAO existe no B.R.A.N.I:** pedido de render/producao completa e `FONTES APROVADO` (a ponte responde ao par "ainda nao disponivel", nada entra na fila),
  fila de jobs (`/job/...` da 404), o painel do Parceiro pela fila, a tela do NOSSO predio mostrada ao par, o envio ao Drive e o visor 8774.
- O tunel da fila (cloudflared) continua nascendo DESLIGADO: so sobe com o botao do dono (`dados/link/tunel_ligado`) e com o Link ligado.
  O endereco fica so em `dados/link/fila_url.txt`; nada e anunciado ao par.
- O portal remoto (porta 8780) ja funciona no B.R.A.N.I com a chave `portal`, com o MESMO login e os mesmos dispositivos confiaveis. O portal barra `/api/ponte`.
- **Subir sozinho com o Windows:** `ferramentas\instalar-inicio.ps1` (cria UM atalho em Inicializar para o lancador escondido
  `ferramentas\lancar-brani.vbs`; nao instala mais nada) e `remover-inicio.ps1` desfaz. O dono roda quando quiser. O `ligar-brani.bat` continua como estava.
  Por que o PowerShell escondido nao subia: o E: pode nao estar pronto no logon, o filho fica preso ao processo que o criou e um segundo `braniac.exe`
  so panica ("porta ocupada") e briga pelo `servidor.log`. O lancador espera o E:, nao sobe se ja houver um `braniac.exe` e roda com a janela em modo 0.
- Nao mexa no Tailscale: o Funnel `:443` ja aponta para a 8780 e o `:8443` (porteiro do painel da esteira) e outro programa.

## Passo 0 - antes de tudo (nada muda)

1. `GET /api/troca` : veja quais chaves estao ligadas (`whatsapp_so_dono` ligada e normal durante o teste).
2. Copie a pasta `E:\braniac-rs\dados` para um backup (ex.: `E:\braniac-rs\dados-backup-AAAA-MM-DD`).
   Desfazer: apague a copia.

## Passo 1 - trazer o historico de novo

`POST /api/importar-antigo` (corpo vazio). Pode rodar quantas vezes quiser: so entra o que falta. Devolve so contagens.
Desfazer: nao precisa (so acrescenta).

## Passo 2 - copiar as identidades (login do portal etc.)

`POST /api/troca/copiar` corpo `{}`. Copia `portal.json`, `par.json`, `rede.json`, `link/pares.json` e `link/identidade/*` para
`dados/`. Sem `forcar`, o que ja existe fica. NAO mostra conteudo. **Depois de parar o Python (passo 4) rode de novo com
`{"forcar": true}`** para pegar os dispositivos e sessoes mais novos do portal.
Conferir: `GET /api/troca/rede` -> `portal.login_copiado: true`.
Desfazer: apague os arquivos copiados em `dados/`.

## Passo 3 - teste "so o dono" (ja em andamento)

`POST /api/troca {"whatsapp_so_dono": true}`: so o chat do dono recebe resposta de verdade; o resto fica em sombra.
Conferir: mande uma mensagem e veja a resposta (texto e audio). Enquanto o Python ainda esta ligado, os DOIS respondem ao dono: so
teste assim por pouco tempo.
Desfazer: `POST /api/troca {"whatsapp_so_dono": false}`.

## Passo 4 - parar o Braniac Python e tudo que o religa (nesta ordem)

Ordem importa: primeiro o que religa, depois o que e religado.

1. **Vigia que religa tudo (a cada 5 s):** `pythonw -m braniac.sentinela`.
   - Impeca de subir no login: renomeie o atalho
     `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\Jarvis - vigia de voz.lnk` para `...lnk.off`.
   - Pare o processo: PowerShell
     `Get-CimInstance Win32_Process | ? { $_.CommandLine -match 'braniac\.sentinela' } | % { Stop-Process -Id $_.ProcessId }`
2. **Servidor** (porta 8765; o robo do Telegram e o portal 8780 do Python moram dentro dele):
   `... -match 'braniac\.server'`. (Se existir `braniac.ligar_tudo` ou `start-chrome-debug.bat` em execucao, pare tambem.)
3. **Janelas flutuantes:** `braniac.orbe` e `braniac.alma_orbe` (pythonw).
4. **Chrome do Braniac** (porta 9222, perfil `C:\Users\usuario\ChromeAgente`): feche so depois de conferir que o B.R.A.N.I usa a aba do
   WhatsApp na VM logada. Os logins das IAs (ChatGPT, Gemini, Grok, DeepSeek) vivem ali: refaca-os na VM se precisar.
5. **Nao pare** o `cloudflared` da porta 8766 sem antes descobrir o que ele serve (nao foi identificado); nem o Tailscale, o Ollama, o
   Radmin, o porteiro `E:\painel-esteira\porteiro.py` e os `passo_esteira.py`/`vigia_*` da esteira.
6. Nao existem tarefas agendadas, chaves `Run` nem servicos do Windows do Braniac (conferido em 21/11/2026).

Conferir: `GET http://127.0.0.1:8765/` nao responde; a porta 8780 esta livre; nenhum `braniac.` aparece em
`Get-CimInstance Win32_Process | ? CommandLine -match 'braniac\.'`. Depois rode o passo 2 com `forcar`.
Desfazer: no PowerShell, na pasta `C:\Users\usuario\Projetos\Agente`, `python -m braniac.ligar_tudo --sem-interface`; devolva o nome do
atalho (tire o `.off`). Antes de religar o Python, desligue as chaves do passo 5 (senao os dois respondem juntos).

## Passo 5 - ligar as chaves, uma por vez, nesta ordem

**Trava do antigo (obrigatoria):** ligar `whatsapp`, `telegram`, `airbnb`, `voz` ou `portal` (por `POST /api/troca` ou pelos botoes do painel, `POST /api/atendimento-auto` e
`POST /api/airbnb-auto`) e RECUSADO (409, "O Braniac antigo ainda esta ligado e atendendo. Desligue ele primeiro...") enquanto a porta 8765 do antigo responder, porque os dois
responderiam as mesmas pessoas. Passam: `whatsapp_so_dono` (teste do dono) e desligar qualquer chave. Com o antigo parado (passo 4) a trava some sozinha. So se o roteiro
exigir ligar com o antigo no ar, mande `"forcar": true` no corpo (ex.: `POST /api/troca {"whatsapp": true, "forcar": true}`); nao use isso para "testar".
Os botoes do painel (Sistemas -> Canais) mostram a chave real: "WhatsApp" = `whatsapp`; "Airbnb" = `airbnb`, que no B.R.A.N.I so LE a caixa e AVISA o dono e a Mae (nunca responde hospede nem aceita reserva).

Use `POST /api/troca {"<chave>": true}`; o B.R.A.N.I nao liga nada sozinho.

1. `whatsapp` (todos os chats). Conferir: `GET /api/whatsapp` com `estado: "logado"`, mande uma mensagem de teste e veja UMA resposta. Se vier duas,
   o Python ainda esta ligado: desligue a chave agora.
2. `telegram`. Conferir: `GET /api/telegram`; mande "oi" ao robo. O robo do Python tem de estar parado (mesmo token).
3. `airbnb`. Conferir: `GET /api/troca` -> `airbnb_estado`; nada e aceito sem OK do dono (regra existente).
4. `portal`. Conferir: `GET /api/troca/rede` -> `portal.porta_aberta: true`; de fora, abra o endereco do Funnel: aparece o login; senha errada
   nao entra; aparelho novo pede o codigo de 6 digitos (chega pelo WhatsApp do dono, com `whatsapp` ligada).
   O portal recusa `POST /api/troca*` e `/api/importar-antigo` vindos de fora (so no PC).
5. `voz` (microfone e alto-falante). So depois de `braniac.orbe`/escuta do Python parados.
6. `loja` e `link` (PONTE): **antes, o Python tem de estar parado (passo 4)**, e rode o passo 2 com `{"forcar": true}` para a ponte ter as identidades mais novas
   (`par.json`, `rede.json`, `link/pares.json`, `link/identidade`, `link/ice.json`). Ligue UMA por vez: `POST /api/troca {"loja": true}`; espere uns 10 s;
   `GET /api/ponte` deve mostrar `rodando: true` e `elos.loja.ligado_na_ponte: true` (`conectado` so fica true quando o outro lado responde). Depois `{"link": true}`.
   Conferir a loja: peca a ela uma mensagem de teste; deve aparecer em `GET /api/par/conversa`. Conferir o Link: o Aliado manda "oi"; aparece em `GET /api/par/link`.
   Se `rodando` ficar false e `ultimo_erro` disser que o Braniac antigo esta no ar: o Python ainda esta ligado; desligue a chave e volte ao passo 4.
   Log da ponte (so eventos e contagens): `dados/ponte.log`.

Desfazer cada um: `POST /api/troca {"<chave>": false}` (a porta 8780 fecha em ate 2 s com `portal`; a ponte para em ate ~5 s quando `loja` e `link` estao desligadas).

## Voltar atras (tudo)

1. Desligue as chaves na ordem inversa: `link`, `loja` (a ponte para), `voz`, `portal`, `airbnb`, `telegram`, `whatsapp`, `whatsapp_so_dono`.
2. Religue o Python (passo 4, "Desfazer"). 3. Confira que `GET http://127.0.0.1:8765/` responde e o WhatsApp responde uma vez so.

## O que ainda depende do Python (NAO apague a pasta `C:\Users\usuario\Projetos\Agente`)

- A PONTE (`ferramentas/ponte`) importa a rede de `par_webrtc`, `rede`, `rede_jarvis` e `link/*` do Python (e precisa de `aiortc` e `cryptography` no `py -3`); sem a pasta `C:\Users\usuario\Projetos\Agente`, loja e Link nao ligam.
- Transcricao de audio: `dados/ouvir.json` chama `python` e o modelo em `braniac\modelos\whisper-small` (e `E:\braniac-rs\ferramentas\ouvir_whisper.py`).
- Segredos: `ligar-brani.bat` aponta `BRANIAC_ENV` para `braniac\.env` do Python; o historico importado le a pasta `data` dele.
- Esteira de videos: `esteira_orq.rs` le `braniac\data\parceiro` e a raiz `C:\Users\usuario\Projetos\Agente`; cortes usam o
  gameplay em `braniac\data\empresa_cortes\fontes`.
- Fora do Braniac, mas ligados a ele: `C:\Parceiro\PACOTE-PRODUCAO-COMPLETA`, `C:\Parceiro\guias-estados`, `E:\braniac-link\fila`,
  `E:\painel-esteira` (Funnel :8443).
- Ollama (modelos em `E:\ollama-models`), cache em `E:\cache-ia`, Ubuntu WSL em `E:\wsl`: sao do computador, nao do Python.
