//! Metin ve JSON çıktı biçimlendirme.
//!
//! Her komut iki biçimde çıktı verir: insanın okuduğu metin (varsayılan) ve
//! makine okuyan JSON (`--json`). Aynı veriden üretilirler; iki ayrı hesap yolu
//! yoktur, böylece "ekranda görünen" ile "JSON'daki" sayılar ayrışamaz.

use serde::Serialize;

use crate::delta::FarkOzeti;
use crate::depo::{DepoOzeti, RestoreSonucu, SaklamaSonucu, UndoSonucu};
use crate::isiharitasi::IsiSatiri;
use crate::kayit::AnlikGoruntuBasi;
use crate::saklama::SaklamaPolitikasi;
use crate::tahmin::Tahmin;
use crate::tarama::{bayt_metni, TaramaOzeti};
use crate::yol::Yol;
use crate::zamancizelgesi::ZamanCizelgesi;

/// Bir komutun çıktısı: metin gövdesi ve isteğe bağlı JSON değeri.
///
/// JSON değeri `serde_json::Value` olarak taşınır; böylece rapor modülü hem
/// metin biçimlendiricinin hem de veri modelinin ayrıntısına bağlı kalmaz.
pub struct RaporCikti {
    /// Konsola yazılacak metin.
    pub metin: String,
    /// `--json` verildiğinde yazılacak değer.
    pub json: serde_json::Value,
}

impl RaporCikti {
    /// Metin ve JSON gövdesinden çıktı üretir.
    pub fn yeni(metin: impl Into<String>, json: serde_json::Value) -> Self {
        RaporCikti {
            metin: metin.into(),
            json,
        }
    }

    /// Serbest metinli (JSON karşılığı olmayan) çıktı üretir.
    ///
    /// Kullanılabilir çünkü her komutun bir JSON karşılığı vardır; bu işlev
    /// yalnızca hata mesajları gibi yapısal olmayan çıktılar içindir.
    pub fn metin_olustur(metin: impl Into<String>) -> Self {
        let metin = metin.into();
        RaporCikti {
            json: serde_json::Value::String(metin.clone()),
            metin,
        }
    }
}

/// `scan` komutunun metin çıktısını üretir.
pub fn tarama_metni(kimlik: &str, ozet: &TaramaOzeti, fark: &FarkOzeti, uygulandi: bool) -> String {
    let mut satirlar = Vec::new();
    satirlar.push(format!(
        "anlık görüntü {} {}",
        kimlik,
        if uygulandi {
            "kaydedildi"
        } else {
            "kuru çalıştırma (yazılmadı)"
        }
    ));
    satirlar.push(format!(
        "  taranan: {} giriş ({} dosya, {} klasör)",
        ozet.taranan, ozet.dosya, ozet.klasor
    ));
    satirlar.push(format!("  toplam: {}", bayt_metni(ozet.toplam_bayt)));
    satirlar.push(format!(
        "  fark: +{} eklendi, -{} silindi, ~{} değişti, net {}",
        fark.eklendi,
        fark.silindi,
        fark.degisti,
        bayt_farki_metni(fark.net_bayt)
    ));
    if fark.bozuk_satir > 0 {
        satirlar.push(format!(
            "  uyarı: {} kayıt bozuk olduğu için atlandı",
            fark.bozuk_satir
        ));
    }
    if ozet.orneklendi {
        satirlar.push(format!(
            "  örnekleme: {} giriş görüldü, {} kayıt saklandı; klasör toplamları alt sınırdır",
            ozet.taranan, ozet.yazilan
        ));
    }
    if ozet.atlanan_izin > 0 || ozet.atlanan_sembolik > 0 || ozet.atlanan_dislama > 0 {
        satirlar.push(format!(
            "  atlanan: {} izin, {} sembolik bağ, {} dışlama",
            ozet.atlanan_izin, ozet.atlanan_sembolik, ozet.atlanan_dislama
        ));
    }
    if ozet.kismi_mi() {
        satirlar.push(format!("  kısmi tarama: {}", ozet.gerekce_metni()));
    }
    satirlar.join("\n")
}

fn bayt_farki_metni(fark: i64) -> String {
    if fark < 0 {
        format!("-{}", bayt_metni(fark.unsigned_abs()))
    } else {
        format!("+{}", bayt_metni(fark as u64))
    }
}

/// Bir yolu metinsel çıktıda gösterir.
///
/// Depoda kök yol boş metin olarak saklanır; ekranda `(kök)` yazmak, boş
/// bir sütun görmekten daha anlaşılırdır.
pub fn yol_adi(yol: &Yol) -> String {
    if yol.kok_mu() {
        "(kök)".to_owned()
    } else {
        yol.metin().to_owned()
    }
}

/// `diff` komutunun metin çıktısını üretir.
pub fn fark_metni(ozet: &FarkOzeti, en_cok: usize, satirlar: &[String]) -> String {
    let mut cikti = vec![format!(
        "fark özeti: {} değişim kaydı ({} eklendi, {} silindi, {} değişti)",
        ozet.toplam(),
        ozet.eklendi,
        ozet.silindi,
        ozet.degisti
    )];
    cikti.push(format!("  net: {}", bayt_farki_metni(ozet.net_bayt)));
    if ozet.bozuk_satir > 0 {
        cikti.push(format!("  uyarı: {} bozuk satır atlandı", ozet.bozuk_satir));
    }
    if ozet.bos_mu() {
        cikti.push("  iki anlık görüntü arasında fark yok.".to_owned());
    }
    for satir in satirlar.iter().take(en_cok) {
        cikti.push(format!("  {}", satir));
    }
    if satirlar.len() > en_cok {
        cikti.push(format!("  ... ve {} kayıt daha", satirlar.len() - en_cok));
    }
    cikti.join("\n")
}

/// `timeline` komutunun metin çıktısını üretir.
pub fn zaman_cizelgesi_metni(cizelge: &ZamanCizelgesi) -> String {
    let mut cikti = vec![format!(
        "{} — {} gözlem, {} ile {} arası",
        yol_adi(&cizelge.yol),
        cizelge.adet(),
        cizelge
            .ilk_bayt()
            .map(bayt_metni)
            .unwrap_or_else(|| "-".to_owned()),
        cizelge
            .son_bayt()
            .map(bayt_metni)
            .unwrap_or_else(|| "-".to_owned())
    )];
    for nokta in &cizelge.noktalar {
        let isaret = if nokta.yol_yok {
            "yol yok"
        } else if nokta.bosluk {
            "BOŞLUK"
        } else {
            &bayt_farki_metni(nokta.fark)
        };
        cikti.push(format!(
            "  {}  {:>10}  {:>12}  {}",
            crate::zaman::unix_saniye_rfc3339(nokta.zaman),
            bayt_metni(nokta.bayt),
            isaret,
            nokta.kimlik
        ));
    }
    if cizelge.bosluk_var_mi() {
        cikti.push(format!(
            "  uyarı: {} anlık görüntü okunamadı; boşluk doldurulmadı",
            cizelge.bosluklar.join(", ")
        ));
    }
    cikti.join("\n")
}

/// `heatmap` komutunun metin çıktısını üretir.
pub fn isi_haritasi_metni(satirlar: &[IsiSatiri], gosterilen: usize) -> String {
    let mut cikti = vec![format!("ısı haritası — {} değişen yol", satirlar.len())];
    if satirlar.is_empty() {
        cikti.push("  iki anlık görüntü arasında değişen yol yok.".to_owned());
        return cikti.join("\n");
    }
    cikti.push(format!(
        "  {:<12} {:>7} {:>10} {:>10} {:>9} {:>8}",
        "bant", "derinlik", "önceki", "yeni", "fark", "skor"
    ));
    for satir in satirlar.iter().take(gosterilen) {
        cikti.push(format!(
            "  {:<12} {:>7} {:>10} {:>10} {:>9} {:>8}",
            satir.bant.etiket(),
            satir.derinlik,
            bayt_metni(satir.onceki_bayt),
            bayt_metni(satir.yeni_bayt),
            bayt_farki_metni(satir.fark),
            satir.skor
        ));
        cikti.push(format!("      {}", yol_adi(&satir.yol)));
    }
    if satirlar.len() > gosterilen {
        cikti.push(format!("  ... ve {} yol daha", satirlar.len() - gosterilen));
    }
    cikti.join("\n")
}

/// `forecast` komutunun metin çıktısını üretir.
pub fn tahmin_metni(yol: &Yol, tahmin: &Tahmin) -> String {
    let mut cikti = vec![format!(
        "{} — {} gözlem, {} serbestlik derecesi",
        yol_adi(yol),
        tahmin.gozlem,
        tahmin.serbestlik
    )];
    cikti.push(format!(
        "  eğilim: {} / gün",
        bayt_farki_metni(tahmin.egim_bayt_gun.round() as i64)
    ));
    cikti.push(format!(
        "  tahmin ({} tarihi, {} gün sonra): {}",
        crate::zaman::unix_saniye_rfc3339(tahmin.hedef_zaman),
        tahmin.ileri_gun,
        bayt_metni(tahmin.tahmin_bayt())
    ));
    cikti.push(format!(
        "  %95 güven aralığı: {} .. {}",
        bayt_metni(tahmin.alt.max(0.0) as u64),
        bayt_metni(tahmin.ust.max(0.0) as u64)
    ));
    cikti.push("  varsayımlar:".to_owned());
    for varsayim in &tahmin.varsayimlar {
        cikti.push(format!("    - {}", varsayim));
    }
    cikti.join("\n")
}

/// `forecast` komutunun "hesaplanamadı" metnini üretir.
pub fn hesaplanamadi_metni(yol: &Yol, gerekce: &str) -> String {
    format!("{} — {}", yol_adi(yol), gerekce)
}

/// Depo istatistiğinin metnini üretir.
pub fn depo_ozeti_metni(ozet: &DepoOzeti) -> String {
    format!(
        "{} anlık görüntü, {} dosya, {} (undo: {})",
        ozet.anlik_goruntu_sayisi,
        ozet.dosya_sayisi,
        bayt_metni(ozet.toplam_bayt),
        if ozet.undo_edilebilir { "var" } else { "yok" }
    )
}

/// `undo`/`redo` metnini üretir.
pub fn undo_metni(eylem: &str, sonuc: &UndoSonucu) -> String {
    format!(
        "{} {} — {} {}",
        eylem,
        sonuc.kimlik,
        bayt_metni(sonuc.serbest_bayt),
        if sonuc.uygulandi {
            "uygulandı"
        } else {
            "kuru çalıştırma, uygulanmadı"
        }
    )
}

/// `restore` metnini üretir.
pub fn restore_metni(sonuc: &RestoreSonucu) -> String {
    let silinen = if sonuc.silinen.is_empty() {
        "hiçbir anlık görüntü".to_owned()
    } else {
        sonuc.silinen.join(", ")
    };
    format!(
        "{} noktasına dönüldü; silinen: {} ({}{})",
        sonuc.hedef,
        silinen,
        bayt_metni(sonuc.serbest_bayt),
        if sonuc.uygulandi {
            ""
        } else {
            "; kuru çalıştırma, uygulanmadı"
        }
    )
}

/// Saklama metnini üretir.
pub fn saklama_metni(sonuc: &SaklamaSonucu, politika: &SaklamaPolitikasi) -> String {
    if sonuc.silinen.is_empty() {
        return format!(
            "saklama politikası uygulandı ({}); silinecek kayıt yok",
            politika.aciklama()
        );
    }
    format!(
        "{} anlık görüntü kaldırıldı, {} kazanıldı ({}){}",
        sonuc.silinen.len(),
        bayt_metni(sonuc.serbest_bayt),
        politika.aciklama(),
        if sonuc.uygulandi {
            ""
        } else {
            "; kuru çalıştırma, uygulanmadı"
        }
    )
}

/// `list` komutunun metnini üretir.
pub fn liste_metni(basliklar: &[AnlikGoruntuBasi], kimlikler: &[String]) -> String {
    if basliklar.is_empty() {
        return "depo boş: henüz tarama yapılmamış.".to_owned();
    }
    let mut cikti = vec![format!("{} anlık görüntü:", basliklar.len())];
    for (baslik, kimlik) in basliklar.iter().zip(kimlikler.iter()) {
        cikti.push(format!(
            "  {}  {}  {:>10} kayıt  {}",
            kimlik,
            crate::zaman::unix_saniye_rfc3339(baslik.zaman),
            baslik.kayit_sayisi,
            if baslik.kismi {
                format!("kısmi ({})", baslik.kismi_gerekce)
            } else if baslik.orneklendi {
                "örneklenmiş".to_owned()
            } else {
                "tam".to_owned()
            }
        ));
    }
    cikti.join("\n")
}

/// JSON çıktısında kullanılan basit bir sayı sözlüğü.
#[derive(Debug, Serialize)]
pub struct SayiKumesi {
    /// Toplam bayt.
    pub toplam_bayt: u64,
    /// Okunabilir karşılık.
    pub toplam_okunabilir: String,
}

/// Bayt sayısını okunabilir metne çeviren yardımcı.
pub fn bayt_kumesi(bayt: u64) -> SayiKumesi {
    SayiKumesi {
        toplam_bayt: bayt,
        toplam_okunabilir: bayt_metni(bayt),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
// expect/unwrap test kodunda bilinçlidir: WORKER_CONTRACT.md § 4.2 bunları
// yalnızca testlerde serbest bırakır. Bir testin expect çağrısının
// başarısız olması, o iddianın yanlış olduğunun en net sinyalidir.
mod tests {
    use super::*;
    use crate::isiharitasi::Bant;
    use crate::kayit::KayitTuru;
    use crate::kayit::{BasitTur, TamKayit};

    fn ozet() -> TaramaOzeti {
        TaramaOzeti {
            taranan: 10,
            yazilan: 8,
            toplam_bayt: 2048,
            klasor: 2,
            dosya: 6,
            ..Default::default()
        }
    }

    #[test]
    fn tarama_metni_iceriyor() {
        let metin = tarama_metni("T0001", &ozet(), &FarkOzeti::default(), true);
        assert!(metin.contains("T0001"));
        assert!(metin.contains("kaydedildi"));
        assert!(metin.contains("2.0 KB"));
        assert!(metin.contains("net +0 B"));
    }

    #[test]
    fn tarama_metni_kuru_calistirmayi_belirtir() {
        let metin = tarama_metni("T0001", &ozet(), &FarkOzeti::default(), false);
        assert!(metin.contains("kuru çalıştırma"));
    }

    #[test]
    fn tarama_metni_orneklemeyi_belirtir() {
        let mut o = ozet();
        o.orneklendi = true;
        let metin = tarama_metni("T0001", &o, &FarkOzeti::default(), true);
        assert!(metin.contains("alt sınırdır"));
    }

    #[test]
    fn fark_metni_bos_durumu_belirtir() {
        let metin = fark_metni(&FarkOzeti::default(), 10, &[]);
        assert!(metin.contains("fark yok"));
    }

    #[test]
    fn fark_metni_kayitlari_listeler() {
        let ozet = FarkOzeti {
            eklendi: 3,
            silindi: 1,
            degisti: 2,
            net_bayt: 500,
            bozuk_satir: 0,
        };
        let satirlar = vec!["eklendi a".to_owned(), "silindi b".to_owned()];
        let metin = fark_metni(&ozet, 1, &satirlar);
        assert!(metin.contains("6 değişim kaydı"));
        assert!(metin.contains("eklendi a"));
        assert!(metin.contains("ve 1 kayıt daha"));
    }

    #[test]
    fn zaman_cizelgesi_metni_boşlugu_belirtir() {
        let cizelge = ZamanCizelgesi {
            yol: Yol::metin_yap("a"),
            noktalar: vec![],
            bosluklar: vec!["T0002".to_owned()],
        };
        let metin = zaman_cizelgesi_metni(&cizelge);
        assert!(metin.contains("boşluk doldurulmadı"));
    }

    #[test]
    fn isi_haritasi_metni_bos_durumu_belirtir() {
        let metin = isi_haritasi_metni(&[], 10);
        assert!(metin.contains("değişen yol yok"));
    }

    #[test]
    fn isi_haritasi_metni_satirlari_yazar() {
        let satirlar = vec![IsiSatiri {
            yol: Yol::metin_yap("a/b"),
            onceki_bayt: 1024,
            yeni_bayt: 2048,
            fark: 1024,
            derinlik: 2,
            skor: 3072,
            bant: Bant::Alasin,
        }];
        let metin = isi_haritasi_metni(&satirlar, 10);
        assert!(metin.contains("alasin"));
        assert!(metin.contains("a/b"));
        assert!(metin.contains("+1.0 KB"));
    }

    #[test]
    fn depo_ozeti_metni_yazar() {
        let ozet = DepoOzeti {
            anlik_goruntu_sayisi: 3,
            dosya_sayisi: 6,
            toplam_bayt: 1024,
            undo_edilebilir: true,
            geri_yigininda: 0,
        };
        let metin = depo_ozeti_metni(&ozet);
        assert!(metin.contains("3 anlık görüntü"));
        assert!(metin.contains("undo: var"));
    }

    #[test]
    fn liste_metni_bos_depo_der() {
        assert!(liste_metni(&[], &[]).contains("depo boş"));
    }

    #[test]
    fn liste_metni_kismi_taramayi_isaretler() {
        let baslik = AnlikGoruntuBasi {
            sema: crate::kayit::SEMA.to_owned(),
            surum: 1,
            kimlik: "T0001".to_owned(),
            temel: None,
            zaman: 1_000,
            kok: "/x".to_owned(),
            kayit_sayisi: 5,
            saglama: 0,
            tur: BasitTur::Tam,
            kismi: true,
            kismi_gerekce: "1 izin hatası".to_owned(),
            orneklendi: false,
            taranan: 5,
        };
        let metin = liste_metni(&[baslik], &["T0001".to_owned()]);
        assert!(metin.contains("kısmi (1 izin hatası)"));
    }

    #[test]
    fn bayt_kumesi_hesaplanir() {
        let k = bayt_kumesi(2048);
        assert_eq!(k.toplam_bayt, 2048);
        assert_eq!(k.toplam_okunabilir, "2.0 KB");
    }

    #[test]
    fn tahmin_metni_ve_hesaplanamadi_metni() {
        let gün = 86_400i64;
        let cizelge = ZamanCizelgesi {
            yol: Yol::metin_yap("a"),
            noktalar: (0..4)
                .map(|i| crate::zamancizelgesi::ZamanNoktasi {
                    kimlik: format!("T{:04}", i + 1),
                    zaman: i * gün,
                    bayt: 1_000 + i as u64 * 100,
                    fark: 0,
                    bosluk: false,
                    yol_yok: false,
                })
                .collect(),
            bosluklar: Vec::new(),
        };
        let tahmin = crate::tahmin::hesapla(&cizelge, 30).expect("tahmin");
        let metin = tahmin_metni(&Yol::metin_yap("a"), &tahmin);
        assert!(metin.contains("güven aralığı"));
        assert!(metin.contains("varsayımlar"));
        let basarisiz = hesaplanamadi_metni(&Yol::metin_yap("a"), "hesaplanamadı — az veri");
        assert!(basarisiz.contains("az veri"));
    }

    #[test]
    fn restore_ve_saklama_metinleri() {
        let restore = RestoreSonucu {
            hedef: "T0002".to_owned(),
            silinen: vec!["T0003".to_owned()],
            serbest_bayt: 1024,
            uygulandi: true,
        };
        assert!(restore_metni(&restore).contains("T0002"));
        let saklama = SaklamaSonucu {
            silinen: vec!["T0001".to_owned()],
            serbest_bayt: 2048,
            uygulandi: false,
        };
        let metin = saklama_metni(&saklama, &SaklamaPolitikasi::son_n(1));
        assert!(metin.contains("kuru çalıştırma"));
        let undo = UndoSonucu {
            kimlik: "T0002".to_owned(),
            uygulandi: true,
            serbest_bayt: 512,
        };
        assert!(undo_metni("undo", &undo).contains("T0002"));
    }

    #[test]
    fn rapor_cikti_json_ve_metin_tasir() {
        let c = RaporCikti::yeni("metin", serde_json::json!({"a": 1}));
        assert_eq!(c.metin, "metin");
        assert_eq!(c.json["a"], 1);
        let s = RaporCikti::metin_olustur("yalnızca metin");
        assert_eq!(s.metin, "yalnızca metin");
    }

    #[test]
    fn kok_yolu_ekranda_kok_olarak_gorunur() {
        assert_eq!(yol_adi(&Yol::kok()), "(kök)");
        assert_eq!(yol_adi(&Yol::metin_yap("a/b.txt")), "a/b.txt");
    }

    #[test]
    fn tam_kayit_etiketi_kullanilabilir() {
        let kayit = TamKayit {
            yol: Yol::kok(),
            tur: KayitTuru::Klasor,
            bayt: 0,
            degisiklik: 0,
        };
        assert_eq!(kayit.tur.etiket(), "klasor");
    }
}
