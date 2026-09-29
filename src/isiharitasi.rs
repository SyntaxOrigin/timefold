//! Isı skoru (değişim büyüklüğü × derinlik) ve bantlama.
//!
//! Isı haritası "hangi klasör dikkat çekiyor" sorusuna yanıt verir. Skor iki
//! çarpanın çarpımıdır:
//!
//! ```text
//! ısı = |değişim baytı| × (derinlik + 1)
//! ```
//!
//! **Neden derinlik çarpanı?** Derin bir ağacın altındaki büyüme, üst seviyedeki
//! aynı büyümmeden daha çok ilgi gerektirir: kök klasörün kendisi zaten
//! büyüdüğü için alt klasörün büyümesi "yeni" bilgidir. Ağırlık, `derinlik`
//! 0 tabanlıdır; böylece kök (`derinlik = 0`) çarpanı 1 alır.
//!
//! Skor kararlıdır: eşit skorda yol adı sıralaması kararı verir, bu yüzden aynı
//! veriyle iki çalıştırma aynı listeyi üretir (raporın v1 kabul kriteri).

use std::collections::BTreeMap;

use crate::depo::Depo;
use crate::hata::Sonuc;
use crate::kayit::Degisim;
use crate::yol::Yol;

/// Isı bantı (0 = soğuk, 4 = çok sıcak).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bant {
    /// Değişim yok veya ihmal edilebilir.
    Soguk,
    /// Küçük değişim.
    Ilik,
    /// Orta değişim.
    Sicak,
    /// Büyük değişim.
    Cok,
    /// Çok büyük değişim.
    Alasin,
}

impl Bant {
    /// Bantın metinsel karşılığı.
    pub fn etiket(self) -> &'static str {
        match self {
            Bant::Soguk => "soguk",
            Bant::Ilik => "ilik",
            Bant::Sicak => "sicak",
            Bant::Cok => "cok",
            Bant::Alasin => "alasin",
        }
    }

    /// En büyük skora göre bandı hesaplar.
    ///
    /// En büyük skor 0 ise her şey `soguk`tur; aksi hâlde oransal dört eşik
    /// kullanılır. Mutlak eşik yerine oran kullanılması, farklı ölçekteki
    /// depoların karşılaştırılabilir kalmasını sağlar.
    pub fn hesapla(skor: u64, en_buyuk: u64) -> Bant {
        if en_buyuk == 0 || skor == 0 {
            return Bant::Soguk;
        }
        let yuzde = skor.saturating_mul(100) / en_buyuk;
        match yuzde {
            0 => Bant::Soguk,
            1..=20 => Bant::Ilik,
            21..=50 => Bant::Sicak,
            51..=80 => Bant::Cok,
            _ => Bant::Alasin,
        }
    }
}

/// Isı haritasının tek bir satırı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsiSatiri {
    /// İncelenen yol.
    pub yol: Yol,
    /// Dönem başındaki boyut.
    pub onceki_bayt: u64,
    /// Dönem sonundaki boyut.
    pub yeni_bayt: u64,
    /// Dönem içindeki değişim.
    pub fark: i64,
    /// Yolun derinliği.
    pub derinlik: usize,
    /// Isı skoru.
    pub skor: u64,
    /// Bant.
    pub bant: Bant,
}

impl IsiSatiri {
    /// Değişimin büyüklüğünü (mutlak değeri) döndürür.
    pub fn degisim_miktari(&self) -> u64 {
        self.fark.unsigned_abs()
    }
}

/// Belirtilen iki anlık görüntü arasındaki ısı haritasını üretir.
///
/// Yalnızca iki anlık görüntü okunur; tüm geçmiş belleğe alınmaz.
pub fn harita_olustur(depo: &Depo, onceki: &str, yeni: &str) -> Sonuc<Vec<IsiSatiri>> {
    let eski_durum = durum_oluştur(depo, onceki)?;
    let yeni_durum = durum_oluştur(depo, yeni)?;

    let mut satirlar: Vec<IsiSatiri> = Vec::new();
    let mut yollar: Vec<Yol> = eski_durum.keys().cloned().collect();
    for yol in yeni_durum.keys() {
        if !yollar.contains(yol) {
            yollar.push(yol.clone());
        }
    }
    yollar.sort();
    yollar.dedup();

    for yol in yollar {
        let eski_bayt = eski_durum.get(&yol).copied().unwrap_or(0);
        let yeni_bayt = yeni_durum.get(&yol).copied().unwrap_or(0);
        if eski_bayt == yeni_bayt {
            continue;
        }
        let fark = yeni_bayt as i64 - eski_bayt as i64;
        let derinlik = yol.derinlik();
        let skor = (fark.unsigned_abs()).saturating_mul((derinlik + 1) as u64);
        satirlar.push(IsiSatiri {
            yol,
            onceki_bayt: eski_bayt,
            yeni_bayt,
            fark,
            derinlik,
            skor,
            bant: Bant::Soguk,
        });
    }

    let en_buyuk = satirlar.iter().map(|s| s.skor).max().unwrap_or(0);
    for satir in satirlar.iter_mut() {
        satir.bant = Bant::hesapla(satir.skor, en_buyuk);
    }
    // Kararlı sıralama: skora göre azalan, eşitlikte yola göre artan.
    satirlar.sort_by(|a, b| b.skor.cmp(&a.skor).then(a.yol.cmp(&b.yol)));
    Ok(satirlar)
}

/// Belirtilen anlık görüntüye kadar zinciri ileri uygulayıp yol → boyut haritası
/// üretir.
pub fn durum_oluştur(depo: &Depo, kimlik: &str) -> Sonuc<BTreeMap<Yol, u64>> {
    let mut durum: BTreeMap<Yol, u64> = BTreeMap::new();
    for bilgi in depo.var_olan_bilgiler() {
        for fark in depo.fark_kayitlari(&bilgi.kimlik)? {
            match fark {
                Degisim::Eklendi { kayit } => {
                    durum.insert(kayit.yol.clone(), kayit.bayt);
                }
                Degisim::Silindi { yol, .. } => {
                    durum.remove(&yol);
                }
                Degisim::Degisti { yol, yeni_bayt, .. } => {
                    durum.insert(yol, yeni_bayt);
                }
                Degisim::AdDegisti {
                    eski_yol,
                    yeni_yol,
                    bayt,
                    ..
                } => {
                    durum.remove(&eski_yol);
                    durum.insert(yeni_yol, bayt);
                }
            }
        }
        if bilgi.kimlik == kimlik {
            break;
        }
    }
    Ok(durum)
}

/// Son iki anlık görüntü arasındaki ısı haritasını üretir.
pub fn son_iki_anlik_goruntu(depo: &Depo) -> Sonuc<Vec<IsiSatiri>> {
    let bilgiler = depo.var_olan_bilgiler();
    if bilgiler.len() < 2 {
        return Ok(Vec::new());
    }
    let n = bilgiler.len();
    harita_olustur(depo, &bilgiler[n - 2].kimlik, &bilgiler[n - 1].kimlik)
}

/// Isı haritasını verilen üst yolün altına indirger ve yeniden sıralar.
pub fn altina_indir(satirlar: &[IsiSatiri], onek: &Yol) -> Vec<IsiSatiri> {
    let mut liste: Vec<IsiSatiri> = satirlar
        .iter()
        .filter(|s| s.yol.altinda(onek))
        .cloned()
        .collect();
    let en_buyuk = liste.iter().map(|s| s.skor).max().unwrap_or(0);
    for satir in liste.iter_mut() {
        satir.bant = Bant::hesapla(satir.skor, en_buyuk);
    }
    liste.sort_by(|a, b| b.skor.cmp(&a.skor).then(a.yol.cmp(&b.yol)));
    liste
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
// expect/unwrap test kodunda bilinçlidir: WORKER_CONTRACT.md § 4.2 bunları
// yalnızca testlerde serbest bırakır. Bir testin expect çağrısının
// başarısız olması, o iddianın yanlış olduğunun en net sinyalidir.
mod tests {
    use super::*;
    use crate::tarama::TaramaAyari;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn gecici(etiket: &str) -> PathBuf {
        let yol =
            std::env::temp_dir().join(format!("timefold-isi-{}-{}", etiket, std::process::id()));
        let _ = fs::remove_dir_all(&yol);
        fs::create_dir_all(&yol).expect("geçici dizin");
        yol
    }

    fn depo_uret(dizin: &Path) -> (PathBuf, PathBuf) {
        let kok = dizin.join("kok");
        fs::create_dir_all(&kok).expect("kok");
        let depo = dizin.join("depo");
        Depo::olustur_veya_ac(&depo, &kok).expect("depo");
        (kok, depo)
    }

    fn tara(depo_yolu: &Path, kok: &Path, zaman: i64) {
        let mut depo = Depo::ac(depo_yolu).expect("aç");
        let mut ayar = TaramaAyari::yeni(kok);
        ayar.zaman = zaman;
        depo.tarama_ekle(&ayar, &depo_yolu.join("gecici"), false)
            .expect("tarama");
    }

    #[test]
    fn bant_esikleri_dogru() {
        assert_eq!(Bant::hesapla(0, 100), Bant::Soguk);
        assert_eq!(Bant::hesapla(5, 100), Bant::Ilik);
        assert_eq!(Bant::hesapla(30, 100), Bant::Sicak);
        assert_eq!(Bant::hesapla(60, 100), Bant::Cok);
        assert_eq!(Bant::hesapla(100, 100), Bant::Alasin);
        assert_eq!(Bant::hesapla(5, 0), Bant::Soguk, "en büyük 0 ise soğuk");
    }

    #[test]
    fn bant_etiketleri_degisik() {
        let etiketler: Vec<&str> = [
            Bant::Soguk,
            Bant::Ilik,
            Bant::Sicak,
            Bant::Cok,
            Bant::Alasin,
        ]
        .iter()
        .map(|b| b.etiket())
        .collect();
        assert_eq!(etiketler, vec!["soguk", "ilik", "sicak", "cok", "alasin"]);
    }

    #[test]
    fn isi_skoru_derinlikle_carpiliyor() {
        let dizin = gecici("skor");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::create_dir_all(kok.join("derin/alt")).expect("dizin");
        fs::write(kok.join("derin/alt/big.bin"), "x".repeat(100)).expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        // İkinci taramada 100 bayt büyüme + 100 bayt silme
        fs::write(kok.join("kok_basi.txt"), "y".repeat(100)).expect("yaz");
        tara(&depo_yolu, &kok, 2_000);
        fs::remove_file(kok.join("derin/alt/big.bin")).expect("sil");
        tara(&depo_yolu, &kok, 3_000);

        let depo = Depo::ac(&depo_yolu).expect("aç");
        let harita = harita_olustur(&depo, "T0002", "T0003").expect("harita");
        let derin = harita
            .iter()
            .find(|s| s.yol.metin() == "derin/alt/big.bin")
            .expect("derin yol");
        // 100 bayt silme × (derinlik 3 + 1) = 400
        assert_eq!(derin.skor, 400);
        assert_eq!(derin.derinlik, 3);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn degismeyen_yol_listede_gorunmez() {
        let dizin = gecici("degismez");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("a.txt"), "12345").expect("yaz");
        fs::write(kok.join("b.txt"), "12345").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        fs::write(kok.join("a.txt"), "1234567890").expect("değiştir");
        tara(&depo_yolu, &kok, 2_000);
        let depo = Depo::ac(&depo_yolu).expect("aç");
        let harita = harita_olustur(&depo, "T0001", "T0002").expect("harita");
        let yollar: Vec<&str> = harita.iter().map(|s| s.yol.metin()).collect();
        assert!(yollar.contains(&"a.txt"));
        assert!(!yollar.contains(&"b.txt"));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn esit_skorlarda_yol_sirasi_kararli() {
        let dizin = gecici("kararli");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("b.txt"), "1").expect("yaz");
        fs::write(kok.join("a.txt"), "1").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        fs::write(kok.join("b.txt"), "12").expect("değiştir");
        fs::write(kok.join("a.txt"), "12").expect("değiştir");
        tara(&depo_yolu, &kok, 2_000);
        let depo = Depo::ac(&depo_yolu).expect("aç");
        let ilk = harita_olustur(&depo, "T0001", "T0002").expect("harita");
        let ikinci = harita_olustur(&depo, "T0001", "T0002").expect("harita");
        assert_eq!(ilk, ikinci);
        // Aynı derinlikte ve aynı değişimde: yol adı kararı verir.
        let isimler: Vec<&str> = ilk.iter().map(|s| s.yol.metin()).collect();
        let mut sirali = isimler.clone();
        sirali.sort();
        assert_eq!(isimler, sirali);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn tek_anlik_goruntude_harita_bos() {
        let dizin = gecici("tek");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("a.txt"), "1").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        let depo = Depo::ac(&depo_yolu).expect("aç");
        let harita = son_iki_anlik_goruntu(&depo).expect("harita");
        assert!(harita.is_empty());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn altina_indirme_onegi_suzer() {
        let satirlar = vec![
            IsiSatiri {
                yol: Yol::metin_yap("a/x"),
                onceki_bayt: 0,
                yeni_bayt: 10,
                fark: 10,
                derinlik: 2,
                skor: 20,
                bant: Bant::Soguk,
            },
            IsiSatiri {
                yol: Yol::metin_yap("b/x"),
                onceki_bayt: 0,
                yeni_bayt: 10,
                fark: 10,
                derinlik: 2,
                skor: 20,
                bant: Bant::Soguk,
            },
        ];
        let süzülmüş = altina_indir(&satirlar, &Yol::metin_yap("a"));
        assert_eq!(süzülmüş.len(), 1);
        assert_eq!(süzülmüş[0].yol.metin(), "a/x");
    }

    #[test]
    fn negatif_degisim_isaretli_kalir() {
        let satir = IsiSatiri {
            yol: Yol::metin_yap("a"),
            onceki_bayt: 100,
            yeni_bayt: 20,
            fark: -80,
            derinlik: 1,
            skor: 160,
            bant: Bant::Sicak,
        };
        assert_eq!(satir.degisim_miktari(), 80);
    }

    #[test]
    fn durum_oluştur_hedefe_kadar_ilerler() {
        let dizin = gecici("durum");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("a.txt"), "1").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        fs::write(kok.join("a.txt"), "12").expect("değiştir");
        tara(&depo_yolu, &kok, 2_000);
        let depo = Depo::ac(&depo_yolu).expect("aç");
        let ilk = durum_oluştur(&depo, "T0001").expect("ilk");
        let son = durum_oluştur(&depo, "T0002").expect("son");
        assert_eq!(ilk.get(&Yol::metin_yap("a.txt")).copied(), Some(1));
        assert_eq!(son.get(&Yol::metin_yap("a.txt")).copied(), Some(2));
        let _ = fs::remove_dir_all(&dizin);
    }
}
