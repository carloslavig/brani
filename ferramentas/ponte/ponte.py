# -*- coding: utf-8 -*-
"""PONTE TEMPORARIA entre o B.R.A.N.I (Rust) e os dois elos de rede que ele ainda nao sabe abrir: a loja e o Link com o outro Braniac.

Usa SO a parte de rede do Braniac antigo (C:\\Users\\usuario\\Projetos\\Agente\\braniac) como biblioteca: rede.py, par_webrtc.py, rede_jarvis.py,
link/no.py, link/confianca.py, link/identidade.py, link/avisos.py e link/fila.py (so a parte de autenticacao). NAO sobe servidor 8765, Chrome,
atendimento, rotinas, vigia nem voz. O Python antigo NAO e alterado: o estado vive em E:\\braniac-rs\\dados (store.DATA e redirecionado antes de
importar o resto). Quem pensa e responde e o B.R.A.N.I: a ponte so leva e traz (POST /api/ponte/* com o token em X-Ponte-Token).

Seguranca: so um dono do elo por vez. Se o servidor antigo estiver no ar (porta 8765 ou processo braniac.server) ou ja houver outra ponte
(dados/ponte.lock), esta ponte sai com codigo 3 sem abrir nada. Log (dados/ponte.log): so eventos e contagens, NUNCA conteudo de conversa.
Variaveis: PONTE_DADOS, PONTE_URL, PONTE_TOKEN, PONTE_PAI (pid do braniac.exe; a ponte sai sozinha se ele sumir)."""
import hashlib
import json
import os
import re
import secrets
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.dont_write_bytecode = True      # nunca grava __pycache__ na pasta do Python antigo
AGENTE = os.environ.get("PONTE_BRANIAC_PY", r"C:\Users\usuario\Projetos\Agente")
DADOS = Path(os.environ.get("PONTE_DADOS", r"E:\braniac-rs\dados"))
URL = os.environ.get("PONTE_URL", "http://127.0.0.1:8865")
TOKEN = os.environ.get("PONTE_TOKEN", "")
PAI = int(os.environ.get("PONTE_PAI", "0") or 0)
PORTA_FILA = int(os.environ.get("PONTE_PORTA_FILA", "8771"))
SEM_CHECAR_PYTHON = os.environ.get("PONTE_TESTE_SEM_CHECAR_PYTHON") == "1"   # SO para teste com par de mentira
LOCK = DADOS / "ponte.lock"
LOG = DADOS / "ponte.log"
NOVO = 0x08000000
MAX_PREDIO = 25 * 1024 * 1024
CONT = {"recebidas": 0, "enviadas": 0}
ESTADO = {"loja": False, "link": False, "fila": None, "tunel": None, "no": None, "router": None, "sair": False}
_ESTADO_ERRO = [""]


def log(evento, **contagens):
    """So evento e numeros."""
    try:
        with LOG.open("a", encoding="utf-8") as f:
            f.write(time.strftime("%Y-%m-%d %H:%M:%S") + " ponte " + evento + "".join(f" {k}={v}" for k, v in contagens.items() if isinstance(v, (int, float))) + "\n")
    except OSError:
        pass


# ---------------- trava contra dois donos do elo ----------------
def pid_vivo(pid):
    import ctypes
    h = ctypes.windll.kernel32.OpenProcess(0x1000, False, int(pid))
    if not h:
        return False
    try:
        cod = ctypes.c_ulong()
        return bool(ctypes.windll.kernel32.GetExitCodeProcess(h, ctypes.byref(cod))) and cod.value == 259
    finally:
        ctypes.windll.kernel32.CloseHandle(h)


def python_antigo_no_ar():
    """Servidor antigo = porta 8765 respondendo OU processo braniac.server vivo."""
    try:
        with socket.socket() as s:
            s.settimeout(1.5)
            if s.connect_ex(("127.0.0.1", 8765)) == 0:
                return "porta 8765 respondendo"
    except OSError:
        pass
    try:
        r = subprocess.run(["powershell", "-NoProfile", "-Command",
                            "(Get-CimInstance Win32_Process | ? { $_.CommandLine -match 'braniac[.]server' } | Measure-Object).Count"],
                           capture_output=True, text=True, timeout=40, creationflags=NOVO)
        if r.stdout.strip().isdigit() and int(r.stdout.strip()) > 0:
            return "processo braniac.server vivo"
    except Exception:
        pass
    return ""


def pegar_trava():
    """Um arquivo de trava impede duas pontes. Devolve '' se pegou, ou o motivo."""
    DADOS.mkdir(parents=True, exist_ok=True)
    for _ in range(2):
        try:
            fd = os.open(str(LOCK), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            os.write(fd, str(os.getpid()).encode())
            os.close(fd)
            return ""
        except FileExistsError:
            try:
                dono = int(LOCK.read_text().strip() or 0)
            except (OSError, ValueError):
                dono = 0
            if dono and dono != os.getpid() and pid_vivo(dono):
                return "ja existe outra ponte"
            try:
                LOCK.unlink()          # trava velha de ponte que morreu
            except OSError:
                pass
    return "nao consegui a trava"


def soltar_trava():
    try:
        if LOCK.read_text().strip() == str(os.getpid()):
            LOCK.unlink()
    except OSError:
        pass


# ---------------- conversa com o B.R.A.N.I ----------------
def post(caminho, corpo, timeout=150):
    """POST ao B.R.A.N.I. Devolve (status, json). Nunca imprime o corpo."""
    req = urllib.request.Request(URL + caminho, data=json.dumps(corpo).encode(), method="POST",
                                 headers={"Content-Type": "application/json", "X-Ponte-Token": TOKEN, "X-Braniac": "ponte"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        try:
            return e.code, json.loads(e.read() or b"{}")
        except ValueError:
            return e.code, {}


class RouterPonte:
    """Faz o papel do router do Braniac antigo: a 'IA neutra' do Link e o B.R.A.N.I."""
    def responder_neutro(self, sistema, texto):
        st, r = post("/api/ponte/link/tarefa", {"pedido": texto})
        if st != 200:
            raise RuntimeError("elo desligado" if r.get("desligado") else "sem resposta")
        return (r.get("texto", ""),)


# ---------------- loja (rede_jarvis / par_webrtc) ----------------
def responder_loja(de, texto):
    st, r = post("/api/ponte/loja", {"de": de, "texto": texto})
    if st == 200 and r.get("ok"):
        CONT["recebidas"] += 1
        CONT["enviadas"] += 1
        return r["texto"]
    raise RuntimeError("elo da loja desligado" if r.get("desligado") else "sem resposta do B.R.A.N.I")   # nunca responde ao par sem o B.R.A.N.I


def ligar_loja(lib):
    lib["rede_jarvis"]._responder = responder_loja          # instancia() e par() leem este nome na hora de criar
    lib["rede_jarvis"].iniciar(ESTADO["router"])
    ESTADO["loja"] = True
    log("loja_ligada")


def desligar_loja(lib):
    rj = lib["rede_jarvis"]
    for parar in (lambda: rj.par().parar(), lambda: rj.instancia().parar()):
        try:
            parar()
        except Exception:
            pass
    ESTADO["loja"] = False
    log("loja_desligada")


def loja_conectada(lib):
    try:
        i = lib["rede_jarvis"].par().info()
        return bool(i.get("conectado"))
    except Exception:
        return False


# ---------------- link (no + fila HTTPS + tunel) ----------------
def ao_msg(par, texto):
    """Mensagem do par: o B.R.A.N.I guarda na caixa, junta as partes, avisa o Jarvis e decide o que responder."""
    CONT["recebidas"] += 1
    st, r = post("/api/ponte/link/mensagem", {"par": par, "texto": texto})
    resp = r.get("resposta") if st == 200 else None
    if resp:
        threading.Thread(target=enviar_ao_par, args=(par, resp), daemon=True).start()


def enviar_ao_par(par, texto):
    lib, n = LIB, ESTADO["no"]
    try:
        lib["integracao"]._avisar_par(n, par, texto)
        CONT["enviadas"] += 1
    except Exception:
        log("envio_falhou")


class FilaHTTP:
    """A porta 8771 (so 127.0.0.1; o tunel e que a expoe, e so o dono liga). Autentica com o Fila antigo (Ed25519) e repassa ao B.R.A.N.I."""
    def __init__(self, lib, pares, pasta):
        self.lib, self.pares = lib, pares
        self.fila = lib["Fila"](pares, pasta, lambda *a: (_ for _ in ()).throw(RuntimeError("ainda nao disponivel")), agente="render")   # so autentica
        self.srv = None

    def servir(self, porta):
        ponte = self

        class H(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def _json(self, codigo, obj):
                d = json.dumps(obj, ensure_ascii=False).encode()
                self.send_response(codigo)
                self.send_header("Content-Type", "application/json; charset=utf-8")
                self.send_header("Content-Length", str(len(d)))
                self.end_headers()
                self.wfile.write(d)

            def _tratar(self, metodo):
                tmp = None
                try:
                    tam = int(self.headers.get("Content-Length") or 0)
                    if tam > MAX_PREDIO:
                        return self._json(413, {"erro": "corpo grande demais"})
                    h = {k: self.headers.get(k, "") for k in ("X-Link-Id", "X-Link-Ts", "X-Link-Sha", "X-Link-Sig", "X-Link-Nonce")}
                    pid = ponte.fila.autenticar(metodo, self.path, h)            # assinatura confere ANTES de ler o corpo
                    pasta_tmp = DADOS / "link" / "ponte-tmp"
                    pasta_tmp.mkdir(parents=True, exist_ok=True)
                    tmp = pasta_tmp / ("rec-" + secrets.token_hex(6))
                    sha, resta = hashlib.sha256(), tam
                    with tmp.open("wb") as f:
                        while resta > 0:
                            parte = self.rfile.read(min(1 << 20, resta))
                            if not parte:
                                break
                            sha.update(parte)
                            f.write(parte)
                            resta -= len(parte)
                    if resta or sha.hexdigest() != h["X-Link-Sha"]:
                        return self._json(400, {"erro": "corpo nao confere com o hash assinado"})
                    return self._rotear(metodo, pid, tmp)
                except ponte.lib["FilaErro"] as e:
                    return self._json(403 if any(x in str(e) for x in ("permissao", "assinatura", "repetido", "data")) else 400, {"erro": str(e)})
                except Exception:
                    return self._json(500, {"erro": "erro na ponte"})
                finally:
                    if tmp:
                        tmp.unlink(missing_ok=True)

            def _rotear(self, metodo, pid, tmp):
                CONT["recebidas"] += 1
                partes = [p for p in self.path.split("?")[0].split("/") if p]
                corpo = tmp.read_bytes()[:200_000] if tmp.exists() else b""
                if metodo == "GET" and partes == ["avisos"]:          # a caixa de avisos e do proprio Python (mesmo arquivo que o B.R.A.N.I le)
                    from urllib.parse import parse_qs, urlparse
                    desde = (parse_qs(urlparse(self.path).query).get("desde") or ["0"])[0]
                    return self._json(200, ponte.lib["avisos"].ler(pid, int(desde) if desde.isdigit() else 0))
                if metodo == "POST" and partes == ["mensagem"]:
                    texto = str((json.loads(corpo or b"{}")).get("texto") or "")
                    if not texto.strip():
                        return self._json(400, {"erro": "texto vazio"})
                    ao_msg(pid, texto[:200_000])
                    return self._json(200, {"ok": True})
                if metodo == "POST" and partes == ["pedido"]:           # render/producao: o B.R.A.N.I ainda nao faz e diz isso
                    st, r = post("/api/ponte/link/pedido", {"par": pid})
                    return self._json(503, {"erro": r.get("erro", "ainda nao disponivel")})
                if partes and partes[0] == "job":
                    return self._json(404, {"erro": "ainda nao disponivel: nao ha fila de render neste B.R.A.N.I"})
                caminho_tmp = str(tmp) if tmp.exists() and tmp.stat().st_size else ""      # fica em dados/link/ponte-tmp ate o fim do pedido
                st, r = post("/api/ponte/link/http", {"metodo": metodo, "caminho": self.path, "pid": pid, "corpo_arquivo": caminho_tmp})
                if st != 200 or "status" not in r:
                    return self._json(st if st >= 400 else 502, {"erro": r.get("erro", "sem resposta do B.R.A.N.I")})
                corpo_r = str(r.get("corpo", "")).encode("utf-8")
                self.send_response(int(r["status"]))
                self.send_header("Content-Type", r.get("tipo") or "application/json")
                self.send_header("Content-Length", str(len(corpo_r)))
                self.end_headers()
                self.wfile.write(corpo_r)

            def do_GET(self):
                self._tratar("GET")

            def do_POST(self):
                self._tratar("POST")

            def do_PUT(self):
                self._tratar("PUT")

        self.srv = ThreadingHTTPServer(("127.0.0.1", porta), H)
        self.srv.daemon_threads = True
        threading.Thread(target=self.srv.serve_forever, daemon=True, name="ponte-fila").start()
        return self.srv.server_address[1]

    def parar(self):
        if self.srv:
            self.srv.shutdown()
            self.srv.server_close()
            self.srv = None


def ligar_link(lib):
    integ = lib["integracao"]
    integ._ao_msg = ao_msg                                           # no() le este nome ao criar o No
    n = integ.no(ESTADO["router"])
    n.iniciar()
    ESTADO["no"] = n
    pasta = integ.PASTA
    if (pasta / "pares.json").exists():
        threading.Thread(target=integ._tentar_conectar, args=(n,), daemon=True, name="ponte-conectar").start()
    if any(p["estado"] == "confiavel" and n.pares.decidir(i, "tarefa", "render") == "aceitar" for i, p in n.pares._ler()["pares"].items()):
        ESTADO["fila"] = FilaHTTP(lib, n.pares, pasta / "fila-ponte")
        ESTADO["fila"].servir(PORTA_FILA)
    ESTADO["link"] = True
    log("link_ligado")


def desligar_link(lib):
    parar_tunel()
    if ESTADO["fila"]:
        ESTADO["fila"].parar()
        ESTADO["fila"] = None
    if ESTADO["no"]:
        try:
            ESTADO["no"].parar()
        except Exception:
            pass
        ESTADO["no"] = None
        lib["integracao"]._NO[0] = None
    ESTADO["link"] = False
    log("link_desligado")


def parar_tunel():
    p = ESTADO["tunel"]
    if p and p.poll() is None:
        p.terminate()
    ESTADO["tunel"] = None


def vigiar_tunel(lib, quer):
    """cloudflared so com a chave do dono (dados/link/tunel_ligado); o endereco fica so em fila_url.txt, nada e anunciado ao par."""
    integ, url_arq = lib["integracao"], lib["integracao"].PASTA / "fila_url.txt"
    if not (quer and ESTADO["fila"]):
        if ESTADO["tunel"]:
            parar_tunel()
            url_arq.unlink(missing_ok=True)
        return
    if ESTADO["tunel"] and ESTADO["tunel"].poll() is None:
        return
    exe = integ._exe()
    if not exe:
        return
    p = subprocess.Popen([exe, "tunnel", "--no-autoupdate", "--protocol", "http2", "--url", f"http://127.0.0.1:{PORTA_FILA}"], stderr=subprocess.PIPE, stdout=subprocess.DEVNULL,
                         text=True, encoding="utf-8", errors="replace", creationflags=NOVO)
    ESTADO["tunel"] = p

    def ler():
        for linha in p.stderr:
            m = integ.URL_TUNEL.search(linha)
            if m:
                url_arq.write_text(m.group(0), encoding="utf-8")
    threading.Thread(target=ler, daemon=True, name="ponte-tunel").start()
    log("tunel_ligado")


def entregar_saida(lib):
    """O que o B.R.A.N.I quer dizer ao par (/api/par/link/falar) sai por aqui: cada parte pelo canal do No."""
    n = ESTADO["no"]
    if not n:
        return
    st, r = post("/api/ponte/link/saida", {"acao": "pegar"})
    feitos = []
    for item in (r.get("itens") or []) if st == 200 else []:
        try:
            n.enviar_msg(item["par"], item["texto"], timeout=45)
            feitos.append(item["id"])
            CONT["enviadas"] += 1
        except Exception:
            log("saida_falhou")
            if time.time() - item.get("ts", 0) > 900:      # 15 min tentando: desiste (o dono ve em /api/ponte as contagens)
                feitos.append(item["id"])
    if feitos:
        post("/api/ponte/link/saida", {"acao": "feito", "ids": feitos})


def link_conectado():
    n = ESTADO["no"]
    return bool(n and getattr(n, "sessoes", None))


# ---------------- carga da biblioteca (so a parte de rede) ----------------
LIB = {}


def carregar():
    sys.path.insert(0, AGENTE)
    import braniac.store as store
    store.DATA = DADOS                  # ANTES de importar o resto: o estado vai para o B.R.A.N.I, nada se grava na pasta do Python antigo
    from braniac import rede_jarvis
    from braniac.link import avisos, integracao
    from braniac.link.fila import Fila, FilaErro
    LIB.update(rede_jarvis=rede_jarvis, avisos=avisos, integracao=integracao, Fila=Fila, FilaErro=FilaErro)
    return LIB


def principal():
    motivo = "" if SEM_CHECAR_PYTHON else python_antigo_no_ar()
    if motivo:
        log("recusou_subir_python_antigo_no_ar")
        print("ponte: recusei subir, o Braniac antigo esta no ar (" + motivo + "). So um pode segurar os elos.", flush=True)
        return 3
    t = pegar_trava()
    if t:
        log("recusou_subir_trava")
        print("ponte: recusei subir (" + t + ").", flush=True)
        return 3
    try:
        if not TOKEN:
            print("ponte: falta PONTE_TOKEN (quem inicia e o braniac.exe).", flush=True)
            return 2
        lib = carregar()
        ESTADO["router"] = RouterPonte()
        log("subiu")
        falhas = 0
        while not ESTADO["sair"]:
            relato = {"loja_ativo": ESTADO["loja"], "link_ativo": ESTADO["link"], "loja_conectado": ESTADO["loja"] and loja_conectada(lib),
                      "link_conectado": ESTADO["link"] and link_conectado(), **CONT, "erro": _ESTADO_ERRO[0]}
            try:
                st, chaves = post("/api/ponte/ciclo", relato, timeout=10)
                falhas = 0 if st == 200 else falhas + 1
            except Exception:
                chaves, falhas = {}, falhas + 1
            if falhas >= 8 or (PAI and not pid_vivo(PAI)):           # o B.R.A.N.I sumiu: a ponte nao fica solta
                break
            if falhas == 0:
                try:
                    if chaves.get("loja") and not ESTADO["loja"]:
                        ligar_loja(lib)
                    elif not chaves.get("loja") and ESTADO["loja"]:
                        desligar_loja(lib)
                    if chaves.get("link") and not ESTADO["link"]:
                        ligar_link(lib)
                    elif not chaves.get("link") and ESTADO["link"]:
                        desligar_link(lib)
                    if ESTADO["link"]:
                        vigiar_tunel(lib, bool(chaves.get("tunel")))
                        entregar_saida(lib)
                    _ESTADO_ERRO[0] = ""
                except Exception as e:
                    _ESTADO_ERRO[0] = type(e).__name__           # so o tipo do erro, nunca a mensagem
                    log("erro_" + type(e).__name__)
            time.sleep(2)
        for f, ativo in ((desligar_loja, ESTADO["loja"]), (desligar_link, ESTADO["link"])):
            if ativo:
                f(lib)
        log("saiu")
        return 0
    finally:
        soltar_trava()


if __name__ == "__main__":
    sys.exit(principal())
