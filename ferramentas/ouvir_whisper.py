"""Ponte do Braniac para o Whisper local (faster-whisper). Uso: python ouvir_whisper.py <audio> <pasta do modelo>
Escreve so o texto falado, em UTF-8. Roda no processador (int8) para nao disputar a placa de video com outros programas.
Exemplo de dados/ouvir.json: {"comando": ["python", "-X", "utf8", "ferramentas/ouvir_whisper.py", "{audio}", "C:/caminho/whisper-small"]}"""
import os
import sys

from faster_whisper import WhisperModel

audio, modelo = sys.argv[1], sys.argv[2]
m = WhisperModel(modelo, device="cpu", compute_type="int8", cpu_threads=max(4, (os.cpu_count() or 8) // 2))
segs, _ = m.transcribe(audio, language="pt", vad_filter=True, beam_size=5, without_timestamps=True, condition_on_previous_text=False,
                         initial_prompt="Jarvis, Braniac, Dono, Deusa Parceira, Claudinho, Airbnb, WhatsApp. Não é com o filho não, Jarvis, é com o marido. Dor de cabeça, enjoo, pressão alta.")
sys.stdout.reconfigure(encoding="utf-8")
print(" ".join(s.text.strip() for s in segs).strip())
