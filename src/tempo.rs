//! Data e hora locais sem crate. As contas de calendario sao as mesmas do estoque da loja (braniac1/estoque-rs).

/// Dias desde 01/01/1970.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Data(pub i64);

fn dias_de_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_de_dias(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

impl Data {
    /// None para data que nao existe (31/02, mes 13).
    pub fn civil(y: i64, m: i64, d: i64) -> Option<Data> {
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        let x = Data(dias_de_civil(y, m, d));
        (x.partes() == (y, m, d)).then_some(x)
    }

    pub fn partes(self) -> (i64, i64, i64) {
        civil_de_dias(self.0)
    }

    /// 0 = segunda ... 6 = domingo.
    pub fn semana(self) -> usize {
        (self.0 + 3).rem_euclid(7) as usize
    }

    pub fn iso(self) -> String {
        let (y, m, d) = self.partes();
        format!("{y:04}-{m:02}-{d:02}")
    }

    pub fn de_iso(s: &str) -> Option<Data> {
        let mut p = s.trim().splitn(3, '-').map(|x| x.parse::<i64>().ok());
        Data::civil(p.next()??, p.next()??, p.next()??)
    }

    pub fn dia_mes(self) -> String {
        let (_, m, d) = self.partes();
        format!("{d:02}/{m:02}")
    }

    /// 1 = primeiro de janeiro.
    pub fn dia_do_ano(self) -> i64 {
        self.0 - dias_de_civil(self.partes().0, 1, 1) + 1
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Agora {
    pub data: Data,
    pub hora: u32,
    pub min: u32,
    pub seg: u32,
}

impl Agora {
    pub fn hm(&self) -> String {
        format!("{:02}:{:02}", self.hora, self.min)
    }

    pub fn segundos(&self) -> i64 {
        (self.hora * 3600 + self.min * 60 + self.seg) as i64
    }

    /// "AAAAMMDD-HHMMSS", comeco do id de cada pedido de rotina.
    pub fn carimbo(&self) -> String {
        format!("{}-{:02}{:02}{:02}", self.data.iso().replace('-', ""), self.hora, self.min, self.seg)
    }
}

#[cfg(windows)]
pub fn agora() -> Agora {
    #[repr(C)]
    #[derive(Default)]
    struct St {
        ano: u16,
        mes: u16,
        _semana: u16,
        dia: u16,
        hora: u16,
        min: u16,
        seg: u16,
        _ms: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(st: *mut St);
    }
    let mut s = St::default();
    // SAFETY: GetLocalTime so preenche a estrutura que recebe.
    unsafe { GetLocalTime(&mut s) };
    Agora { data: Data(dias_de_civil(s.ano as i64, s.mes as i64, s.dia as i64)), hora: s.hora as u32, min: s.min as u32, seg: s.seg as u32 }
}

// shortcut: fora do Windows a hora sai em UTC; trocar por localtime quando o Braniac rodar em outro sistema
#[cfg(not(windows))]
pub fn agora() -> Agora {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let r = s.rem_euclid(86400) as u32;
    Agora { data: Data(s.div_euclid(86400)), hora: r / 3600, min: r / 60 % 60, seg: r % 60 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendario_vai_e_volta_e_recusa_data_que_nao_existe() {
        assert_eq!(Data::civil(1970, 1, 1), Some(Data(0)));
        assert_eq!(Data(0).semana(), 3); // quinta
        let d = Data::civil(2026, 11, 17).unwrap();
        assert_eq!((d.semana(), d.iso(), d.dia_mes()), (1, "2026-11-17".to_string(), "17/11".to_string())); // terca
        assert_eq!(Data::de_iso("2028-02-29").map(|d| d.partes()), Some((2028, 2, 29)));
        assert_eq!(Data(d.0 + 14).iso(), "2026-12-01");
        assert_eq!((Data::civil(2026, 1, 1).unwrap().dia_do_ano(), Data::civil(2028, 12, 31).unwrap().dia_do_ano()), (1, 366));
        for ruim in [(2026, 2, 29), (2026, 13, 1), (2026, 4, 31), (2026, 0, 5)] {
            assert_eq!(Data::civil(ruim.0, ruim.1, ruim.2), None);
        }
        assert_eq!(Data::de_iso("ontem"), None);
        let a = agora();
        assert!(a.data > Data::civil(2026, 1, 1).unwrap() && a.hora < 24);
    }
}
