//! Checagem de seguranca do PC e da rede (braniac/seguranca.py). SO LE: nao muda firewall, contas, registro nem roteador, e nao tenta
//! senha de nada. Cada achado vem com a correcao sugerida; quem corrige e o dono.
//! A saida do PowerShell (constante PS, so leitura) entra como texto; aqui ficam o parse, as regras e o relatorio.
//! De fora: rodar o PowerShell e varrer as pastas atras de client_secret (quem chama passa a lista das pastas onde achou).
#![allow(dead_code)]
use crate::tempo::{Agora, Data};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Coleta somente leitura; a ultima linha da saida e um JSON.
pub const PS: &str = r#"
$o=@{}
$d=Get-MpComputerStatus; $o.defender=@($d.AntivirusEnabled,$d.RealTimeProtectionEnabled,$d.AntivirusSignatureAge,$d.IsTamperProtected)
$o.firewall=@(Get-NetFirewallProfile|?{-not $_.Enabled}|%{$_.Name})
$o.rdp=((Get-ItemProperty 'HKLM:\System\CurrentControlSet\Control\Terminal Server').fDenyTSConnections -eq 0)
$o.smb1=[bool](Get-SmbServerConfiguration).EnableSMB1Protocol
$o.guest=[bool](Get-LocalUser|?{$_.SID -like '*-501' -and $_.Enabled})
$o.autologon=((Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon').AutoAdminLogon -eq '1')
$o.uac=((Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System').EnableLUA -eq 1)
$o.publica=@(Get-NetConnectionProfile|?{$_.NetworkCategory -eq 'Public'}|%{$_.Name})
$o.smb_publico=[bool](Get-NetFirewallRule -Direction Inbound -Enabled True -Action Allow -ea 0|?{$_.Profile -match 'Public|Any'}|%{($_|Get-NetFirewallPortFilter).LocalPort}|?{$_ -eq '445'})
$o.atualizado_em=(Get-HotFix|sort InstalledOn -Desc|select -First 1).InstalledOn.ToString('yyyy-MM-dd')
$o.escuta=@(Get-NetTCPConnection -State Listen|?{$_.LocalAddress -in '0.0.0.0','::'}|%{"$($_.LocalPort)"}|sort -u)
$gw=(Get-NetRoute -DestinationPrefix 0.0.0.0/0|select -First 1).NextHop; $o.gw=$gw
$o.gw_abertas=@(foreach($p in 21,22,23,80,443,7547,8080){$c=New-Object Net.Sockets.TcpClient;$r=$c.BeginConnect($gw,$p,$null,$null);if($r.AsyncWaitHandle.WaitOne(600) -and $c.Connected){$p};$c.Close()})
$ini=(Get-Date).AddDays(-7)
$o.falhas=$null; try{$o.falhas=@(Get-WinEvent -FilterHashtable @{LogName='Security';Id=4625;StartTime=$ini} -ea Stop).Count}catch{if($_.Exception.Message -match 'Nenhum evento|No events'){$o.falhas=0}}
$o.rdp_entrou=$null; try{$o.rdp_entrou=@(Get-WinEvent -FilterHashtable @{LogName='Security';Id=4624;StartTime=$ini} -ea Stop|?{$_.Properties[8].Value -in 3,10 -and $_.Properties[18].Value -notmatch '^(127\.|::1|-|192\.168\.)'}).Count}catch{if($_.Exception.Message -match 'Nenhum evento|No events'){$o.rdp_entrou=0}}
$o.ameacas=@(Get-MpThreatDetection -ea 0|?{$_.InitialDetectionTime -gt (Get-Date).AddDays(-30)}|%{"$($_.ThreatID)"}|sort -u)
$o.contas=@(Get-LocalUser|?{$_.Enabled}|%{$_.Name}|sort)
$o.admins=@(Get-LocalGroupMember -SID S-1-5-32-544 -ea 0|%{$_.Name}|sort)
$o.externas_suspeitas=@(Get-NetTCPConnection -State Established -ea 0|?{$_.RemoteAddress -notmatch '^(127\.|::1|192\.168\.|10\.|0\.|fe80)'}|%{$pr=Get-Process -Id $_.OwningProcess -ea 0; if($pr.Path -match '\\(Temp|Downloads|Users\\Public)\\'){"$($pr.ProcessName)->$($_.RemoteAddress):$($_.RemotePort)"}}|sort -u)
$o|ConvertTo-Json -Compress
"#;

/// (gravidade, achado, correcao). Gravidade: alta, media, baixa.
pub type Achado = (&'static str, String, String);

/// Verdade do Python: null, false, 0, "" e lista/objeto vazio sao falsos.
fn vdd(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn textos(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect()).unwrap_or_default()
}

/// A ultima linha da saida do PowerShell e o JSON da coleta. None = coleta falhou (nunca vira "tudo certo").
pub fn ler_coleta(saida: &str) -> Option<Value> {
    serde_json::from_str::<Value>(saida.trim().lines().last()?).ok().filter(Value::is_object)
}

pub fn avaliar(d: &Value, hoje: Data, dados: &Path) -> Vec<Achado> {
    let alta = |t: String, c: &str| -> Achado { ("alta", t, c.to_string()) };
    let mut a: Vec<Achado> = vec![];
    let def = &d["defender"];
    if !vdd(&def[0]) || !vdd(&def[1]) {
        a.push(alta("Antivirus do Windows desligado.".into(), "Ligar a Seguranca do Windows (protecao em tempo real)."));
    }
    let idade = def[2].as_f64().unwrap_or(0.0);
    if idade > 7.0 {
        a.push(("media", format!("Definicoes do antivirus com {idade} dias."), "Atualizar pela Seguranca do Windows.".into()));
    }
    for nome in textos(&d["firewall"]) {
        a.push(alta(format!("Firewall desligado no perfil {nome}."), "Ligar o Firewall do Windows nesse perfil."));
    }
    if vdd(&d["rdp"]) {
        a.push(alta("Area de Trabalho Remota (RDP) ligada.".into(), "Desligar em Configuracoes > Sistema > Area de Trabalho Remota, se nao usa."));
    }
    if vdd(&d["smb1"]) {
        a.push(alta("SMB1 (protocolo antigo e inseguro) ligado.".into(), "Desativar o recurso SMB 1.0 do Windows."));
    }
    if vdd(&d["autologon"]) {
        a.push(alta("Login automatico do Windows ligado (senha guardada no registro).".into(), "Desligar o login automatico."));
    }
    if d.get("uac").is_some_and(|u| !vdd(u)) {
        a.push(alta("UAC desligado.".into(), "Ligar o Controle de Conta de Usuario."));
    }
    if vdd(&d["guest"]) {
        a.push(("media", "Conta Convidado ativa (sem senha).".into(), "Desativar: 'net user Convidado /active:no' num terminal de administrador.".into()));
    }
    if vdd(&d["smb_publico"]) && vdd(&d["publica"]) {
        a.push((
            "media",
            format!("Compartilhamento de arquivos (porta 445) liberado para qualquer endereco em rede Publica ({}).", textos(&d["publica"]).join(", ")),
            "Desativar a regra 'Compartilhamento de Arquivo e Impressora (Restritivo)' ou deixar so para a sub-rede local.".into(),
        ));
    }
    if let Some(em) = d["atualizado_em"].as_str().filter(|e| !e.is_empty() && **e < *Data(hoje.0 - 45).iso()) {
        a.push(("media", format!("Ultima atualizacao do Windows instalada em {em}."), "Rodar o Windows Update.".into()));
    }
    let abertas: Vec<i64> = d["gw_abertas"].as_array().map(|x| x.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
    let gw = d["gw"].as_str().unwrap_or("None");
    for p in &abertas {
        if [21, 23, 7547].contains(p) {
            a.push(alta(format!("Roteador {gw} com porta {p} (FTP/Telnet/TR-069) aberta na rede interna."), "Desligar esse servico no painel do roteador."));
        } else if *p == 80 && !abertas.contains(&443) {
            a.push(("baixa", "Painel do roteador so em HTTP (sem criptografia).".into(), "Usar o painel so de casa e trocar a senha de admin do roteador.".into()));
        }
    }
    a.extend(invasao(d, dados));
    a.push((
        "baixa",
        "Painel do roteador: nao tento senhas nem entro nele. Confira voce mesmo se a senha de admin nao e a de fabrica e se a Wi-Fi usa WPA2/WPA3 e WPS desligado.".into(),
        format!("Acessar http://{gw} de dentro de casa."),
    ));
    a
}

/// Sinais de que alguem tentou ou conseguiu entrar (7 dias). Falta de permissao no log = sem dado, nunca "tudo certo".
/// Contas e admins vistos na 1a checagem ficam em dados/seguranca_base.json: o que aparecer depois e novidade.
pub fn invasao(d: &Value, dados: &Path) -> Vec<Achado> {
    let mut a: Vec<Achado> = vec![];
    let falhas = d["falhas"].as_i64();
    if falhas.unwrap_or(0) >= 20 {
        a.push(("media", format!("{} tentativas de login que falharam nos ultimos 7 dias.", falhas.unwrap_or(0)), "Ver o Visualizador de Eventos (Seguranca, ID 4625) e trocar a senha se nao foram voce.".into()));
    }
    let entrou = d["rdp_entrou"].as_i64().unwrap_or(0);
    if entrou > 0 {
        a.push(("alta", format!("{entrou} login(s) vindo de fora da rede local nos ultimos 7 dias."), "Conferir o IP no Visualizador de Eventos (ID 4624); se nao foi voce, trocar a senha e desligar o acesso remoto.".into()));
    }
    let ameacas = textos(&d["ameacas"]);
    if !ameacas.is_empty() {
        a.push(("alta", format!("O Defender detectou {} ameaca(s) nos ultimos 30 dias.", ameacas.len()), "Abrir Seguranca do Windows > Protecao contra virus > Historico de protecao.".into()));
    }
    let ext = textos(&d["externas_suspeitas"]);
    if !ext.is_empty() {
        a.push((
            "alta",
            format!("Programa rodando de Temp/Downloads/Public com conexao para a internet: {}", ext.iter().take(5).cloned().collect::<Vec<_>>().join(", ")),
            "Identificar o programa no Gerenciador de Tarefas; se nao reconhece, desconectar da rede e rodar uma verificacao completa.".into(),
        ));
    }
    let ordenado = |v: &Value| {
        let mut t = textos(v);
        t.sort();
        t
    };
    let atual = [("contas", "Conta ativa nova", ordenado(&d["contas"])), ("admins", "Novo administrador", ordenado(&d["admins"]))];
    let arq = dados.join("seguranca_base.json");
    match std::fs::read_to_string(&arq).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        Some(base) => {
            for (chave, nome, lista) in &atual {
                let antes = textos(&base[*chave]);
                let mut novos: Vec<&String> = lista.iter().filter(|x| !antes.contains(x)).collect();
                novos.dedup();
                for x in novos {
                    a.push(("alta", format!("{nome} no PC: {x}."), "Se nao foi voce, desativar a conta e trocar as senhas.".into()));
                }
            }
        }
        None if !atual[0].2.is_empty() => {
            let _ = std::fs::create_dir_all(dados);
            let _ = std::fs::write(&arq, json!({ "contas": atual[0].2, "admins": atual[1].2 }).to_string());
        }
        None => {}
    }
    if falhas.is_none() {
        a.push(("baixa", "Sem permissao para ler o log de Seguranca: tentativas de login nao foram checadas.".into(), "Rodar o Braniac como administrador, se quiser essa checagem.".into()));
    }
    a
}

/// Segredos soltos em pastas comuns (so nomes, nunca o conteudo). `pastas_com_segredo` = nome da pasta de cada client_secret*.json achado.
pub fn arquivos_sensiveis(pastas_com_segredo: &[String], env_existe: bool) -> Vec<Achado> {
    let mut a: Vec<Achado> = pastas_com_segredo
        .iter()
        .map(|p| ("media", format!("Segredo do Google (client_secret) solto em {p}/."), "Mover para uma pasta protegida ou apagar se nao usa; nunca enviar a ninguem.".to_string()))
        .collect();
    if env_existe {
        a.push(("baixa", "braniac/.env guarda chaves em texto puro (esta fora do git).".into(), "Manter fora de nuvem/backup publico.".into()));
    }
    a
}

fn ordem(g: &str) -> u8 {
    ["alta", "media", "baixa"].iter().position(|x| *x == g).unwrap_or(3) as u8
}

/// Texto do relatorio, do mais grave para o menos grave.
pub fn relatorio(achados: &[Achado], d: &Value, agora: &Agora) -> String {
    let mut ach: Vec<&Achado> = achados.iter().collect();
    ach.sort_by_key(|x| ordem(x.0));
    let (y, _, _) = agora.data.partes();
    let mut l = vec![format!("# Seguranca do PC e da rede — {}/{y} {}", agora.data.dia_mes(), agora.hm()), String::new(), "Checagem somente de leitura. Nada foi alterado.".into(), String::new()];
    l.extend(ach.iter().map(|(g, t, c)| format!("- **{}** — {t}\n  - Correcao: {c}", g.to_uppercase())));
    l.push(String::new());
    l.push(format!(
        "Portas escutando em todas as interfaces: {} (a internet so chega nelas se o roteador abrir porta; o firewall bloqueia as que nao tem regra).",
        textos(&d["escuta"]).join(", ")
    ));
    l.join("\n") + "\n"
}

/// Avalia a saida do PowerShell, grava `pasta_relatorios/seguranca-AAAA-MM-DD.md` e avisa pelo `registrar`. Err = coleta ilegivel.
pub fn rodar(saida_ps: &str, pastas_com_segredo: &[String], env_existe: bool, dados: &Path, pasta_relatorios: &Path, agora: &Agora, registrar: &dyn Fn(&str)) -> Result<(Vec<Achado>, PathBuf), String> {
    let d = ler_coleta(saida_ps).ok_or("coleta de seguranca ilegivel: nada foi avaliado")?;
    let mut achados = avaliar(&d, agora.data, dados);
    achados.extend(arquivos_sensiveis(pastas_com_segredo, env_existe));
    let arq = pasta_relatorios.join(format!("seguranca-{}.md", agora.data.iso()));
    std::fs::create_dir_all(pasta_relatorios).and_then(|_| std::fs::write(&arq, relatorio(&achados, &d, agora))).map_err(|e| e.to_string())?;
    let n = |g: &str| achados.iter().filter(|x| x.0 == g).count();
    registrar(&format!(
        "🔒 Seguranca do PC e da rede: {} alta(s), {} media(s), {} baixa(s). Detalhes em relatorios/{}.",
        n("alta"),
        n("media"),
        n("baixa"),
        arq.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()
    ));
    Ok((achados, arq))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dados(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-seguranca-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn saudavel() -> Value {
        json!({"defender": [true, true, 1, true], "firewall": [], "rdp": false, "smb1": false, "guest": false, "autologon": false, "uac": true, "publica": [],
               "smb_publico": false, "atualizado_em": "2026-11-10", "escuta": ["135", "445"], "gw": "192.168.0.1", "gw_abertas": [80, 443],
               "falhas": 0, "rdp_entrou": 0, "ameacas": [], "contas": ["ana"], "admins": ["ana"], "externas_suspeitas": []})
    }

    #[test]
    fn invasao_sinais() {
        let t = dados("inv");
        let d = json!({"falhas": 50, "rdp_entrou": 2, "ameacas": ["x"], "externas_suspeitas": ["a.exe->1.2.3.4:80"], "contas": ["ana"], "admins": ["ana"]});
        let g: Vec<&str> = invasao(&d, &t).iter().map(|x| x.0).collect();
        assert_eq!((g.iter().filter(|x| **x == "alta").count(), g.iter().filter(|x| **x == "media").count()), (3, 1));
        let d2 = json!({"falhas": 0, "rdp_entrou": 0, "ameacas": [], "externas_suspeitas": [], "contas": ["ana", "intruso"], "admins": ["ana", "intruso"]});
        let t2: Vec<String> = invasao(&d2, &t).into_iter().map(|x| x.1).collect();
        assert_eq!(t2, ["Conta ativa nova no PC: intruso.", "Novo administrador no PC: intruso."]);
    }

    #[test]
    fn sem_permissao_nao_diz_que_esta_tudo_certo() {
        let t = dados("perm");
        assert!(invasao(&json!({"falhas": null}), &t).iter().any(|x| x.1.contains("Sem permissao")));
        assert!(!t.join("seguranca_base.json").exists()); // sem contas na coleta, nao grava base vazia
        assert_eq!(ler_coleta("aviso qualquer\nnao e json"), None);
        assert!(rodar("erro do powershell", &[], false, &t, &t.join("rel"), &Agora { data: Data(20000), hora: 3, min: 0, seg: 0 }, &|_| panic!("nao avisa")).is_err());
    }

    #[test]
    fn pc_saudavel_so_tem_o_lembrete_do_roteador_e_pc_ruim_lista_tudo() {
        let t = dados("aval");
        let hoje = Data::civil(2026, 11, 18).unwrap();
        let bom = avaliar(&saudavel(), hoje, &t);
        assert_eq!(bom.len(), 1);
        assert!(bom[0].0 == "baixa" && bom[0].2 == "Acessar http://192.168.0.1 de dentro de casa.");
        let mut r = saudavel();
        for (k, v) in [("defender", json!([true, false, 20, true])), ("firewall", json!(["Public"])), ("rdp", json!(true)), ("smb1", json!(true)), ("guest", json!(true)),
                       ("autologon", json!(true)), ("uac", json!(false)), ("publica", json!(["Rede 2"])), ("smb_publico", json!(true)), ("atualizado_em", json!("2026-09-01")), ("gw_abertas", json!([23, 80]))] {
            r[k] = v;
        }
        let ruim = avaliar(&r, hoje, &t);
        let texto = ruim.iter().map(|x| x.1.as_str()).collect::<Vec<_>>().join(" | ");
        for trecho in ["Antivirus do Windows desligado", "com 20 dias", "perfil Public", "RDP", "SMB1", "Login automatico", "UAC desligado", "Convidado", "(Rede 2)", "em 2026-09-01", "porta 23", "so em HTTP"] {
            assert!(texto.contains(trecho), "{trecho}: {texto}");
        }
        assert_eq!(ruim.iter().filter(|x| x.0 == "alta").count(), 7);
        // uac ausente = nao acusa; atualizacao ha 44 dias = nao acusa
        let mut s = saudavel();
        s.as_object_mut().unwrap().remove("uac");
        s["atualizado_em"] = json!(Data(hoje.0 - 44).iso());
        assert_eq!(avaliar(&s, hoje, &t).len(), 1);
    }

    #[test]
    fn roda_grava_relatorio_ordenado_e_avisa_a_contagem() {
        let t = dados("rodar");
        let agora = Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 3, min: 7, seg: 0 };
        let mut d = saudavel();
        d["rdp"] = json!(true);
        let avisos = std::cell::RefCell::new(vec![]);
        let saida = format!("ruido\n{d}\n");
        let (ach, arq) = rodar(&saida, &["Downloads".to_string()], true, &t, &t.join("relatorios"), &agora, &|s| avisos.borrow_mut().push(s.to_string())).unwrap();
        assert_eq!(ach.len(), 4);
        assert!(avisos.borrow()[0].contains("1 alta(s), 1 media(s), 2 baixa(s). Detalhes em relatorios/seguranca-2026-11-18.md"));
        let txt = std::fs::read_to_string(arq).unwrap();
        assert!(txt.starts_with("# Seguranca do PC e da rede — 18/11/2026 03:07\n\nChecagem somente de leitura. Nada foi alterado.\n\n- **ALTA** — Area de Trabalho Remota"));
        assert!(txt.find("**MEDIA**").unwrap() < txt.find("**BAIXA**").unwrap() && txt.contains("interfaces: 135, 445 ("));
    }
}
