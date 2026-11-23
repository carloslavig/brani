# Braniac em Rust

Reconstrução do Braniac em Rust, em paralelo ao Braniac atual (Python, `%USERPROFILE%\Projetos\Agente\braniac`, porta 8765). O atual não é tocado nem substituído.

## Rodar

    cargo run --release

Para usar as chaves do Braniac em Python sem copiar o arquivo, ligue com `BRANIAC_ENV` apontando para o `.env` dele (so leitura) e `BRANIAC_DADOS` para a pasta de dados.

Abre em http://127.0.0.1:8865 a tela de progresso (quantos módulos do Braniac atual já funcionam aqui).

## Regra que resolve "instalei em outro PC e perdi função"

- **Código e `padrao/`**: iguais em todo PC. Toda função mora aqui.
- **`dados/`** (ou a pasta em `BRANIAC_DADOS`): tudo que é pessoal (contas, contatos, conversas, regras de família). Nunca entra no código nem no Git.
- Função que hoje depende de nome/regra pessoal é portada como função geral + configuração em `dados/`.

## Paridade

`paridade.json` lista os 293 módulos do Braniac atual com estado `falta`, `em andamento` ou `portado`. Ao portar um módulo, troque o estado. `py -3 ferramentas/inventario.py` relê o Braniac atual e mantém os estados.

## Ordem combinada

1. Núcleo: servidor, módulos, tela de paridade (feito).
2. VM (feito o básico, tela `/vm`): distro WSL `BraniacVM` em `E:raniac-vm`, Chromium na porta 9333, tela remota na 6090. Agentes nas abas (feito): `cdp.rs` controla uma aba por WebSocket e `abas.rs` da a cada agente a SUA aba (navegar, ler, pesquisar). Ferramentas `pesquisar_web` e `vm_navegador` (navegar, ler, mostrar). Texto de pagina sai com aviso de dado de terceiros; o que se le de aba fica marcado como local; consulta com dado sensivel nao e pesquisada. Clicar e digitar (feito): acoes `elementos`, `clicar` e `digitar` (clique e tecla de verdade pelo navegador). `travas_pagina.rs` recusa site de banco/pagamento, botao de pagar/comprar/doar/apagar conta/entrar e campo de segredo; texto sensivel nunca e digitado. Rotas diretas em `/api/vm/abas/{elementos,clicar,digitar}`. Conselho de IAs (feito com IAs de chave): `conselho.rs`, tela `/conselho`, rota `POST /api/conselho`, ferramenta `conselho_de_ias`; ate 4 IAs online (uma por familia, cada uma com um papel) opinam ao mesmo tempo, membro que falha cede a cadeira a uma reserva, e sai a sintese. So o tema vai; tema sensivel cancela. Falta: os membros por aba logada na VM (ChatGPT, Gemini, Grok, DeepSeek), que dependem do login da pessoa; entram trocando a funcao `consultar`. Falta tambem e trava de destinatario para sites de mensagem (vem com o modulo do WhatsApp). Limite conhecido: depois de ler uma aba o turno fica so na IA local pequena, que as vezes diz que fez sem chamar a ferramenta; melhora quando o Claude entrar como IA de confianca.
3. Tela de código (feito o básico, tela `/codigo`): mostra a pasta em `BRANIAC_CODIGO` ou `dados/projetos`.
4. Base (feito o nucleo): `store` (historico, descanso, uso), `provedores` (OpenAI e Ollama), `roteador` (cadeia na ordem, trava de texto sensivel) e `banco` (fatos e eventos em duas camadas). Rotas: `POST /api/chat`, `GET /api/ias`, `GET /api/banco?q=`, `POST /api/banco/fato`. Agente com ferramentas (feito): `POST /api/chat` usa `roteador.responder_agente` com as ferramentas de `ferramentas.rs` (`consultar_banco`, `lembrar`, `status_braniac`, `vm_navegador`); travas em `travas.rs` e `texto_ferramenta.rs`. O que AGE so roda com origem `pc`. Turno que tocou dado local fica marcado `local/` no historico e nao volta para IA online. Cada modulo portado ganha a sua ferramenta ali (o Braniac atual tem ~55). Falta: pedido enxuto no erro 413, disputa em paralelo, ordenacao por latencia, bancos de assunto e barramento. Chaves (feito): `segredos.rs` le `dados/.env` no formato `NOME=dpapi:<base64>` (DPAPI do Windows; chave colada em texto puro e criptografada sozinha); `BRANIAC_ENV` aponta para o .env de outro programa, so leitura. Claude (feito): `claude_cli.rs` chama `claude -p` como ultima IA da cadeia, com teto `claude_max_por_dia`; com ferramentas ele pede por texto e o laco executa. E lento (30 s por chamada; 2 min num turno com ferramenta). Rota `POST /api/ia` pergunta a uma IA pelo nome. Agenda e rotinas (feito o nucleo, tela `/agenda`): `agenda.rs` (mesmo formato do `agenda.json` do Braniac em Python: datas em portugues, repeticao, horarios livres, lembretes), `compasso.rs` (rotinas com o protocolo pedido, checagem, execucao; nascem pausadas), `tempo.rs` (calendario e hora local sem crate), `texto_pt.rs` e `avisos.rs`. Um relogio de 1 minuto no servidor chama `compasso::tick`. Ferramentas `agenda` e `compasso_rotinas`; rotas `GET/POST /api/agenda`, `GET/POST /api/rotinas` (criar manda a rotina em `rotina`), `GET /api/avisos`. Quem pode receber rotina fica em `dados/pessoal.json` (`contatos`: apelido -> nome no chat). Limite: os avisos so aparecem na tela (`dados/avisos.jsonl`); saem de verdade quando o modulo de mensagens for portado (trocar o corpo de `avisos::registrar`). Falta: frase da agenda no bom dia, acompanhar os outros agentes (barramento), audio e a acao `verificar_youtube`. Agentes e barramento (tela `/agentes`): `barramento.rs` (recados entre agentes em `dados/agentes.db`, mesmo formato do Python; so entrega, nunca executa; nome desconhecido e recusado) e `agentes.rs` (um agente por assunto: o codigo traz so Forja, Ada e Compasso; os da pessoa ficam em `dados/agentes.json`, chave `agentes`, com id, nome, agente, descricao, instrucao e palavras). O roteador ganhou o campo `contexto`: a instrucao do agente do assunto entra no prompt e a consulta fica registrada no barramento (so o nome do assunto). Ferramenta `listar_agentes`; rota `GET /api/agentes`. Fatos por assunto (feito): `fatos.rs` guarda cada fato no banco do assunto (`dados/assuntos/<id>.db`, mesmo formato do Python) com validade ("so hoje" vence as 5h, "esta semana" 7 dias, "temporariamente" 3 dias); a ferramenta `lembrar` e `POST /api/banco/fato` espelham o fato no assunto; o prompt leva ate 5 fatos validos do assunto (IA online so recebe os publicos); o curador roda a cada 30 min no relogio (vence, refaz a ficha, e contradicao achada pelo `contradiz.rs` vira aviso na tela). Nada e apagado, so marcado como vencido. Falta: assunto novo proposto pela IA local, trazer os fatos antigos do banco principal, Tela dos fatos (feito): em /agentes, tocar no agente lista os fatos dele (GET /api/agentes/fatos?id=), validos primeiro e vencidos riscados; fato regravado que muda de assunto sai do assunto antigo (motivo movido). Delegacao (feito o nucleo): `delegacao.rs` + `consulta.rs`. Pergunta de consulta (nao pedido de acao) vai ao agente do assunto; agente LOCAL (campo `local` em `dados/agentes.json`; sem o campo = local) responde com os proprios dados, SEM nenhuma IA, e o turno fica marcado `local/` no historico; agente nao local deixa a IA compor com os fatos do prompt. O Compasso responde com a agenda. No barramento vai so o assunto e um resumo, nunca a pergunta. Avisos do relogio passam pelo barramento (Compasso -> Jarvis). Falta: blocos proprios dos agentes cujos modulos ainda nao foram portados (financeiro, vaquinha, Airbnb...), perguntas de opiniao e a espera maxima por agente.
5. Canais (WhatsApp, Telegram, voz) e depois os andares.

## Só algumas funções usam a VM

O Braniac Rust roda no Windows. A VM é uma ferramenta dele (navegador e, depois, execução de código). Recriar a VM em outro PC:

    wsl --install Debian --name BraniacVM --location <pasta> --no-launch

e depois instalar `chromium xvfb x11vnc novnc websockify openbox` e copiar `padrao/ligar.sh` para `/opt/braniac/ligar.sh`.

## Trabalho com o NexoDev

Cópia de trabalho em `braniac/data/empresa_dev/projetos/braniac-rs` (com `paridade.json` resumido: os textos pessoais não vão a IA online). Um arquivo por ticket e entrega curta: a resposta vem cortada perto de 8 KB (uns 150 a 200 linhas), entao arquivo maior que isso o Claude escreve. Chamar so `empresa_dev.desenvolver(t)`: o ciclo revisao/QA do NexoDev entra em laco com projeto que ele nao enxerga. O Claude revisa, corrige e integra aqui.

Bom dia diario (feito o nucleo): `bomdia.rs` + `para_fala.rs`. Uma vez por dia, a partir das 6h, o relogio entrega a fala do bom dia (por enquanto na tela de avisos): o que foi feito (`dados/relatorio.md`, uma linha por tarefa; `POST /api/relatorio` com `texto` anota; depois de entregue vai para `relatorio_historico.md` e recomeca), a agenda de hoje (`agenda::resumo_falado`) e o espaco livre nos discos. `GET /api/bomdia` mostra a previa. O nome da pessoa vem de `dados/pessoal.json` (`nome`). Contradicao achada pelo curador entra no relatorio. Falta: audio, e-mails, reuniao dos agentes, seguranca do PC e o cuidado diario da familia (dependem de modulos nao portados).

Voz (feito o nucleo): `voz.rs` + `wav.rs`. `POST /api/voz` com `texto` devolve um WAV (cabecalho `X-Voz` diz o motor). Configuracao em `dados/voz.json` (`motor`: `gemini` ou `local`; `voz`; `estilo`). Sem o arquivo fala a voz do proprio Windows (SAPI pelo PowerShell): nada sai do PC. Com `gemini`, tenta 3 modelos de voz com as chaves `GEMINI_TTS_KEY*`/`GEMINI_API_KEY*`, descansa o modelo sem cota por 15 min, recusa audio longo demais e cai na voz local se falhar. Texto sensivel nunca vai a nuvem. Antes de falar, `voz::limpar` tira links, markdown, emojis e marcadores. Na tela `/agenda`, cada aviso (e o bom dia) tem o botao Ouvir. Limite: fala os primeiros 1500 caracteres. Voz por agente (feito): `agentes` em `dados/voz.json` (chave = nome do agente em minusculas e sem acento; `voz`, `estilo`, `genero`); o que falta vem da voz geral; `POST /api/voz` aceita `agente`; em /agentes cada agente tem o botao Ouvir a voz. `genero` F pede voz local feminina (depende das vozes instaladas no Windows). Falta: reservas Edge/Piper/Kokoro, ajustes de tom e velocidade, ouvir (Whisper), converter para OGG e mandar o audio por mensagem.

Ouvir e conversa por voz (feito o nucleo): `ouvir.rs` + tela `/conversa` (`conversa.html`). `POST /api/ouvir` recebe o audio cru (webm, ogg, wav; ate 20 MB) e devolve `texto`. O Braniac nao embute reconhecedor de fala: chama o transcritor LOCAL de `dados/ouvir.json` (`comando`: lista com o programa e os argumentos; `{audio}` vira o caminho do arquivo; a saida e o texto em UTF-8). Sem o arquivo, ouvir fica desligado. Ponte pronta: `ferramentas/ouvir_whisper.py` (faster-whisper no processador). Uma transcricao por vez, espera maxima de 3 min, o audio e apagado depois. Na tela `/conversa`: escrever ou tocar em Falar (grava pelo navegador), e cada resposta tem o botao Ouvir. Limite: a ponte carrega o modelo a cada audio (uns 10 s por frase). Falta: transcritor que fica carregado, palavra de chamada (escuta continua) e audio vindo de mensagens.

Verificador de respostas (nao inventar): toda resposta de IA (Jarvis e agentes passam pelo mesmo `roteador.rs`) e vigiada antes de sair. 1) Regra no prompt (`padrao/provedores.json`): fato so se estiver na conversa, no BANCO, nos fatos do agente ou em resultado de ferramenta; nao sabe = procura com ferramenta e, se nao achar, diz que nao sabe; pedido ambiguo = uma pergunta curta. 2) Numeros (`travas::numeros_inventados`, sem IA): numero de 2+ digitos que nao esta em nenhuma fonte e barrado SEMPRE, com ou sem ferramenta; excecao so para conta pedida pela pessoa (`travas::eh_conta`). 3) Revisor (`Roteador::revisar` + `parecer.rs`): uma segunda IA, diferente da que respondeu, confere a resposta contra as fontes da camada dela e devolve JSON com o trecho sem fonte; o trecho so vale se existir de verdade na resposta. Resposta de IA local so e revista por IA local. 4) Pegou algo: o modelo refaz UMA vez com o defeito dito (pode usar ferramenta para confirmar). Insistiu: a frase com numero sem fonte sai e a pessoa e avisada; o trecho do revisor fica com o aviso "Nao consegui confirmar este trecho". 5) Revisor fora do ar nao prende a resposta. 6) Tudo que foi pego fica em `dados/verificacao.jsonl`. Agente local (delegacao) responde so com os proprios dados, sem IA. Limites: nome proprio inventado depende do revisor (nao ha checagem fixa); a resposta refeita nao passa de novo pelo revisor; cada resposta custa uma chamada a mais.

## WhatsApp (1a parte, 18/11/2026)

`src/whatsapp.rs` + tela `/whatsapp`: SO LEITURA da lista de conversas (nome e nao lidas) na aba reservada `whatsapp` do navegador da VM. Nao abre conversa, nao marca como lida, nao envia. Login por QR code e sempre do Dono (tela da VM). As ferramentas das IAs nao leem nem mexem nessa aba. Rotas: `GET /api/whatsapp`, `POST /api/whatsapp/abrir`. Falta: ler mensagens, responder, audio.

## Cofre (18/11/2026)

`src/cofre.rs`: meta de saldo ate o 5o dia util (feriados fixos + Sexta-feira Santa de 2026/2027), ritmo de 14 dias, nivel e fala em reais. O Rust NAO le banco: saldo e meta entram por `POST /api/cofre {saldo, meta}` e ficam em `dados/cofre.json`; `GET /api/cofre` devolve a situacao. Sem saldo, diz que nao tem. Na delegacao, o Cofre responde a conta quando ha saldo e meta informados. Falta: leitura da conta, alertas, vaquinha.

## Estado em 18/11/2026 (ondas de portagem e ligacao)

- 232 modulos do Braniac em Python que valem (fora: `skills/*`, vaquinha, xtts): todos tem as regras portadas com testes (984 testes).
  ~142 estao LIGADOS ao programa (rota, conversa ou relogio); ~90 estao prontos mas sem ligacao. Ver `/falta` e `GET /api/nao-ligados`.
- REGRA CENTRAL: este programa roda em paralelo ao Braniac real e NAO envia nada para fora. Todo envio (WhatsApp, Airbnb, e-mail,
  Telegram, publicar) vai para `dados/saida.jsonl` (tela `/saida`). So muda quando o dono mandar trocar.
- Ligacao em tres arquivos, chamados por `main.rs` (get, post, tick): `liga_atendimento.rs` (ensaio do atendimento, Airbnb e dono),
  `liga_esteira.rs` (levantamento pelo tribunal + auditoria), `liga_casa.rs` (escuta pela tela, cuidar do PC, vagas, Alma aprende).
- Esteira: `POST /api/tribunais/levantar {uf, cidade, numero}` com rede ligada em CE, RS, TO, ES, PR, SP, MA, GO, RJ, RN. Precisa de
  `BRANIAC_PACOTE` apontando o pacote de producao (o script de setores roda como processo; nao foi copiado). Conferido ao vivo em 18/11:
  Fortaleza (CE) chegou a "pronto" em 16 min e os 102 numeros de `cidade.json` bateram no centavo com os do Python; Natal (RN) parou em
  pendencia legitima (o tribunal nao publicou 2026).
- Subir: `BRANIAC_DADOS`, `BRANIAC_ENV`, `BRANIAC_PACOTE`. Recompilar sem derrubar a VM: `taskkill /IM braniac.exe /F` (nao matar a arvore).
- O que falta para substituir o atual: login do dono (QR do WhatsApp na VM, Gmail, Airbnb, Telegram, Spotify), aparelhos (microfone nativo,
  esfera, render, TTS da esteira), o que expoe a maquina (portal, rede, porta LAN, remoto: so o dono liga) e a troca em si (ler mensagens
  marca como lida no WhatsApp real, entao so depois de desligar o atendimento do Python).

## Conferencia ao vivo (18/11/2026, madrugada)

Levantamento direto do tribunal pelo Rust x `cidade.json` do pacote (Python), numeros iguais no centavo:
Fortaleza 102/102, Porto Alegre 44/44, Palmas 46/46, Rio de Janeiro 50/50, Curitiba 35/35, Goiania 38/38, Vitoria 38/38.
Sao Paulo levantou ("pronto" em 40 s; sem `cidade.json` de referencia no pacote). Natal: pendencia legitima (TCE-RN nao publicou 2026).
Fortaleza fim a fim: levantar -> roteiro (408 palavras, com legenda) -> conferir -> porta BARRADA ate a releitura do tribunal.
O roteiro roda em pasta-sombra (`<levantamento>/roteiro`): o pacote de producao nao e alterado (conferido: 0 arquivos mexidos).
Subir com: `BRANIAC_DADOS`, `BRANIAC_ENV`, `BRANIAC_PACOTE=C:\Cliente\PACOTE-PRODUCAO-COMPLETA`, `BRANIAC_GUIAS=C:\Cliente\guias-estados\estados`.

## WhatsApp: audio, arquivo e a chave "so o dono" (21/11/2026)

`src/wpp_midia.rs` (portado de wpp_voz.py e wpp_arquivo.py). ENVIAR AUDIO: o WAV falado vira `dados/mic.wav` (mono 16 bits + 1,5 s de silencio) e o Chromium da VM o le como MICROFONE FALSO (flags em `padrao/ligar.sh`; copiar para `/opt/braniac/ligar.sh` da VM e reiniciar o Chromium; a VM ve o arquivo em `/mnt/e/...`). Um envio por vez, nome conferido no topo antes de gravar, ate 150 s, rascunho na caixa barra, `gravando()` impede aba nova durante a gravacao. OUVIR: gancho em `URL.createObjectURL` pega o blob do audio (se nao veio, aperta o play do ultimo audio dele), 60 s de teto, falhou = 20 min de pausa por contato; transcricao LOCAL por `ouvir.rs` (`ferramentas/ouvir_whisper.py`, faster-whisper `whisper-small` no processador, ~6 s por frase, depende de Python com faster-whisper e do modelo do Braniac antigo, so leitura). ARQUIVO: `wpp_midia::enviar_arquivo` (seletor interceptado por CDP, legenda, confere "enviada"); nenhuma regra do laco o chama ainda. Chave restrita: `dados/troca.json` `{"whatsapp": false, "whatsapp_so_dono": true}` (ou `POST /api/troca {"whatsapp_so_dono": true}`) atende de verdade SO o chat do dono (texto e audio); o resto fica em sombra. As duas nascem false e so o dono liga.

