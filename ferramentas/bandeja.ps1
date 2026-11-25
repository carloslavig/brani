# Icones do B.R.A.N.I na bandeja do Windows (Jarvis = anel laranja, Alma = anel roxo). Sem janela flutuante.
# Uso: powershell -STA -File bandeja.ps1 [porta]. Conversa com o servidor so por HTTP (127.0.0.1); uma copia so (mutex nomeado).
param([int]$Porta = 8865)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$criado = $false
$mx = New-Object System.Threading.Mutex($true, 'Local\BraniBandeja', [ref]$criado)
if (-not $criado) { exit }
$base = "http://127.0.0.1:$Porta"
$cab = @{ 'X-Braniac' = 'bandeja' }
function Pegar($c) { try { Invoke-RestMethod -Uri "$base$c" -Headers $cab -TimeoutSec 5 } catch { $null } }
function Mandar($c, $o) { try { Invoke-RestMethod -Uri "$base$c" -Method Post -Headers $cab -ContentType 'application/json' -Body ($o | ConvertTo-Json -Compress) -TimeoutSec 20 | Out-Null } catch {} }
function Anel($cor) {
    $b = New-Object System.Drawing.Bitmap 32, 32
    $g = [System.Drawing.Graphics]::FromImage($b); $g.SmoothingMode = 'AntiAlias'
    $p = New-Object System.Drawing.Pen($cor, 5); $g.DrawEllipse($p, 4, 4, 23, 23); $g.Dispose()
    [System.Drawing.Icon]::FromHandle($b.GetHicon())
}
function Criar($nome, $cor, $extra) {
    $ni = New-Object System.Windows.Forms.NotifyIcon
    $ni.Icon = Anel $cor; $ni.Text = "B.R.A.N.I - $nome"; $ni.Visible = $true
    $m = New-Object System.Windows.Forms.ContextMenuStrip
    # sempre o APLICATIVO (o servidor abre ou traz a janela do app para a frente), nunca o navegador
    $abrir = $m.Items.Add("Abrir o painel"); $abrir.Add_Click({ Mandar '/api/app/abrir' @{ rota = '/' } })
    $ni.Add_MouseClick({ param($s, $e) if ($e.Button -eq 'Left') { Mandar '/api/app/abrir' @{ rota = '/' } } })
    $srv = $m.Items.Add("Servidor: ...")
    $srv.Add_Click({ $s = Pegar '/api/servidor'; if ($s.estado -eq 'ligado') { Mandar '/api/servidor/pausa' @{} } elseif ($s.estado -eq 'pausado') { Mandar '/api/servidor/retomar' @{} } })
    $dormir = $m.Items.Add("Acordar / Descansar"); $mic = $m.Items.Add("Ligar / desligar microfone"); $sil = $m.Items.Add("Silenciar por 1 hora")
    $dormir.Add_Click({ $e = Pegar '/api/escuta-estado'; Mandar '/api/escuta-acao' @{ acao = $(if ($e.estado -eq 'acordado') { 'descansar' } else { 'acordar' }) } })
    $mic.Add_Click({ $e = Pegar '/api/escuta-estado'; Mandar '/api/escuta-acao' @{ acao = $(if ($e.microfone.microfone_ligado) { 'desligar' } else { 'ligar' }) } })
    $sil.Add_Click({ Mandar '/api/mudo' @{ minutos = 60 } })
    if ($extra) { $dormir.Visible = $false; $mic.Visible = $false }
    # com a chave "voz" desligada o aparelho nao existe: so "Abrir o painel" fica ligado
    $m.Add_Opening({
        $s = Pegar '/api/servidor'
        $srv.Text = switch ($s.estado) { 'ligado' { 'Servidor: ligado (clique para desligar)' } 'parando' { 'Servidor: parando...' } 'pausado' { 'Servidor: desligado (clique para ligar)' } default { 'Servidor: sem resposta' } }
        $srv.Enabled = ($s.estado -eq 'ligado' -or $s.estado -eq 'pausado')
        $e = Pegar '/api/aparelho/voz'; $on = [bool]($e -and $e.chave); $dormir.Enabled = $on; $mic.Enabled = $on; $sil.Enabled = $on })
    $ni.ContextMenuStrip = $m
    $ni
}
$jarvis = Criar 'Jarvis' ([System.Drawing.Color]::FromArgb(242, 150, 50)) $false
$alma = Criar 'Alma' ([System.Drawing.Color]::FromArgb(168, 85, 247)) $true
[System.Windows.Forms.Application]::Run()
