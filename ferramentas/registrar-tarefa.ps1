# Registra uma tarefa no quadro de Projetos do B.R.A.N.I (POST /api/projetos/evento). Sem janela, sem segredo no texto.
# Uso: powershell -NoProfile -File E:\braniac-rs\ferramentas\registrar-tarefa.ps1 -Projeto "B.R.A.N.I" -Tarefa "Corrigir o painel" -Estado feito -Nota "compilado e testado"
# Estado: feito | falta | novo | bloqueado | andando. Servidor fora do ar = avisa e sai sem erro (nao trava a sessao).
param([Parameter(Mandatory)][string]$Projeto, [Parameter(Mandatory)][string]$Tarefa,
      [ValidateSet('feito', 'falta', 'novo', 'bloqueado', 'andando')][string]$Estado = 'feito',
      [string]$Nota = '', [string]$Origem = 'claude', [string]$Servidor = 'http://127.0.0.1:8865')
$j = @{ projeto = $Projeto; tarefa = $Tarefa; estado = $Estado; origem = $Origem }
if ($Nota) { $j.nota = $Nota }
try {
    $r = Invoke-RestMethod "$Servidor/api/projetos/evento" -Method Post -Headers @{ 'X-Braniac' = 'claude' } `
        -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes(($j | ConvertTo-Json -Compress))) -TimeoutSec 10
    "quadro: $($r.projeto) -> $($r.resultado)"
} catch { "quadro: nao registrado ($($_.Exception.Message))" }
