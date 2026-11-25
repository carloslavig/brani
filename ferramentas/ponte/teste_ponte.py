# -*- coding: utf-8 -*-
"""Testes da ponte com PAR DE MENTIRA (nada vai a loja, a Aliado nem a rede): um B.R.A.N.I falso local, texto sintetico, pasta temporaria.
Roda:  py -3 E:\\braniac-rs\\ferramentas\\ponte\\teste_ponte.py
NAO prova a conexao real com os pares (WebRTC/ntfy/Radmin): isso so se prova na troca, com o Python antigo parado."""
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

TMP = Path(tempfile.mkdtemp(prefix="ponte-teste-"))
VISTO = []                      # (caminho, corpo, token) que o B.R.A.N.I falso recebeu
RESP = {}                       # caminho -> (status, json)


class Falso(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        corpo = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)) or b"{}")
        VISTO.append((self.path, corpo, self.headers.get("X-Ponte-Token")))
        st, r = RESP.get(self.path, (200, {"ok": True}))
        d = json.dumps(r).encode()
        self.send_response(st)
        self.send_header("Content-Length", str(len(d)))
        self.end_headers()
        self.wfile.write(d)


srv = ThreadingHTTPServer(("127.0.0.1", 0), Falso)
threading.Thread(target=srv.serve_forever, daemon=True).start()
os.environ.update(PONTE_DADOS=str(TMP), PONTE_URL=f"http://127.0.0.1:{srv.server_address[1]}", PONTE_TOKEN="token-de-teste", PONTE_TESTE_SEM_CHECAR_PYTHON="1", PONTE_PORTA_FILA="0")
sys.path.insert(0, str(Path(__file__).parent))
import ponte  # noqa: E402


def http(porta, metodo, caminho, corpo=b"", sha=None):
    req = urllib.request.Request(f"http://127.0.0.1:{porta}{caminho}", data=corpo or None, method=metodo,
                                 headers={"X-Link-Id": "par1", "X-Link-Sha": sha if sha is not None else hashlib.sha256(corpo).hexdigest(), "X-Link-Ts": "1", "X-Link-Sig": "x", "X-Link-Nonce": "n"})
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, r.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()


class Loja(unittest.TestCase):
    def setUp(self):
        VISTO.clear()
        RESP.clear()

    def test_mensagem_da_loja_vai_ao_brani_com_token_e_volta_a_resposta(self):
        RESP["/api/ponte/loja"] = (200, {"ok": True, "texto": "resposta-sintetica"})
        self.assertEqual(ponte.responder_loja("loja", "oi sintetico"), "resposta-sintetica")
        c, corpo, token = VISTO[-1]
        self.assertEqual((c, corpo["texto"], token), ("/api/ponte/loja", "oi sintetico", "token-de-teste"))

    def test_chave_desligada_a_ponte_nao_responde_ao_par(self):
        RESP["/api/ponte/loja"] = (503, {"ok": False, "desligado": True})
        with self.assertRaises(RuntimeError):
            ponte.responder_loja("loja", "oi")

    def test_ligar_loja_troca_o_responder_antigo(self):
        class RJ:
            _responder = None
            chamado = None

            @classmethod
            def iniciar(cls, router):
                cls.chamado = router
        ponte.ligar_loja({"rede_jarvis": RJ})
        self.assertIs(RJ._responder, ponte.responder_loja)
        self.assertTrue(ponte.ESTADO["loja"])
        ponte.ESTADO["loja"] = False


class Trava(unittest.TestCase):
    def test_duas_pontes_nao_sobem(self):
        ponte.LOCK.unlink(missing_ok=True)
        outro = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        try:
            ponte.LOCK.write_text(str(outro.pid))
            self.assertEqual(ponte.pegar_trava(), "ja existe outra ponte")
        finally:
            outro.kill()
            outro.wait()
        self.assertEqual(ponte.pegar_trava(), "")       # trava de ponte que morreu e retomada
        ponte.soltar_trava()
        self.assertFalse(ponte.LOCK.exists())


class Fila(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        lib = ponte.carregar()
        from braniac.link import confianca
        pares = confianca.Pares(TMP / "link")
        cls.f = ponte.FilaHTTP(lib, pares, TMP / "link" / "fila-ponte")
        cls.porta = cls.f.servir(0)
        cls.real = cls.f.fila.autenticar
        # a assinatura Ed25519 e do Python antigo e nao e o que se testa aqui: o par de mentira e confiavel
        cls.f.fila.autenticar = lambda m, c, h, agora=None: "par1"

    @classmethod
    def tearDownClass(cls):
        cls.f.parar()

    def setUp(self):
        VISTO.clear()
        RESP.clear()

    def test_o_estado_do_python_antigo_nao_foi_tocado(self):
        self.assertTrue(str((TMP / "link").resolve()).startswith(str(TMP.resolve())))
        import braniac.store as st
        self.assertEqual(Path(st.DATA), TMP)

    def test_mensagem_do_par_entra_no_brani(self):
        st, _ = http(self.porta, "POST", "/mensagem", json.dumps({"texto": "msg sintetica"}).encode())
        self.assertEqual(st, 200)
        self.assertEqual([(c, b["par"], b["texto"]) for c, b, _ in VISTO], [("/api/ponte/link/mensagem", "par1", "msg sintetica")])

    def test_pedido_de_render_e_recusado_com_clareza(self):
        RESP["/api/ponte/link/pedido"] = (503, {"ok": False, "erro": "ainda nao disponivel: sintetico"})
        st, corpo = http(self.porta, "POST", "/pedido", json.dumps({"tipo": "render"}).encode())
        self.assertEqual(st, 503)
        self.assertIn("ainda nao disponivel", corpo)
        st, corpo = http(self.porta, "GET", "/job/abcdef012345")
        self.assertEqual(st, 404)

    def test_consulta_do_predio_passa_pelo_brani(self):
        RESP["/api/ponte/link/http"] = (200, {"status": 200, "tipo": "application/json", "corpo": '{"olhando": false}'})
        st, corpo = http(self.porta, "GET", "/predio-par/olhando")
        self.assertEqual((st, json.loads(corpo)), (200, {"olhando": False}))
        c, b, _ = VISTO[-1]
        self.assertEqual((c, b["metodo"], b["caminho"], b["pid"]), ("/api/ponte/link/http", "GET", "/predio-par/olhando", "par1"))

    def test_corpo_que_nao_confere_com_o_hash_e_barrado(self):
        st, _ = http(self.porta, "POST", "/mensagem", b'{"texto":"x"}', sha="0" * 64)
        self.assertEqual(st, 400)
        self.assertEqual(VISTO, [])

    def test_par_desconhecido_e_assinatura_ruim_dao_403_sem_chegar_ao_brani(self):
        self.f.fila.autenticar = self.real
        try:
            st, _ = http(self.porta, "POST", "/mensagem", b'{"texto":"x"}')
        finally:
            self.f.fila.autenticar = lambda m, c, h, agora=None: "par1"
        self.assertEqual(st, 403)
        self.assertEqual(VISTO, [])

    def test_resposta_do_brani_ao_par_sai_pelo_no(self):
        enviados = []

        class No:
            def enviar_msg(self, par, texto, timeout=0):
                enviados.append((par, texto))
        ponte.ESTADO["no"] = No()
        ponte.LIB["integracao"]._avisar_par = lambda n, p, t: n.enviar_msg(p, t)
        RESP["/api/ponte/link/mensagem"] = (200, {"ok": True, "resposta": "ainda nao disponivel: sintetico"})
        http(self.porta, "POST", "/mensagem", json.dumps({"texto": "VIDEO APROVADO 1"}).encode())
        for _ in range(50):
            if enviados:
                break
            threading.Event().wait(0.1)
        self.assertEqual(enviados, [("par1", "ainda nao disponivel: sintetico")])
        ponte.ESTADO["no"] = None

    def test_saida_do_brani_vai_ao_par(self):
        enviados = []

        class No:
            def enviar_msg(self, par, texto, timeout=0):
                enviados.append((par, texto))
        ponte.ESTADO["no"] = No()
        RESP["/api/ponte/link/saida"] = (200, {"ok": True, "itens": [{"id": 7, "par": "par1", "texto": "(1/1) oi", "ts": 1}]})
        ponte.entregar_saida(ponte.LIB)
        self.assertEqual(enviados, [("par1", "(1/1) oi")])
        self.assertEqual(VISTO[-1][1], {"acao": "feito", "ids": [7]})
        ponte.ESTADO["no"] = None


class ArquivosDoLog(unittest.TestCase):
    def test_log_so_tem_contagens(self):
        ponte.log("evento", recebidas=3, texto="SEGREDO")
        t = ponte.LOG.read_text(encoding="utf-8")
        self.assertIn("recebidas=3", t)
        self.assertNotIn("SEGREDO", t)


if __name__ == "__main__":
    unittest.main(verbosity=2)
