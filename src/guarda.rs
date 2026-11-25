//! GUARDA: de olho na seguranca do PC e do IP (braniac/guarda.py). SO OBSERVA E AVISA; nao muda firewall nem fecha portas.
//! Vigia: IP publico que mudou, porta nova escutando para a rede, porta de risco, firewall desligado, RDP ligado, Defender sem
//! protecao ou com assinatura velha, rajada de falhas de login. Alerta repetido so apos 12 h. Estado em dados/guarda.json.
//! De fora: rodar os comandos (constantes PS_*, so leitura) e consultar o IP publico: as saidas entram como texto em `montar`.
#![allow(dead_code)]
use serde_json::{json, Map, Value};
use std::path::Path;

pub const PS_PORTAS: &str = "Get-NetTCPConnection -State Listen | Where-Object { $_.LocalAddress -in '0.0.0.0','::' } | ForEach-Object { [pscustomobject]@{p=$_.LocalPort; n=(Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName} } | ConvertTo-Json -Compress";
pub const PS_FIREWALL: &str = "Get-NetFirewallProfile | Select-Object Name,Enabled | ConvertTo-Json -Compress";
pub const PS_RDP: &str = "(Get-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Terminal Server').fDenyTSConnections";
pub const PS_DEFENDER: &str = "Get-MpComputerStatus | Select-Object RealTimeProtectionEnabled,@{n='dias';e={((Get-Date)-$_.AntivirusSignatureLastUpdated).Days}} | ConvertTo-Json -Compress";
pub const PS_FALHAS: &str = "try { (Get-WinEvent -FilterHashtable @{LogName='Security';Id=4625;StartTime=(Get-Date).AddHours(-1)} -ErrorAction Stop | Measure-Object).Count } catch { 0 }";

const COOLDOWN_S: f64 = 12.0 * 3600.0;
const ESPERADAS: [i64; 13] = [135, 445, 5040, 7680, 2179, 49664, 49665, 49666, 49667, 49668, 49669, 49670, 49671]; // normais do Windows

pub fn porta_risco(p: i64) -> Option<&'static str> {
    Some(match p {
        21 => "FTP",
        23 => "Telnet",
        135 => "RPC",
        445 => "Compartilhamento de arquivos (SMB)",
        1433 => "SQL Server",
        3306 => "MySQL",
        3389 => "Area de Trabalho Remota (RDP)",
        5432 => "PostgreSQL",
        5900 => "VNC",
        6379 => "Redis",
        27017 => "MongoDB",
        9200 => "Elasticsearch",
        _ => return None,
    })
}

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

/// JSON do PowerShell: um objeto sozinho vira lista de um. Vazio ou ilegivel = None.
fn lista(saida: &str) -> Option<Vec<Value>> {
    match serde_json::from_str::<Value>(saida.trim()).ok()? {
        Value::Array(a) if !a.is_empty() => Some(a),
        v @ Value::Object(_) => Some(vec![v]),
        _ => None,
    }
}

/// Fotografia do estado de seguranca a partir das saidas dos comandos. Cada coleta falha sozinha (null) sem derrubar as outras.
pub fn montar(quando: f64, ip: Option<&str>, portas: &str, firewall: &str, rdp: &str, defender: &str, falhas: &str) -> Value {
    let portas = lista(portas).map(|l| {
        l.iter()
            .filter(|x| !x["p"].is_null())
            .map(|x| (x["p"].as_i64().map(|p| p.to_string()).unwrap_or_else(|| x["p"].to_string()), json!(x["n"].as_str().filter(|n| !n.is_empty()).unwrap_or("?"))))
            .collect::<Map<String, Value>>()
    });
    let fw = lista(firewall).map(|l| l.iter().filter(|x| !vdd(&x["Enabled"])).map(|x| x["Name"].clone()).collect::<Vec<Value>>());
    let rdp = match rdp.trim() {
        "0" => json!(true),
        "1" => json!(false),
        _ => Value::Null,
    };
    let def = serde_json::from_str::<Value>(defender.trim()).ok().filter(|d| d.as_object().is_some_and(|o| !o.is_empty()));
    let def = def.map(|d| json!({ "tempo_real": vdd(&d["RealTimeProtectionEnabled"]), "assinatura_dias": d["dias"] }));
    let f = falhas.trim();
    let falhas = if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) { f.parse::<i64>().ok() } else { None };
    json!({ "quando": quando, "ip": ip.map(str::trim).filter(|i| !i.is_empty()), "portas": portas, "firewall_desligado": fw, "rdp_ligado": rdp, "defender": def, "falhas_login_1h": falhas })
}

fn portas_ordenadas(est: &Value) -> Vec<(i64, String, String)> {
    let mut v: Vec<(i64, String, String)> =
        est["portas"].as_object().map(|o| o.iter().filter_map(|(p, n)| Some((p.parse().ok()?, p.clone(), n.as_str().unwrap_or("?").to_string()))).collect()).unwrap_or_default();
    v.sort();
    v
}

/// [(chave, texto)] de alertas a partir da foto atual e do que ja se conhecia (base). Puro.
pub fn avaliar(est: &Value, base: &Value) -> Vec<(String, String)> {
    let mut a = vec![];
    if let (Some(ip), Some(antes)) = (est["ip"].as_str().filter(|s| !s.is_empty()), base["ip"].as_str().filter(|s| !s.is_empty())) {
        if ip != antes {
            a.push((format!("ip:{ip}"), format!("🌐 Seu IP público mudou: {antes} → {ip}. Normal se o provedor é dinâmico ou se você ligou/desligou a VPN; se não foi nada disso, vale conferir.")));
        }
    }
    let conhecidas: Vec<&str> = base["portas_conhecidas"].as_array().map(|c| c.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    for (pi, p, nome) in portas_ordenadas(est) {
        match porta_risco(pi).filter(|_| pi != 135 && pi != 445) {
            Some(risco) => a.push((
                format!("risco:{pi}"),
                format!("⚠️ A porta {pi} ({risco}, processo {nome}) está aberta para a rede, não só para este PC. Se você não precisa que outros aparelhos acessem, o ideal é fazer o serviço escutar só em 127.0.0.1 ou bloquear no firewall. Não mexi em nada."),
            )),
            None if !conhecidas.is_empty() && !conhecidas.contains(&p.as_str()) && !ESPERADAS.contains(&pi) && pi < 49152 => {
                a.push((format!("nova:{pi}"), format!("🔎 Nova porta aberta para a rede: {pi} (processo {nome}). Se você não instalou nada agora, vale conferir.")))
            }
            None => {}
        }
    }
    if vdd(&est["firewall_desligado"]) {
        let perfis: Vec<&str> = est["firewall_desligado"].as_array().map(|f| f.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        a.push(("firewall".into(), format!("🚨 O Firewall do Windows está DESLIGADO no perfil: {}.", perfis.join(", "))));
    }
    if vdd(&est["rdp_ligado"]) {
        a.push(("rdp".into(), "🚨 A Área de Trabalho Remota (RDP) está LIGADA: é a porta de entrada mais atacada. Desligue se não usa.".into()));
    }
    let d = &est["defender"];
    if vdd(d) && !vdd(&d["tempo_real"]) {
        a.push(("defender".into(), "🚨 A proteção em tempo real do Windows Defender está DESLIGADA.".into()));
    }
    if d["assinatura_dias"].as_f64().unwrap_or(0.0) > 7.0 {
        a.push(("assinatura".into(), format!("🛡 As definições do antivírus estão com {} dias. Abra Segurança do Windows e atualize.", d["assinatura_dias"])));
    }
    if est["falhas_login_1h"].as_i64().unwrap_or(0) >= 10 {
        a.push(("login".into(), format!("🚨 {} tentativas de login falhas na última hora: alguém pode estar tentando entrar no PC.", est["falhas_login_1h"])));
    }
    a
}

fn ler(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("guarda.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// Roda uma checagem sobre a foto `est`. `enviar(texto)` entrega ao dono. Devolve os alertas enviados.
/// Primeira vez: so memoriza as portas (porta de risco avisa mesmo assim).
pub fn verificar(dados: &Path, est: &Value, agora: f64, enviar: &dyn Fn(&str)) -> Vec<String> {
    let base = ler(dados);
    let mut avisados = base["avisados"].as_object().cloned().unwrap_or_default();
    let mut enviados = vec![];
    for (chave, texto) in avaliar(est, &base) {
        if agora - avisados.get(&chave).and_then(Value::as_f64).unwrap_or(0.0) >= COOLDOWN_S {
            enviar(&texto);
            avisados.insert(chave, json!(agora));
            enviados.push(texto);
        }
    }
    avisados.retain(|_, v| agora - v.as_f64().unwrap_or(0.0) < 7.0 * 86400.0);
    let mut novo = base.clone();
    novo["avisados"] = Value::Object(avisados);
    if vdd(&est["ip"]) {
        novo["ip"] = est["ip"].clone();
    }
    if let Some(portas) = est["portas"].as_object() {
        let mut todas: Vec<String> = base["portas_conhecidas"].as_array().map(|c| c.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        todas.extend(portas.keys().cloned());
        todas.sort();
        todas.dedup();
        novo["portas_conhecidas"] = json!(todas);
    }
    novo["ultima"] = est.clone();
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join("guarda.json"), novo.to_string());
    enviados
}

/// Linha(s) para o bom dia.
pub fn resumo(dados: &Path) -> String {
    let b = ler(dados);
    let u = &b["ultima"];
    if !vdd(u) {
        return "🛡 Guarda de segurança: ainda sem leitura.".into();
    }
    let exp: Vec<String> = portas_ordenadas(u).into_iter().filter(|(pi, _, _)| porta_risco(*pi).is_some() && *pi != 135 && *pi != 445).map(|(_, p, n)| format!("{p}({n})")).collect();
    let fw = match u["firewall_desligado"].as_array() {
        Some(f) if f.is_empty() => "OK",
        Some(_) => "ATENÇÃO",
        None => "?",
    };
    let falhas = u["falhas_login_1h"].as_i64().map(|f| f.to_string()).unwrap_or_else(|| "?".into());
    let partes = [
        format!("IP {}", u["ip"].as_str().filter(|s| !s.is_empty()).unwrap_or("?")),
        format!("firewall {fw}"),
        format!("antivírus {}", if vdd(&u["defender"]["tempo_real"]) { "OK" } else { "ATENÇÃO" }),
        format!("falhas de login/h: {falhas}"),
    ];
    format!("🛡 Segurança: {}{}", partes.join(" · "), if exp.is_empty() { String::new() } else { format!("\n⚠️ Portas de risco abertas para a rede: {}", exp.join(", ")) })
}

/// Sites hospedados no celular-servidor (S20 FE, Termux + tunel da Cloudflare). Pingados 1 vez por dia, a partir das 9h.
pub const SITES: [(&str, &str); 3] = [
    ("Katupy (katupyoka.dpdns.org)", "https://katupyoka.dpdns.org"),
    ("Site (site.katupyoka.dpdns.org)", "https://site.katupyoka.dpdns.org"),
    ("Arquivos (arquivos.katupyoka.dpdns.org)", "https://arquivos.katupyoka.dpdns.org"),
];
const ARQ_SITES: &str = "guarda_sites.json";
const HORA_SITES: u32 = 9;

fn dia_dos_sites(dados: &Path) -> Option<i64> {
    serde_json::from_str::<Value>(&std::fs::read_to_string(dados.join(ARQ_SITES)).ok()?).ok()?["dia"].as_i64()
}

/// Ja esta na hora de pingar os sites hoje? (barato: so le o estado do dia)
pub fn sites_pendente(dados: &Path, dia: i64, hora: u32) -> bool {
    hora >= HORA_SITES && dia_dos_sites(dados) != Some(dia)
}

/// Pinga os sites UMA vez por dia (a partir das 9h). `ping` devolve o codigo HTTP; qualquer resposta abaixo de 500 conta como "no ar"
/// (401 e a tela de login). Quem nao responde ou devolve 5xx (530 = tunel caido) entra no aviso, que vai por `enviar`.
/// So OBSERVA: nao liga nem reinicia nada. None = nao era hora ou ja rodou hoje.
pub fn sites_do_dia(dados: &Path, dia: i64, hora: u32, ping: &dyn Fn(&str) -> Result<u16, String>, enviar: &dyn Fn(&str)) -> Option<Vec<String>> {
    if !sites_pendente(dados, dia, hora) {
        return None;
    }
    let mut falhas = vec![];
    for (nome, url) in SITES {
        match ping(url) {
            Ok(c) if c < 500 => {}
            Ok(c) => falhas.push(format!("{nome}: erro {c}")),
            Err(e) => falhas.push(format!("{nome}: sem resposta ({e})")),
        }
    }
    // grava o dia ANTES de avisar: se o aviso demorar, o laço de 1 minuto não repete a checagem
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join(ARQ_SITES), json!({ "dia": dia, "falhas": falhas }).to_string());
    if !falhas.is_empty() {
        // "urgente" faz o funil mandar na hora de dia; o resto do texto diz o que olhar
        enviar(&format!(
            "Urgente: o servidor do celular não está respondendo. Sem resposta hoje: {}. Veja se o celular está ligado, no Wi-Fi e com o Termux e o Tailscale abertos.",
            falhas.join("; ")
        ));
    }
    Some(falhas)
}

#[cfg(test)]
mod tests_sites {
    use super::*;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-guarda-sites-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn site_fora_avisa_uma_vez_no_dia_e_so_depois_das_9h() {
        let d = pasta("fora");
        let avisos = std::cell::RefCell::new(vec![]);
        let enviar = |t: &str| avisos.borrow_mut().push(t.to_string());
        let fora = |u: &str| if u.contains("arquivos") { Ok(530) } else { Ok(200) };
        assert!(sites_do_dia(&d, 100, 8, &fora, &enviar).is_none(), "antes das 9h não checa");
        let f = sites_do_dia(&d, 100, 9, &fora, &enviar).unwrap();
        assert_eq!(f.len(), 1);
        assert!(f[0].contains("Arquivos") && f[0].contains("530"));
        assert_eq!(avisos.borrow().len(), 1);
        assert!(crate::funil::urgente(&avisos.borrow()[0]), "o funil precisa mandar na hora");
        assert!(sites_do_dia(&d, 100, 15, &fora, &enviar).is_none(), "já rodou hoje");
        assert!(sites_do_dia(&d, 101, 9, &fora, &enviar).is_some(), "no dia seguinte checa de novo");
        assert_eq!(avisos.borrow().len(), 2);
    }

    #[test]
    fn tudo_no_ar_ou_login_401_nao_avisa_e_erro_de_rede_avisa() {
        let d = pasta("ok");
        let avisos = std::cell::RefCell::new(0);
        let enviar = |_: &str| *avisos.borrow_mut() += 1;
        assert_eq!(sites_do_dia(&d, 5, 10, &|_| Ok(401), &enviar).unwrap().len(), 0);
        assert_eq!(*avisos.borrow(), 0);
        let f = sites_do_dia(&d, 6, 10, &|_| Err("timeout".into()), &enviar).unwrap();
        assert_eq!(f.len(), 3);
        assert_eq!(*avisos.borrow(), 1, "um aviso só, com os três");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const T0: f64 = 1_800_000_000.0;

    fn dados(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-guarda-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn foto(muda: Value) -> Value {
        let mut b = json!({"ip": "1.1.1.1", "portas": {"5040": "svchost", "5432": "postgres"}, "firewall_desligado": [], "rdp_ligado": false,
                           "defender": {"tempo_real": true, "assinatura_dias": 1}, "falhas_login_1h": 0});
        for (k, v) in muda.as_object().unwrap() {
            b[k] = v.clone();
        }
        b
    }

    fn roda(d: &Path, est: Value, t: f64) -> Vec<String> {
        let env = std::cell::RefCell::new(vec![]);
        let dev = verificar(d, &est, T0 + t, &|s| env.borrow_mut().push(s.to_string()));
        assert_eq!(dev, *env.borrow());
        env.into_inner()
    }

    #[test]
    fn primeira_leitura_memoriza_mas_avisa_porta_de_risco() {
        let env = roda(&dados("a"), foto(json!({})), 0.0);
        assert!(env.len() == 1 && env[0].contains("5432") && env[0].contains("PostgreSQL") && env[0].contains("Não mexi em nada"));
    }

    #[test]
    fn alerta_nao_repete_dentro_de_12h_e_repete_depois() {
        let d = dados("b");
        assert_eq!(roda(&d, foto(json!({})), 1000.0).len(), 1);
        assert!(roda(&d, foto(json!({})), 2000.0).is_empty());
        assert_eq!(roda(&d, foto(json!({})), 1000.0 + 13.0 * 3600.0).len(), 1);
    }

    #[test]
    fn ip_mudou_avisa_so_na_mudanca() {
        let d = dados("c");
        roda(&d, foto(json!({"portas": {}})), 1000.0);
        let env = roda(&d, foto(json!({"ip": "2.2.2.2", "portas": {}})), 2000.0);
        assert!(env.iter().any(|e| e.contains("IP público mudou: 1.1.1.1 → 2.2.2.2")));
        assert!(roda(&d, foto(json!({"ip": "2.2.2.2", "portas": {}})), 3000.0).is_empty());
    }

    #[test]
    fn porta_nova_depois_do_baseline_avisa() {
        let d = dados("d");
        assert!(roda(&d, foto(json!({"portas": {"5040": "svchost"}})), 1000.0).is_empty());
        let env = roda(&d, foto(json!({"portas": {"5040": "svchost", "8080": "node", "50000": "x", "7680": "svchost"}})), 2000.0);
        assert!(env.len() == 1 && env[0].contains("Nova porta aberta") && env[0].contains("8080"));
    }

    #[test]
    fn firewall_rdp_defender_e_forca_bruta_sao_alertas() {
        let est = foto(json!({"portas": {}, "firewall_desligado": ["Public"], "rdp_ligado": true, "defender": {"tempo_real": false, "assinatura_dias": 20}, "falhas_login_1h": 25}));
        let texto = roda(&dados("e"), est, 0.0).join(" | ");
        for trecho in ["Firewall do Windows está DESLIGADO", "RDP", "tempo real do Windows Defender", "20 dias", "25 tentativas de login"] {
            assert!(texto.contains(trecho), "{trecho}");
        }
    }

    #[test]
    fn pc_saudavel_nao_gera_alerta_e_resumo_diz_ok() {
        let d = dados("f");
        assert_eq!(resumo(&d), "🛡 Guarda de segurança: ainda sem leitura.");
        assert!(roda(&d, foto(json!({"portas": {"5040": "svchost"}})), 0.0).is_empty());
        let r = resumo(&d);
        assert!(r.contains("firewall OK") && r.contains("antivírus OK") && r.contains("IP 1.1.1.1") && !r.contains("Portas de risco"));
        roda(&d, foto(json!({})), 10.0);
        assert!(resumo(&d).ends_with("Portas de risco abertas para a rede: 5432(postgres)"));
    }

    #[test]
    fn coletor_que_falha_nao_derruba() {
        let d = dados("g");
        let vazio = montar(T0, None, "", "erro", "", "", "sem acesso");
        for k in ["ip", "portas", "firewall_desligado", "rdp_ligado", "defender", "falhas_login_1h"] {
            assert!(vazio[k].is_null(), "{k}");
        }
        assert!(roda(&d, vazio, 0.0).is_empty());
        assert!(resumo(&d).contains("firewall ?") && resumo(&d).contains("antivírus ATENÇÃO"));
    }

    #[test]
    fn monta_a_foto_a_partir_das_saidas_dos_comandos() {
        let est = montar(
            T0,
            Some(" 9.9.9.9\n"),
            r#"{"p":3389,"n":null}"#,
            r#"[{"Name":"Domain","Enabled":1},{"Name":"Public","Enabled":0},{"Name":"Private","Enabled":false}]"#,
            "0\n",
            r#"{"RealTimeProtectionEnabled":true,"dias":9}"#,
            " 12 ",
        );
        assert_eq!(est["portas"], json!({"3389": "?"}));
        assert_eq!(est["firewall_desligado"], json!(["Public", "Private"]));
        assert_eq!((est["ip"].as_str(), est["rdp_ligado"].as_bool(), est["falhas_login_1h"].as_i64()), (Some("9.9.9.9"), Some(true), Some(12)));
        assert_eq!(est["defender"], json!({"tempo_real": true, "assinatura_dias": 9}));
        let chaves: Vec<String> = avaliar(&est, &json!({"ip": "9.9.9.9"})).into_iter().map(|a| a.0).collect();
        assert_eq!(chaves, ["risco:3389", "firewall", "rdp", "assinatura", "login"]);
        assert_eq!(montar(T0, None, "[]", "", "1", "{}", "")["rdp_ligado"], json!(false));
    }
}
