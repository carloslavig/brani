' Lancador ESCONDIDO do B.R.A.N.I (sem janela, sem piscar). Nao apaga nem substitui o ligar-brani.bat.
' O que o .bat/PowerShell nao fazia bem: (1) o E: ainda pode nao estar pronto no logon; (2) abrir cmd escondido por PowerShell deixa o
' filho preso ao processo que o criou; (3) um segundo braniac.exe so panica ("porta ocupada") e ainda disputa o servidor.log.
' Aqui: espera o E: e o exe (ate ~3 min), nao sobe se ja ha um braniac.exe deste caminho, e roda tudo com a janela em modo 0.
' Uso: wscript lancar-brani.vbs          (sobe)
'      wscript lancar-brani.vbs /seco    (so escreve dados\lancador-teste.txt com o que faria; nao sobe nada)
Option Explicit
Dim sh, fso, env, exe, dados, seco, i, cmd, log, f, svc, p, ja
Set sh = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")
exe = "E:\braniac-rs\target\release\braniac.exe"
dados = "E:\braniac-rs\dados"
log = dados & "\servidor.log"
seco = WScript.Arguments.Named.Exists("seco")

For i = 1 To 36
  If fso.FileExists(exe) And fso.FolderExists(dados) Then Exit For
  If seco Then Exit For
  WScript.Sleep 5000
Next
If Not (fso.FileExists(exe) And fso.FolderExists(dados)) Then WScript.Quit 2

ja = False
Set svc = GetObject("winmgmts:root\cimv2")
For Each p In svc.ExecQuery("Select * From Win32_Process Where Name = 'braniac.exe'")
  If Not IsNull(p.ExecutablePath) Then
    If LCase(p.ExecutablePath) = LCase(exe) Then ja = True
  End If
Next

' O wsl.exe/powershell que o braniac.exe anterior deixou rodando herdam o servidor.log e o travam: nesse caso usa um log com a hora.
On Error Resume Next
Set f = fso.OpenTextFile(log, 8, True)
If Err.Number <> 0 Then
  Err.Clear
  log = dados & "\servidor-" & Year(Now) & Right("0" & Month(Now), 2) & Right("0" & Day(Now), 2) & "-" & Right("0" & Hour(Now), 2) & Right("0" & Minute(Now), 2) & ".log"
Else
  f.Close
End If
On Error GoTo 0
Set env = sh.Environment("Process")
env("BRANIAC_DADOS") = dados
env("BRANIAC_ENV") = "%USERPROFILE%\Projetos\Agente\braniac\.env"
env("BRANIAC_PACOTE") = "C:\Cliente\PACOTE-PRODUCAO-COMPLETA"
env("BRANIAC_GUIAS") = "C:\Cliente\guias-estados\estados"
env("RUST_BACKTRACE") = "1"
cmd = "cmd /c " & exe & " >> " & log & " 2>&1"  ' caminhos sem espaco: sem aspas aninhadas (com elas o cmd nao subia o exe)

If seco Then
  Set f = fso.CreateTextFile(dados & "\lancador-teste.txt", True)
  f.WriteLine "ja_rodando=" & ja
  f.WriteLine "comando=" & cmd
  f.WriteLine "BRANIAC_DADOS=" & env("BRANIAC_DADOS")
  f.Close
  ' prova que um filho escondido pelo Run modo 0 continua vivo depois que este script acaba
  sh.Run "cmd /c ping -n 30 127.0.0.1 > """ & dados & "\lancador-teste-filho.txt""", 0, False
  WScript.Quit 0
End If

If ja Then WScript.Quit 0
sh.Run cmd, 0, False
