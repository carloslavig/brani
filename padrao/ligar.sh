#!/bin/sh
# O microfone do navegador e FALSO: le dados/mic.wav (o B.R.A.N.I grava a fala ali) para enviar mensagem de voz no WhatsApp.
# Liga a tela, o navegador e a tela remota da VM do Braniac. Fica em primeiro plano:
# o WSL desliga a distro quando o ultimo processo iniciado pelo Windows termina.
export DISPLAY=:1
# o WSL anuncia Wayland e o x11vnc se recusa a subir; aqui e tudo X11
unset WAYLAND_DISPLAY XDG_SESSION_TYPE
pgrep -x Xvfb >/dev/null || Xvfb :1 -screen 0 1366x768x24 -nolisten tcp >/dev/null 2>&1 &
sleep 1
pgrep -x openbox >/dev/null || su braniac -c "DISPLAY=:1 openbox" >/dev/null 2>&1 &
pgrep -f remote-debugging-port=9333 >/dev/null || su braniac -c "DISPLAY=:1 chromium --ozone-platform=x11 --remote-debugging-port=9333 --user-data-dir=/home/braniac/perfil --no-first-run --no-default-browser-check --start-maximized --disable-gpu --use-fake-device-for-media-stream --use-fake-ui-for-media-stream --use-file-for-fake-audio-capture=/mnt/e/braniac-rs/dados/mic.wav%noloop --mute-audio about:blank" >/dev/null 2>&1 &
pgrep -x x11vnc >/dev/null || x11vnc -display :1 -localhost -forever -shared -nopw -quiet >/dev/null 2>&1 &
pgrep -f "websockify.*6090" >/dev/null || websockify --web /usr/share/novnc 6090 localhost:5900 >/dev/null 2>&1 &
exec sleep infinity
