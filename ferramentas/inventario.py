"""Refaz padrao/modulos.json e paridade.json a partir do Braniac em Python.

Uso: py -3 ferramentas/inventario.py [pasta do braniac]
Quem ja esta "portado" ou "em andamento" no paridade.json continua assim.
"""
import ast, json, sys
from pathlib import Path

ORIGEM = Path(sys.argv[1] if len(sys.argv) > 1 else r"C:\Users\usuario\Projetos\Agente\braniac")
AQUI = Path(__file__).parent.parent
FORA = {"data", "tests", "__pycache__", "graphify-out", "modelos", "wpp_baileys", "kit", "dados-link"}

sys.path.insert(0, str(ORIGEM.parent))
from braniac import modulos
(AQUI / "padrao" / "modulos.json").write_text(
    json.dumps(modulos.CATALOGO, ensure_ascii=False, indent=1), encoding="utf-8")

arq = AQUI / "paridade.json"
antigo = {i["modulo"]: i["estado"] for i in json.loads(arq.read_text(encoding="utf-8"))} if arq.exists() else {}
itens = []
for f in sorted(ORIGEM.rglob("*.py")):
    rel = f.relative_to(ORIGEM)
    if FORA & set(rel.parts) or ".bak" in f.name or f.name.startswith("__"):
        continue
    texto = f.read_text(encoding="utf-8", errors="replace")
    try:
        doc = (ast.get_docstring(ast.parse(texto)) or "").strip().splitlines()
    except SyntaxError:
        doc = []
    nome = rel.with_suffix("").as_posix()
    itens.append({"modulo": nome, "linhas": texto.count("\n") + 1,
                  "o_que_faz": doc[0] if doc else "", "estado": antigo.get(nome, "falta")})
arq.write_text(json.dumps(itens, ensure_ascii=False, indent=1), encoding="utf-8")
print(len(itens), "modulos;", sum(i["linhas"] for i in itens), "linhas;",
      sum(i["estado"] == "portado" for i in itens), "portados")
