# Instala o B.R.A.N.I para subir sozinho no logon do Windows: cria UM atalho na pasta Inicializar do usuario (nada alem disso).
# O atalho chama o lancador escondido (lancar-brani.vbs): espera o E:, nao sobe se ja houver um braniac.exe e nao mostra janela.
# NAO foi rodado pelo Claude: o dono roda quando quiser.  Desfazer: remover-inicio.ps1.   Teste sem instalar: -Pasta C:\temp\x
param([string]$Pasta = [Environment]::GetFolderPath('Startup'))
$ErrorActionPreference = 'Stop'
$vbs = Join-Path $PSScriptRoot 'lancar-brani.vbs'
if (-not (Test-Path $vbs)) { throw "nao achei $vbs" }
New-Item -ItemType Directory -Force $Pasta | Out-Null
$atalho = Join-Path $Pasta 'B.R.A.N.I.lnk'
$s = (New-Object -ComObject WScript.Shell).CreateShortcut($atalho)
$s.TargetPath = "$env:WINDIR\System32\wscript.exe"
$s.Arguments = "//B //NoLogo `"$vbs`""
$s.WorkingDirectory = Split-Path $PSScriptRoot -Parent
$s.WindowStyle = 7
$s.Description = 'Sobe o B.R.A.N.I escondido no logon'
$s.Save()
Write-Host "Atalho criado: $atalho"
Write-Host "Para desfazer: remover-inicio.ps1"
