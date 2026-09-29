//! Yol başına boyut geçmişi, zaman çizelgesi ve boşluk işaretleme.
//!
//! Zaman çizelgesi, "o tarihte ne vardı" sorusunu yanıtlar. Anlık görüntüler
//! fark tabanlı olduğu için bir yolun boyutu **zincir boyunca ileri doğru**
//! uygulanarak bulunur; çözümleme sıralı ve deterministiktir.
//!
//! **Boşluk kuralı (raporun en önemli dürüstlük taahhüdü):** bir anlık görüntü
//! okunamıyorsa o tarih aralığı "veri yok" olarak işaretlenir ve **araya değer
//! uydurulmaz**. Böyle bir noktanın ardından gelen ölçüm `bosluk` işaretlidir ve
//! tahmin hesabına girmez.

use std::collections::BTreeMap;
use std::path::Path;

use crate::depo::{AnlikGoruntuBilgisi, BilgiDurumu, Depo};
use crate::hata::{Hata, Sonuc};
use crate::kayit::{Degisim, TamKayit};
use crate::yol::Yol;

/// Zaman çizelgesindeki tek bir gözlem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZamanNoktasi {
    /// Anlık görüntü kimliği.
    pub kimlik: String,
    /// Anlık görüntünün UTC unix saniyesi.
    pub zaman: i64,
    /// O andaki boyut (bayt).
    pub bayt: u64,
    /// Bir önceki **zaman sırasındaki** ölçüme göre değişim.
    pub fark: i64,
    /// Bu nokta bir okunamayan anlık görüntüden mi geldi?
    pub bosluk: bool,
    /// Bu ölçüm silinmiş bir yol için mümkün mü? (yol yoksa `true`)
    pub yol_yok: bool,
}

/// Bir yolun tam zaman çizelgesi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZamanCizelgesi {
    /// İncelenen yol.
    pub yol: Yol,
    /// Zaman sırasına göre gözlemler.
    pub noktalar: Vec<ZamanNoktasi>,
    /// Okunamayan anlık görüntü kimlikleri.
    pub bosluklar: Vec<String>,
}

impl ZamanCizelgesi {
    /// İlk ölçülen boyut.
    pub fn ilk_bayt(&self) -> Option<u64> {
        self.noktalar.first().map(|n| n.bayt)
    }

    /// Son ölçülen boyut.
    pub fn son_bayt(&self) -> Option<u64> {
        self.noktalar.last().map(|n| n.bayt)
    }

    /// Son ölçümden ilk ölçüme göre net değişim.
    pub fn net_fark(&self) -> i64 {
        match (self.ilk_bayt(), self.son_bayt()) {
            (Some(ilk), Some(son)) => son as i64 - ilk as i64,
            _ => 0,
        }
    }

    /// Boşluk var mı?
    pub fn bosluk_var_mi(&self) -> bool {
        !self.bosluklar.is_empty()
    }

    /// Gözlem sayısı.
    pub fn adet(&self) -> usize {
        self.noktalar.len()
    }

    /// Tahmin için kullanılabilir (zaman, bayt) çiftlerini döndürür.
    ///
    /// Boşluk işaretli gözlemler ve yolun silinmiş olduğu gözlemler **dışlanır**;
    /// araya değer uydurulmaz. Boşluk varsa liste boş döner; çağıran taraf
    /// "hesaplanamadı" demelidir.
    pub fn temiz_noktalar(&self) -> Vec<(i64, u64)> {
        if self.bosluk_var_mi() {
            return Vec::new();
        }
        self.noktalar
            .iter()
            .filter(|n| !n.bosluk && !n.yol_yok)
            .map(|n| (n.zaman, n.bayt))
            .collect()
    }
}

/// Bir anlık görüntünün farkını yola uygular ve yolun yeni değerini döndürür.
///
/// `(yeni_deger, bu_taramada_yol_gecmis_mi)` döndürür. Zincir bozulduğunda
/// `mevcut` taşınır ve `gecti` yanlış döner; çağıran taraf boşluğu işaretler.
fn uygula(yol: &Yol, farklar: &[Degisim], mevcut: Option<u64>) -> (Option<u64>, bool) {
    let mut deger = mevcut;
    let mut gecti = false;
    for fark in farklar {
        match fark {
            Degisim::Eklendi { kayit } if &kayit.yol == yol => {
                deger = Some(kayit.bayt);
                gecti = true;
            }
            Degisim::Silindi { yol: silinen, .. } if silinen == yol => {
                deger = None;
                gecti = true;
            }
            Degisim::Degisti {
                yol: degisen,
                yeni_bayt,
                ..
            } if degisen == yol => {
                deger = Some(*yeni_bayt);
                gecti = true;
            }
            Degisim::AdDegisti { yeni_yol, bayt, .. } if yeni_yol == yol => {
                deger = Some(*bayt);
                gecti = true;
            }
            _ => {}
        }
    }
    (deger, gecti)
}

/// Bir yolun depo boyunca zaman çizelgesini üretir.
///
/// Zincir **kimlik sırasına** göre ileri doğru uygulanır (depo sırası = yazım
/// sırası), ardından gözlemler **zaman sırasına** göre sıralanır; böylece
/// "ne zaman büyüdü" sorusu kronolojik yanıtlanır.
pub fn cizelge_olustur(depo: &Depo, yol: &Yol) -> Sonuc<ZamanCizelgesi> {
    let bilgiler = depo.bilgiler();
    let mut noktalar: Vec<ZamanNoktasi> = Vec::new();
    let mut bosluklar: Vec<String> = Vec::new();
    let mut mevcut: Option<u64> = None;

    for durum in &bilgiler {
        let bilgi = match durum {
            BilgiDurumu::Ok(bilgi) => bilgi,
            BilgiDurumu::Bozuk { kimlik, ayrinti } => {
                // Okunamayan anlık görüntü: **boşluk**. Değer uydurulmaz; yalnızca
                // o ana kadar bilinen değer, boşluk işaretiyle korunur.
                bosluklar.push(kimlik.clone());
                if let Some(bayt) = mevcut {
                    noktalar.push(ZamanNoktasi {
                        kimlik: kimlik.clone(),
                        zaman: 0,
                        bayt,
                        fark: 0,
                        bosluk: true,
                        yol_yok: false,
                    });
                }
                let _ = ayrinti;
                continue;
            }
        };
        let kimlik = &bilgi.kimlik;
        let farklar = depo.fark_kayitlari(kimlik)?;
        let (deger, gecti) = uygula(yol, &farklar, mevcut);
        mevcut = deger;
        if gecti || mevcut.is_some() {
            let yol_yok = mevcut.is_none();
            noktalar.push(ZamanNoktasi {
                kimlik: kimlik.clone(),
                zaman: bilgi.baslik.zaman,
                bayt: mevcut.unwrap_or(0),
                fark: 0,
                bosluk: false,
                yol_yok,
            });
        }
    }

    // Kronolojik sıralama: aynı zamanda kimlik sırası kararı verilir.
    noktalar.sort_by(|a, b| a.zaman.cmp(&b.zaman).then(a.kimlik.cmp(&b.kimlik)));
    // `fark`, **zaman sırasındaki** önceki geçerli ölçüme göre hesaplanır.
    let mut son_gecerli: Option<u64> = None;
    for nokta in noktalar.iter_mut() {
        nokta.fark = match son_gecerli {
            Some(onceki_bayt) if !nokta.yol_yok => nokta.bayt as i64 - onceki_bayt as i64,
            _ => 0,
        };
        if !nokta.bosluk && !nokta.yol_yok {
            son_gecerli = Some(nokta.bayt);
        }
    }

    Ok(ZamanCizelgesi {
        yol: yol.clone(),
        noktalar,
        bosluklar,
    })
}

/// Depodaki tüm yolları ve güncel boyutlarını döndürür.
///
/// Zincir ileri doğru uygulanır. Bellek, son durumdaki yol sayısı kadardır.
pub fn guncel_durum(depo: &Depo) -> Sonuc<Vec<(Yol, u64)>> {
    let mut durum: BTreeMap<Yol, u64> = BTreeMap::new();
    for bilgi in depo.var_olan_bilgiler() {
        let farklar =
            depo.fark_kayitlari(&bilgi.kimlik)
                .map_err(|k: Hata| Hata::DepoBosOrBozuk {
                    dizin: depo.dizin().to_path_buf(),
                    ayrinti: k.to_string(),
                })?;
        for fark in farklar {
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
    }
    Ok(durum.into_iter().collect())
}

/// Yolları verilen üst yolün altında olacak şekilde filtreler.
pub fn yollari_filtrele(yollar: &[Yol], onek: &Yol) -> Vec<Yol> {
    yollar.iter().filter(|y| y.altinda(onek)).cloned().collect()
}

/// Depodaki en büyük yolun adını döndürür (hata ayıklama ve varsayılan seçim).
pub fn en_derin_yol(depo: &Depo) -> Sonuc<Yol> {
    let bilgiler = depo.var_olan_bilgiler();
    let son = bilgiler.last().ok_or_else(|| Hata::DepoBosOrBozuk {
        dizin: depo.dizin().to_path_buf(),
        ayrinti: "depoda anlık görüntü yok".to_owned(),
    })?;
    let kayitlar = depo.tam_kayitlari(&son.kimlik)?;
    let mut en_derin: Option<&TamKayit> = None;
    for kayit in &kayitlar {
        let aday = en_derin
            .map(|m| kayit.yol.derinlik() > m.yol.derinlik())
            .unwrap_or(true);
        if aday {
            en_derin = Some(kayit);
        }
    }
    Ok(en_derin.map(|k| k.yol.clone()).unwrap_or_else(Yol::kok))
}

/// Depo dizinindeki anlık görüntü kimliklerini listeler.
pub fn depo_kimliklerini_listele(depo_dizini: &Path) -> Sonuc<Vec<String>> {
    let depo = Depo::ac(depo_dizini)?;
    Ok(depo.kimlikler().to_vec())
}

/// Anlık görüntü başlıklarını kronolojik sıraya göre döndürür.
///
/// Zaman damgaları aynıysa kimlik sırası kararı verilir; sıralama kararlıdır.
pub fn kronolojik_sirala(bilgiler: &[AnlikGoruntuBilgisi]) -> Vec<AnlikGoruntuBilgisi> {
    let mut kopya = bilgiler.to_vec();
    kopya.sort_by(|a, b| {
        a.baslik
            .zaman
            .cmp(&b.baslik.zaman)
            .then(a.kimlik.cmp(&b.kimlik))
    });
    kopya
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
    use std::path::PathBuf;

    fn gecici(etiket: &str) -> PathBuf {
        let yol =
            std::env::temp_dir().join(format!("timefold-zaman-{}-{}", etiket, std::process::id()));
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
    fn zaman_cizelgesi_tek_nokta_uretir() {
        let dizin = gecici("tek");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("a.txt"), "12345").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        let depo = Depo::ac(&depo_yolu).expect("aç");
        let c = cizelge_olustur(&depo, &Yol::metin_yap("a.txt")).expect("çizelge");
        assert_eq!(c.adet(), 1);
        assert_eq!(c.ilk_bayt(), Some(5));
        assert_eq!(c.son_bayt(), Some(5));
        assert!(!c.bosluk_var_mi());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn zaman_cizelgesi_art_arda_tarama_gecmi_tutar() {
        let dizin = gecici("gecer");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("a.txt"), "1").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        fs::write(kok.join("a.txt"), "12").expect("yaz");
        tara(&depo_yolu, &kok, 2_000);
        fs::write(kok.join("a.txt"), "123").expect("yaz");
        tara(&depo_yolu, &kok, 3_000);
        let depo = Depo::ac(&depo_yolu).expect("aç");
        let c = cizelge_olustur(&depo, &Yol::metin_yap("a.txt")).expect("çizelge");
        assert_eq!(c.adet(), 3);
        assert_eq!(c.ilk_bayt(), Some(1));
        assert_eq!(c.son_bayt(), Some(3));
        assert_eq!(c.net_fark(), 2);
        let farklar: Vec<i64> = c.noktalar.iter().map(|n| n.fark).collect();
        assert_eq!(farklar, vec![0, 1, 1]);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn zaman_cizelgesi_kronolojik_sirali() {
        let dizin = gecici("sirali");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("a.txt"), "1").expect("yaz");
        // Depo sırası ile zaman sırası bilinçli olarak ters kurulur.
        tara(&depo_yolu, &kok, 3_000);
        fs::write(kok.join("a.txt"), "12").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        fs::write(kok.join("a.txt"), "123").expect("yaz");
        tara(&depo_yolu, &kok, 2_000);
        let depo = Depo::ac(&depo_yolu).expect("aç");
        let c = cizelge_olustur(&depo, &Yol::metin_yap("a.txt")).expect("çizelge");
        let zamanlar: Vec<i64> = c.noktalar.iter().map(|n| n.zaman).collect();
        assert_eq!(zamanlar, vec![1_000, 2_000, 3_000], "zaman artanda olmalı");
        let boyutlar: Vec<u64> = c.noktalar.iter().map(|n| n.bayt).collect();
        assert_eq!(boyutlar, vec![2, 3, 1]);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn silinen_yol_cizelgede_isaretlenir() {
        let dizin = gecici("silinen");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("a.txt"), "12345").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        fs::remove_file(kok.join("a.txt")).expect("sil");
        tara(&depo_yolu, &kok, 2_000);
        let depo = Depo::ac(&depo_yolu).expect("aç");
        let c = cizelge_olustur(&depo, &Yol::metin_yap("a.txt")).expect("çizelge");
        assert_eq!(c.adet(), 2);
        assert!(c.noktalar[1].yol_yok);
        assert_eq!(c.son_bayt(), Some(0));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn eksik_dosya_bosluk_olusum_tesbit_ediliyor() {
        let dizin = gecici("bosluk");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("a.txt"), "1").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        fs::write(kok.join("a.txt"), "12").expect("yaz");
        tara(&depo_yolu, &kok, 2_000);
        fs::write(kok.join("a.txt"), "123").expect("yaz");
        tara(&depo_yolu, &kok, 3_000);
        let depo_yolu2 = depo_yolu.clone();
        // Ortadaki anlık görüntüyü sil: zincir kırılır.
        let depo = Depo::ac(&depo_yolu2).expect("aç");
        fs::remove_file(depo.tam_yol("T0002")).expect("sil");
        fs::remove_file(depo.fark_yol("T0002")).expect("sil");
        let depo = Depo::ac(&depo_yolu2).expect("yeniden aç");
        let c = cizelge_olustur(&depo, &Yol::metin_yap("a.txt")).expect("çizelge");
        assert!(c.bosluk_var_mi(), "bozuk kayıt boşluğu işaretlenmeli");
        // Araya değer uydurulmaz: temiz nokta listesi boş döner.
        assert!(c.temiz_noktalar().is_empty());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn guncel_durum_son_durumu_yansitir() {
        let dizin = gecici("durum");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("a.txt"), "1").expect("yaz");
        fs::write(kok.join("b.txt"), "22").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        fs::remove_file(kok.join("b.txt")).expect("sil");
        tara(&depo_yolu, &kok, 2_000);
        let depo = Depo::ac(&depo_yolu).expect("aç");
        let durum = guncel_durum(&depo).expect("durum");
        let a = durum.iter().find(|(y, _)| y.metin() == "a.txt");
        let b = durum.iter().find(|(y, _)| y.metin() == "b.txt");
        assert_eq!(a.map(|(_, v)| *v), Some(1));
        assert!(b.is_none());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn yollari_filtrele_onek_altini_secer() {
        let yollar = vec![
            Yol::metin_yap("a"),
            Yol::metin_yap("a/b"),
            Yol::metin_yap("a/b/c"),
            Yol::metin_yap("d"),
        ];
        let secilen = yollari_filtrele(&yollar, &Yol::metin_yap("a"));
        assert_eq!(secilen.len(), 3);
    }

    #[test]
    fn en_derin_yol_bulunuyor() {
        let dizin = gecici("derin");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::create_dir_all(kok.join("a/b")).expect("dizin");
        fs::write(kok.join("a/b/c.txt"), "x").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        let depo = Depo::ac(&depo_yolu).expect("aç");
        let derin = en_derin_yol(&depo).expect("derin");
        assert_eq!(derin.metin(), "a/b/c.txt");
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn depo_kimliklerini_listele_calisir() {
        let dizin = gecici("liste");
        let (kok, depo_yolu) = depo_uret(&dizin);
        fs::write(kok.join("a.txt"), "1").expect("yaz");
        tara(&depo_yolu, &kok, 1_000);
        let liste = depo_kimliklerini_listele(&depo_yolu).expect("liste");
        assert_eq!(liste, vec!["T0001".to_owned()]);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn kronolojik_sirala_ayni_zamanlari_kimlige_gore_cozer() {
        let depo_yolu = Path::new(".");
        let _ = depo_yolu;
        let bilgiler: Vec<AnlikGoruntuBilgisi> = vec![
            AnlikGoruntuBilgisi {
                kimlik: "T0002".to_owned(),
                baslik: crate::kayit::AnlikGoruntuBasi {
                    sema: crate::kayit::SEMA.to_owned(),
                    surum: 1,
                    kimlik: "T0002".to_owned(),
                    temel: None,
                    zaman: 100,
                    kok: "/x".to_owned(),
                    kayit_sayisi: 0,
                    saglama: 0,
                    tur: crate::kayit::BasitTur::Tam,
                    kismi: false,
                    kismi_gerekce: String::new(),
                    orneklendi: false,
                    taranan: 0,
                },
            },
            AnlikGoruntuBilgisi {
                kimlik: "T0001".to_owned(),
                baslik: crate::kayit::AnlikGoruntuBasi {
                    sema: crate::kayit::SEMA.to_owned(),
                    surum: 1,
                    kimlik: "T0001".to_owned(),
                    temel: None,
                    zaman: 100,
                    kok: "/x".to_owned(),
                    kayit_sayisi: 0,
                    saglama: 0,
                    tur: crate::kayit::BasitTur::Tam,
                    kismi: false,
                    kismi_gerekce: String::new(),
                    orneklendi: false,
                    taranan: 0,
                },
            },
        ];
        let sirali = kronolojik_sirala(&bilgiler);
        assert_eq!(sirali[0].kimlik, "T0001");
        assert_eq!(sirali[1].kimlik, "T0002");
    }
}
