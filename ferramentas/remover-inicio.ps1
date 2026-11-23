# Desfaz instalar-inicio.ps1: apaga SO o atalho "B.R.A.N.I.lnk" da pasta Inicializar. Nao para o B.R.A.N.I que ja estiver rodando.
param([string]$Pasta = [Environment]::GetFolderPath('Startup'))
$atalho = Join-Path $Pasta 'B.R.A.N.I.lnk'
if (Test-Path $atalho) { Remove-Item $atalho -Force; Write-Host "Removido: $atalho" } else { Write-Host "Nao havia atalho em $Pasta" }
