//! Komut satırı ile çekirdek arasındaki komut yürütücü.
//!
//! Bu modül, `clap` tarafından ayrıştırılan istekleri çekirdek işlevlere
//! çevirir ve [`crate::rapor`] biçimlendiricilerine yollar. Tüm yazma işlemleri
//! burada toplanır ve **yalnızca depo dizinine** yapılır; tarama köküne hiçbir
//! komutta yazılmaz.

use std::path::{Path, PathBuf};

use crate::depo::Depo;
use crate::hata::{Hata, Sonuc};
use crate::isiharitasi;
use crate::kayit::Degisim;
use crate::rapor::{self, RaporCikti};
use crate::saklama::SaklamaPolitikasi;
use crate::tahmin;
use crate::tarama::TaramaAyari;
use crate::yol::{Dislama, Yol};
use crate::zamancizelgesi;

/// `scan` komutunun isteği.
#[derive(Debug, Clone)]
pub struct ScanIstegi {
    /// Taranacak kök dizin.
    pub kok: PathBuf,
    /// Depo dizini.
    pub depo: PathBuf,
    /// Dışlanacak joker desenleri.
    pub haric: Vec<String>,
    /// Nokta ile başlayan adlar dışlansın mı?
    pub gizli_haric: bool,
    /// En fazla izlenecek derinlik.
    pub derinlik: usize,
    /// Saklanacak en fazla giriş sayısı (örnekleme eşiği).
    pub en_fazla_giris: Option<u64>,
    /// Korunacak en yeni anlık görüntü sayısı.
    pub tut: Option<usize>,
    /// Günlük seyreltme açık mı?
    pub gunluk: bool,
    /// Hiçbir şey yazma.
    pub kuru_sur: bool,
    /// Zaman damgası (testlerde sabit enjekte edilir).
    pub zaman: i64,
}

/// `diff` komutunun isteği.
#[derive(Debug, Clone)]
pub struct DiffIstegi {
    /// Depo dizini.
    pub depo: PathBuf,
    /// Karşılaştırılacak önceki anlık görüntü (yoksa en yeni).
    pub onceki: Option<String>,
    /// Listede gösterilecek en fazla kayıt sayısı.
    pub en_cok: usize,
}

/// `timeline` komutunun isteği.
#[derive(Debug, Clone)]
pub struct TimelineIstegi {
    /// Depo dizini.
    pub depo: PathBuf,
    /// İncelenecek yol.
    pub yol: Yol,
}

/// `heatmap` komutunun isteği.
#[derive(Debug, Clone)]
pub struct HeatmapIstegi {
    /// Depo dizini.
    pub depo: PathBuf,
    /// Başlangıç anlık görüntüsü (yoksa son iki anlık görüntü kullanılır).
    pub baslangic: Option<String>,
    /// Listede gösterilecek en fazla satır.
    pub en_cok: usize,
    /// Sonuçları bu üst yolün altına indir.
    pub altina: Option<Yol>,
}

/// `undo`/`redo` komutunun isteği.
#[derive(Debug, Clone)]
pub struct GeriIstegi {
    /// Depo dizini.
    pub depo: PathBuf,
    /// Hiçbir şey yazma.
    pub kuru_sur: bool,
}

/// `restore` komutunun isteği.
#[derive(Debug, Clone)]
pub struct RestoreIstegi {
    /// Depo dizini.
    pub depo: PathBuf,
    /// Dönülecek anlık görüntü.
    pub hedef: String,
    /// Hiçbir şey yazma.
    pub kuru_sur: bool,
}

/// `forecast` komutunun isteği.
#[derive(Debug, Clone)]
pub struct ForecastIstegi {
    /// Depo dizini.
    pub depo: PathBuf,
    /// İncelenecek yol.
    pub yol: Yol,
    /// Kaç gün sonrası tahmin edilsin.
    pub ileri_gun: u32,
}

/// `list` komutunun isteği.
#[derive(Debug, Clone)]
pub struct ListIstegi {
    /// Depo dizini.
    pub depo: PathBuf,
}

/// Tek bir komutun adı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Komut {
    /// Tarama yap ve depoya ekle.
    Scan,
    /// İki anlık görüntüyü karşılaştır.
    Diff,
    /// Yol başına zaman çizelgesi.
    Timeline,
    /// Isı haritası.
    Heatmap,
    /// Son anlık görüntüyü geri al.
    Undo,
    /// Geri alınan anlık görüntüyü yeniden uygula.
    Redo,
    /// Belirli bir anlık görüntüye dön.
    Restore,
    /// Büyüme tahmini.
    Forecast,
    /// Depodaki anlık görüntüleri listele.
    List,
}

impl Komut {
    /// Komutun adını döndürür.
    pub fn ad(self) -> &'static str {
        match self {
            Komut::Scan => "scan",
            Komut::Diff => "diff",
            Komut::Timeline => "timeline",
            Komut::Heatmap => "heatmap",
            Komut::Undo => "undo",
            Komut::Redo => "redo",
            Komut::Restore => "restore",
            Komut::Forecast => "forecast",
            Komut::List => "list",
        }
    }
}

fn depo_ac(depo: &Path) -> Sonuc<Depo> {
    Depo::ac(depo)
}

fn gecici_dizin(depo: &Path) -> PathBuf {
    depo.join("gecici")
}

/// Süreç içinde artan sayaç; kuru çalıştırmanın geçici dizin adını benzersiz
/// kılar.
///
/// `std::process::id()` **tek başına yeterli değildir**: test ikilisi tek süreçte
/// birden çok iş parçacığı çalıştırır ve iki `scan` aynı anda aynı dizini seçerse
/// biri diğerinin dosyasını siler (testlerde gözlenen yarış koşulu).
static KURU_SAYAÇ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Kuru çalıştırma için benzersiz bir geçici dizin yolu üretir.
fn kuru_gecici_dizin() -> PathBuf {
    let sira = KURU_SAYAÇ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("timefold-kuru-{}-{}", std::process::id(), sira))
}

/// `scan` komutunu yürütür.
///
/// Yazma davranışı: tarama köküne **hiçbir** zaman yazılmaz. Tek yazma hedefi
/// `istek.depo` dizinidir; `--kuru-sur` verildiğinde ve depo henüz yoksa hiçbir
/// dosya ya da dizin bile oluşturulmaz.
pub fn scan_yurut(istek: &ScanIstegi) -> Sonuc<RaporCikti> {
    let ayar = tarama_ayarini_hazirla(istek)?;
    let gecici = gecici_dizin(&istek.depo);
    let depo_var = istek.depo.join(crate::depo::INDEKS_ADI).is_file();

    let (sonuc, saklama_notu) = if istek.kuru_sur && !depo_var {
        // Depo yokken kuru çalıştırma: geçici dosyalar sistem geçici dizinine
        // yazılır ve hemen silinir; depo hiç oluşmaz.
        let gecici_gecici = kuru_gecici_dizin();
        std::fs::create_dir_all(&gecici_gecici)
            .map_err(|kaynak| Hata::io("geçici dizin oluştur", &gecici_gecici, kaynak))?;
        let sonuc = Depo::kuru_tarama(&ayar, &istek.depo, &gecici_gecici);
        let _ = std::fs::remove_dir_all(&gecici_gecici);
        (sonuc?, serde_json::Value::Null)
    } else {
        let mut depo = Depo::olustur_veya_ac(&istek.depo, &istek.kok)?;
        let sonuc = depo.tarama_ekle(&ayar, &gecici, istek.kuru_sur)?;
        let mut not = serde_json::Value::Null;
        let politika = SaklamaPolitikasi {
            tut: istek.tut,
            gunluk: istek.gunluk,
        };
        if politika.etkin_mi() && !istek.kuru_sur {
            let silinecek = depo.saklama_hedefleri(&politika);
            if !silinecek.is_empty() {
                let saklama = depo.saklama_uygula(&politika, false)?;
                not = serde_json::json!({
                    "silinen": saklama.silinen,
                    "serbest_bayt": saklama.serbest_bayt,
                    "politika": politika.aciklama(),
                });
            }
        }
        (sonuc, not)
    };

    let metin = rapor::tarama_metni(&sonuc.kimlik, &sonuc.ozet, &sonuc.fark, sonuc.uygulandi);
    let json = serde_json::json!({
        "komut": "scan",
        "anlik_goruntu": sonuc.kimlik,
        "uygulandi": sonuc.uygulandi,
        "kok": istek.kok.to_string_lossy(),
        "zaman": crate::zaman::unix_saniye_rfc3339(istek.zaman),
        "taranan": sonuc.ozet.taranan,
        "yazilan": sonuc.ozet.yazilan,
        "toplam_bayt": sonuc.ozet.toplam_bayt,
        "dosya": sonuc.ozet.dosya,
        "klasor": sonuc.ozet.klasor,
        "orneklendi": sonuc.ozet.orneklendi,
        "atlanan_izin": sonuc.ozet.atlanan_izin,
        "atlanan_sembolik": sonuc.ozet.atlanan_sembolik,
        "atlanan_dislama": sonuc.ozet.atlanan_dislama,
        "kesilen": sonuc.ozet.kesilen,
        "kismi": sonuc.ozet.kismi_mi(),
        "gerekce": sonuc.ozet.gerekce_metni(),
        "fark": {
            "eklendi": sonuc.fark.eklendi,
            "silindi": sonuc.fark.silindi,
            "degisti": sonuc.fark.degisti,
            "bozuk_satir": sonuc.fark.bozuk_satir,
            "net_bayt": sonuc.fark.net_bayt,
        },
        "saklama": saklama_notu,
    });
    Ok(RaporCikti::yeni(metin, json))
}

/// `ScanIstegi` değerinden doğrulanmış [`TaramaAyari`] üretir.
fn tarama_ayarini_hazirla(istek: &ScanIstegi) -> Sonuc<TaramaAyari> {
    let mut ayar = TaramaAyari::yeni(istek.kok.clone());
    ayar.en_fazla_derinlik = istek.derinlik;
    ayar.en_fazla_giris = istek.en_fazla_giris;
    ayar.zaman = istek.zaman;
    let mut dislama = Dislama::yeni();
    for desen in &istek.haric {
        dislama.desen_ekle(desen);
    }
    dislama.gizlileri_haric_ayarla(istek.gizli_haric);
    ayar.dislama = dislama;
    // Depo dizini ve geçici dizin taramaya dahil edilmez: araç kendi yazdığı
    // dosyaları tararsa "ne kadar büyüdü" cevabı kendi çıktısıyla şişer.
    ayar.haric_yol_ekle(istek.depo.clone());
    ayar.haric_yol_ekle(gecici_dizin(&istek.depo));
    ayar.dogrula()?;
    Ok(ayar)
}

/// `diff` komutunu yürütür.
pub fn diff_yurut(istek: &DiffIstegi) -> Sonuc<RaporCikti> {
    let depo = depo_ac(&istek.depo)?;
    if depo.bos_mu() {
        return Err(Hata::DepoBosOrBozuk {
            dizin: istek.depo.clone(),
            ayrinti: "karşılaştırılacak anlık görüntü yok".to_owned(),
        });
    }
    let kimlikler = depo.kimlikler().to_vec();
    let yeni = kimlikler
        .last()
        .cloned()
        .ok_or_else(|| Hata::AnlikGoruntuYok {
            istenen: "en yeni".to_owned(),
            en_yeni: None,
        })?;
    let onceki = match &istek.onceki {
        Some(k) => {
            if !depo.kimlik_var_mi(k) {
                return Err(Hata::AnlikGoruntuYok {
                    istenen: k.clone(),
                    en_yeni: depo.en_yeni().map(str::to_owned),
                });
            }
            k.clone()
        }
        None => {
            if kimlikler.len() < 2 {
                return Err(Hata::DepoBosOrBozuk {
                    dizin: istek.depo.clone(),
                    ayrinti: "karşılaştırma için en az iki anlık görüntü gerekir".to_owned(),
                });
            }
            kimlikler[kimlikler.len() - 2].clone()
        }
    };

    let farklar = depo.fark_kayitlari(&yeni)?;
    let ozet = fark_ozeti_hesapla(&depo, &onceki, &yeni, &farklar)?;
    let satirlar: Vec<String> = farklar.iter().map(fark_satiri_metni).collect();
    let metin = rapor::fark_metni(&ozet, istek.en_cok, &satirlar);
    let json = serde_json::json!({
        "komut": "diff",
        "onceki": onceki,
        "yeni": yeni,
        "eklendi": ozet.eklendi,
        "silindi": ozet.silindi,
        "degisti": ozet.degisti,
        "net_bayt": ozet.net_bayt,
        "kayitlar": farklar.iter().map(fark_json).collect::<Vec<_>>(),
    });
    Ok(RaporCikti::yeni(metin, json))
}

fn fark_ozeti_hesapla(
    _depo: &Depo,
    _onceki: &str,
    _yeni: &str,
    farklar: &[Degisim],
) -> Sonuc<crate::delta::FarkOzeti> {
    let mut ozet = crate::delta::FarkOzeti::default();
    for fark in farklar {
        match fark {
            Degisim::Eklendi { .. } => ozet.eklendi += 1,
            Degisim::Silindi { .. } => ozet.silindi += 1,
            Degisim::Degisti { .. } => ozet.degisti += 1,
            Degisim::AdDegisti { .. } => {}
        }
        ozet.net_bayt += fark.bayt_etkisi();
    }
    Ok(ozet)
}

fn fark_satiri_metni(fark: &Degisim) -> String {
    match fark {
        Degisim::Eklendi { kayit } => format!(
            "+ {} ({})",
            rapor::yol_adi(&kayit.yol),
            crate::tarama::bayt_metni(kayit.bayt)
        ),
        Degisim::Silindi { yol, son_bayt, .. } => {
            format!(
                "- {} (son boyut {})",
                rapor::yol_adi(yol),
                crate::tarama::bayt_metni(*son_bayt)
            )
        }
        Degisim::Degisti {
            yol,
            onceki_bayt,
            yeni_bayt,
            ..
        } => format!(
            "~ {} ({} -> {})",
            rapor::yol_adi(yol),
            crate::tarama::bayt_metni(*onceki_bayt),
            crate::tarama::bayt_metni(*yeni_bayt)
        ),
        Degisim::AdDegisti {
            eski_yol, yeni_yol, ..
        } => format!(
            "» {} -> {}",
            rapor::yol_adi(eski_yol),
            rapor::yol_adi(yeni_yol)
        ),
    }
}

fn fark_json(fark: &Degisim) -> serde_json::Value {
    match fark {
        Degisim::Eklendi { kayit } => serde_json::json!({
            "tip": "eklendi", "yol": kayit.yol, "bayt": kayit.bayt
        }),
        Degisim::Silindi { yol, son_bayt, .. } => serde_json::json!({
            "tip": "silindi", "yol": yol, "son_bayt": son_bayt
        }),
        Degisim::Degisti {
            yol,
            onceki_bayt,
            yeni_bayt,
            ..
        } => serde_json::json!({
            "tip": "degisti", "yol": yol,
            "onceki_bayt": onceki_bayt, "yeni_bayt": yeni_bayt
        }),
        Degisim::AdDegisti {
            eski_yol,
            yeni_yol,
            bayt,
        } => serde_json::json!({
            "tip": "ad_degisti", "eski_yol": eski_yol, "yeni_yol": yeni_yol, "bayt": bayt
        }),
    }
}

/// `timeline` komutunu yürütür.
pub fn timeline_yurut(istek: &TimelineIstegi) -> Sonuc<RaporCikti> {
    let depo = depo_ac(&istek.depo)?;
    let cizelge = zamancizelgesi::cizelge_olustur(&depo, &istek.yol)?;
    let metin = rapor::zaman_cizelgesi_metni(&cizelge);
    let json = serde_json::json!({
        "komut": "timeline",
        "yol": istek.yol,
        "gozlem": cizelge.adet(),
        "ilk_bayt": cizelge.ilk_bayt(),
        "son_bayt": cizelge.son_bayt(),
        "net_fark": cizelge.net_fark(),
        "bosluk": cizelge.bosluklar,
        "noktalar": cizelge.noktalar.iter().map(|n| serde_json::json!({
            "kimlik": n.kimlik,
            "zaman": crate::zaman::unix_saniye_rfc3339(n.zaman),
            "bayt": n.bayt,
            "fark": n.fark,
            "bosluk": n.bosluk,
            "yol_yok": n.yol_yok,
        })).collect::<Vec<_>>(),
    });
    Ok(RaporCikti::yeni(metin, json))
}

/// `heatmap` komutunu yürütür.
pub fn heatmap_yurut(istek: &HeatmapIstegi) -> Sonuc<RaporCikti> {
    let depo = depo_ac(&istek.depo)?;
    let satirlar = match &istek.baslangic {
        Some(baslangic) => {
            let yeni = depo
                .en_yeni()
                .ok_or_else(|| Hata::AnlikGoruntuYok {
                    istenen: "en yeni".to_owned(),
                    en_yeni: None,
                })?
                .to_owned();
            isiharitasi::harita_olustur(&depo, baslangic, &yeni)?
        }
        None => isiharitasi::son_iki_anlik_goruntu(&depo)?,
    };
    let satirlar = match &istek.altina {
        Some(onek) => isiharitasi::altina_indir(&satirlar, onek),
        None => satirlar,
    };
    let metin = rapor::isi_haritasi_metni(&satirlar, istek.en_cok);
    let json = serde_json::json!({
        "komut": "heatmap",
        "satir_sayisi": satirlar.len(),
        "satirlar": satirlar.iter().map(|s| serde_json::json!({
            "yol": s.yol,
            "onceki_bayt": s.onceki_bayt,
            "yeni_bayt": s.yeni_bayt,
            "fark": s.fark,
            "derinlik": s.derinlik,
            "skor": s.skor,
            "bant": s.bant.etiket(),
        })).collect::<Vec<_>>(),
    });
    Ok(RaporCikti::yeni(metin, json))
}

/// `undo` komutunu yürütür.
pub fn undo_yurut(istek: &GeriIstegi) -> Sonuc<RaporCikti> {
    let mut depo = depo_ac(&istek.depo)?;
    let sonuc = depo.undo(istek.kuru_sur)?;
    let metin = rapor::undo_metni("undo", &sonuc);
    let json = serde_json::json!({
        "komut": "undo",
        "anlik_goruntu": sonuc.kimlik,
        "uygulandi": sonuc.uygulandi,
        "serbest_bayt": sonuc.serbest_bayt,
        "kalan_adet": depo.adet(),
    });
    Ok(RaporCikti::yeni(metin, json))
}

/// `redo` komutunu yürütür.
pub fn redo_yurut(istek: &GeriIstegi) -> Sonuc<RaporCikti> {
    let mut depo = depo_ac(&istek.depo)?;
    let sonuc = depo.redo(istek.kuru_sur)?;
    let metin = rapor::undo_metni("redo", &sonuc);
    let json = serde_json::json!({
        "komut": "redo",
        "anlik_goruntu": sonuc.kimlik,
        "uygulandi": sonuc.uygulandi,
        "serbest_bayt": sonuc.serbest_bayt,
        "kalan_adet": depo.adet(),
    });
    Ok(RaporCikti::yeni(metin, json))
}

/// `restore` komutunu yürütür.
pub fn restore_yurut(istek: &RestoreIstegi) -> Sonuc<RaporCikti> {
    let mut depo = depo_ac(&istek.depo)?;
    let sonuc = depo.geri_al_hedefe(&istek.hedef, istek.kuru_sur)?;
    let metin = rapor::restore_metni(&sonuc);
    let json = serde_json::json!({
        "komut": "restore",
        "hedef": sonuc.hedef,
        "silinen": sonuc.silinen,
        "serbest_bayt": sonuc.serbest_bayt,
        "uygulandi": sonuc.uygulandi,
        "kalan_adet": depo.adet(),
    });
    Ok(RaporCikti::yeni(metin, json))
}

/// `forecast` komutunu yürütür.
pub fn forecast_yurut(istek: &ForecastIstegi) -> Sonuc<RaporCikti> {
    let depo = depo_ac(&istek.depo)?;
    let cizelge = zamancizelgesi::cizelge_olustur(&depo, &istek.yol)?;
    match tahmin::hesapla(&cizelge, istek.ileri_gun) {
        Ok(t) => {
            let metin = rapor::tahmin_metni(&istek.yol, &t);
            let json = serde_json::json!({
                "komut": "forecast",
                "yol": istek.yol,
                "hesaplandi": true,
                "gozlem": t.gozlem,
                "serbestlik": t.serbestlik,
                "egim_bayt_gun": t.egim_bayt_gun,
                "hedef_zaman": crate::zaman::unix_saniye_rfc3339(t.hedef_zaman),
                "tahmin": t.tahmin,
                "alt": t.alt,
                "ust": t.ust,
                "guven_duzeyi": t.guven_duzeyi,
                "varsayimlar": t.varsayimlar,
            });
            Ok(RaporCikti::yeni(metin, json))
        }
        Err(gerekce) => {
            let aciklama = gerekce.aciklama();
            let metin = rapor::hesaplanamadi_metni(&istek.yol, &aciklama);
            let json = serde_json::json!({
                "komut": "forecast",
                "yol": istek.yol,
                "hesaplandi": false,
                "gerekce": aciklama,
                "gozlem": cizelge.adet(),
            });
            Ok(RaporCikti::yeni(metin, json))
        }
    }
}

/// `list` komutunu yürütür.
pub fn list_yurut(istek: &ListIstegi) -> Sonuc<RaporCikti> {
    let depo = depo_ac(&istek.depo)?;
    let bilgiler = depo.var_olan_bilgiler();
    let kimlikler: Vec<String> = bilgiler.iter().map(|b| b.kimlik.clone()).collect();
    let basliklar: Vec<crate::kayit::AnlikGoruntuBasi> =
        bilgiler.iter().map(|b| b.baslik.clone()).collect();
    let ozet = depo.ozet()?;
    let mut metin = vec![rapor::liste_metni(&basliklar, &kimlikler)];
    metin.push(rapor::depo_ozeti_metni(&ozet));
    let json = serde_json::json!({
        "komut": "list",
        "adet": kimlikler.len(),
        "kok": depo.indeks().kok,
        "anlik_goruntuler": bilgiler.iter().map(|b| serde_json::json!({
            "kimlik": b.kimlik,
            "zaman": crate::zaman::unix_saniye_rfc3339(b.baslik.zaman),
            "tur": b.baslik.tur,
            "kayit_sayisi": b.baslik.kayit_sayisi,
            "kismi": b.baslik.kismi,
            "orneklendi": b.baslik.orneklendi,
            "taranan": b.baslik.taranan,
        })).collect::<Vec<_>>(),
        "depo_bayt": ozet.toplam_bayt,
    });
    Ok(RaporCikti::yeni(metin.join("\n"), json))
}

#[cfg(test)]
// expect/unwrap test kodunda bilinçlidir: WORKER_CONTRACT.md § 4.2 bunları
// yalnz=zca testlerde serbest bırakır. Bir testin expect çağrısı
// başarısız olmak, o iddianın yanlış olduğunun en net sinyalidir.
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn gecici(etiket: &str) -> PathBuf {
        let yol =
            std::env::temp_dir().join(format!("timefold-uyg-{}-{}", etiket, std::process::id()));
        let _ = fs::remove_dir_all(&yol);
        fs::create_dir_all(&yol).expect("geçici dizin");
        yol
    }

    fn temel_agac(dizin: &Path) -> PathBuf {
        let kok = dizin.join("kok");
        fs::create_dir_all(kok.join("a")).expect("dizin");
        fs::write(kok.join("a/x.txt"), "12345").expect("yaz");
        fs::write(kok.join("b.txt"), "123").expect("yaz");
        kok
    }

    fn scan_istegi(kok: &Path, depo: &Path) -> ScanIstegi {
        ScanIstegi {
            kok: kok.to_path_buf(),
            depo: depo.to_path_buf(),
            haric: Vec::new(),
            gizli_haric: false,
            derinlik: 64,
            en_fazla_giris: None,
            tut: None,
            gunluk: false,
            kuru_sur: false,
            zaman: 1_000,
        }
    }

    #[test]
    fn scan_list_diff_akisi_calisir() {
        let dizin = gecici("akis");
        let kok = temel_agac(&dizin);
        let depo = dizin.join("depo");
        let istek = scan_istegi(&kok, &depo);
        let cikti = scan_yurut(&istek).expect("scan");
        assert!(cikti.metin.contains("T0001"));

        // İkinci tarama: b.txt silinir.
        fs::remove_file(kok.join("b.txt")).expect("sil");
        let mut istek2 = istek.clone();
        istek2.zaman = 2_000;
        scan_yurut(&istek2).expect("scan2");

        let liste = list_yurut(&ListIstegi { depo: depo.clone() }).expect("list");
        assert!(liste.metin.contains("2 anlık görüntü"));

        let diff = diff_yurut(&DiffIstegi {
            depo: depo.clone(),
            onceki: None,
            en_cok: 5,
        })
        .expect("diff");
        assert!(diff.metin.contains("silindi"));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn scan_sonrasi_timeline_calisir() {
        let dizin = gecici("timeline");
        let kok = temel_agac(&dizin);
        let depo = dizin.join("depo");
        let istek = scan_istegi(&kok, &depo);
        scan_yurut(&istek).expect("scan");
        let mut istek2 = istek.clone();
        istek2.zaman = 2_000;
        fs::write(kok.join("a/x.txt"), "1234567890").expect("değiştir");
        scan_yurut(&istek2).expect("scan2");
        let cikti = timeline_yurut(&TimelineIstegi {
            depo: depo.clone(),
            yol: Yol::metin_yap("a/x.txt"),
        })
        .expect("timeline");
        assert!(cikti.metin.contains("2 gözlem"));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn scan_sonrasi_heatmap_calisir() {
        let dizin = gecici("heatmap");
        let kok = temel_agac(&dizin);
        let depo = dizin.join("depo");
        let istek = scan_istegi(&kok, &depo);
        scan_yurut(&istek).expect("scan");
        let mut istek2 = istek.clone();
        istek2.zaman = 2_000;
        fs::write(kok.join("yeni.bin"), "0123456789").expect("ekle");
        scan_yurut(&istek2).expect("scan2");
        let cikti = heatmap_yurut(&HeatmapIstegi {
            depo: depo.clone(),
            baslangic: None,
            en_cok: 5,
            altina: None,
        })
        .expect("heatmap");
        assert!(cikti.metin.contains("ısı haritası"));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn scan_sonrasi_forecast_calisir() {
        let dizin = gecici("forecast");
        let kok = temel_agac(&dizin);
        let depo = dizin.join("depo");
        let istek = scan_istegi(&kok, &depo);
        for i in 0..5_i64 {
            let mut t = istek.clone();
            t.zaman = 1_000 + i * 86_400;
            fs::write(kok.join("a/x.txt"), "x".repeat((i as usize + 1) * 10)).expect("yaz");
            scan_yurut(&t).expect("scan");
        }
        let cikti = forecast_yurut(&ForecastIstegi {
            depo: depo.clone(),
            yol: Yol::metin_yap("a/x.txt"),
            ileri_gun: 30,
        })
        .expect("forecast");
        assert!(cikti.metin.contains("güven aralığı"));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn undo_redo_restore_akisi_calisir() {
        let dizin = gecici("undo");
        let kok = temel_agac(&dizin);
        let depo = dizin.join("depo");
        let istek = scan_istegi(&kok, &depo);
        scan_yurut(&istek).expect("1");
        let mut istek2 = istek.clone();
        istek2.zaman = 2_000;
        scan_yurut(&istek2).expect("2");
        let mut istek3 = istek.clone();
        istek3.zaman = 3_000;
        scan_yurut(&istek3).expect("3");

        let undo = undo_yurut(&GeriIstegi {
            depo: depo.clone(),
            kuru_sur: false,
        })
        .expect("undo");
        assert!(undo.metin.contains("T0003"));
        let redo = redo_yurut(&GeriIstegi {
            depo: depo.clone(),
            kuru_sur: false,
        })
        .expect("redo");
        assert!(redo.metin.contains("T0003"));
        let restore = restore_yurut(&RestoreIstegi {
            depo: depo.clone(),
            hedef: "T0001".to_owned(),
            kuru_sur: false,
        })
        .expect("restore");
        assert!(restore.metin.contains("T0001"));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn kuru_surma_hiçbir_sey_yazmaz() {
        let dizin = gecici("kuru");
        let kok = temel_agac(&dizin);
        let depo = dizin.join("depo");
        let mut istek = scan_istegi(&kok, &depo);
        istek.kuru_sur = true;
        let cikti = scan_yurut(&istek).expect("kuru scan");
        assert!(cikti.metin.contains("yazılmadı"));
        assert!(
            !depo.join(crate::depo::INDEKS_ADI).exists() || {
                let depo_ac = Depo::ac(&depo);
                matches!(depo_ac, Ok(d) if d.bos_mu())
            }
        );
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn bos_depoda_diff_hata_donduruyor() {
        let dizin = gecici("bosdepo");
        let depo = dizin.join("depo");
        let _ = Depo::olustur_veya_ac(&depo, &dizin);
        assert!(diff_yurut(&DiffIstegi {
            depo,
            onceki: None,
            en_cok: 5
        })
        .is_err());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn saklama_politikasi_scan_icinde_uygulanir() {
        let dizin = gecici("saklama");
        let kok = temel_agac(&dizin);
        let depo = dizin.join("depo");
        let mut istek = scan_istegi(&kok, &depo);
        istek.tut = Some(2);
        for i in 0..5 {
            let mut t = istek.clone();
            t.zaman = 1_000 + i;
            fs::write(kok.join(format!("f{}.txt", i)), "x").expect("yaz");
            scan_yurut(&t).expect("scan");
        }
        let depo_ac = Depo::ac(&depo).expect("aç");
        assert_eq!(depo_ac.adet(), 2, "son 2 anlık görüntü korunmalı");
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn komut_adlari_dogru() {
        assert_eq!(Komut::Scan.ad(), "scan");
        assert_eq!(Komut::Forecast.ad(), "forecast");
        assert_eq!(Komut::Restore.ad(), "restore");
    }
}
