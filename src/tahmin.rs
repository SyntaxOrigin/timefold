//! En küçük kareler eğilim çizgisi, güven aralığı ve varsayım listesi.
//!
//! Bu modül raporun **dürüstlük taahhüdünü** uygular: her tahmin üç şeyle
//! gelir — hesaplanan değer, güven aralığı ve varsayım listesi. Veri yetersizse
//! tahmin **üretilmez**; "hesaplanamadı" gerekçesiyle döner (raporun R4 riski).
//!
//! Model: `boyut(t) = a + b · t`, burada `t` gün cinsinden geçen süredir.
//!
//! - `a` kestirimi: ortalama `boyut − b·t`
//! - `b` kestirimi: `Σ(t−t̄)(y−ȳ) / Σ(t−t̄)²`
//! - Artıklar: `e_i = y_i − (a + b·t_i)`
//! - Kalan kareler toplamı: `SSE = Σ e_i²`
//! - Artık standart sapması: `s = √(SSE / (n − 2))`  (n ≤ 2 ise tahmin yok)
//! - Ortalama yanıt için standart hata: `s · √(1 + 1/n + (t₀−t̄)²/Sxx)`
//!
//! Güven aralığı, iki taraflı 95% düzeyi ve Student t dağılımının `n−2` serbestlik
//! derecesi için kritik değeridir. Dağılımın kuyruğu bir **yaklaşımdır**;
//! varsayım listesinde bu açıkça yazılır.

use crate::zamancizelgesi::ZamanCizelgesi;

/// Bir günün saniye cinsinden uzunluğu.
const GUN_SANIYE: f64 = 86_400.0;

/// Bir tahminin hesaplanamama gerekçesi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hesaplanamama {
    /// Yeterli gözlem yok.
    ///
    /// Eğilim + güven aralığı için en az üç gözlem gerekir; iki gözlemde
    /// artık serbestlik derecesi sıfırdır ve sapma hesaplanamaz.
    YetersizVeri {
        /// Elimizdeki gözlem sayısı.
        var: usize,
        /// Gereken en az gözlem sayısı.
        gereken: usize,
    },
    /// Tüm gözlemler aynı zamanda (tek bir tarama).
    TekZaman {
        /// Tek zaman noktasının unix saniyesi.
        zaman: i64,
    },
    /// Veri aralığında boşluk var; araya değer uydurulmaz.
    Bosluklu {
        /// Boşlukta bulunan anlık görüntü kimlikleri.
        kimlikler: Vec<String>,
    },
    /// Sıfır bölme: tüm gözlemler aynı zamanda.
    DegersizAralik,
}

impl Hesaplanamama {
    /// Kullanıcıya gösterilecek açıklama metni.
    pub fn aciklama(&self) -> String {
        match self {
            Hesaplanamama::YetersizVeri { var, gereken } => format!(
                "hesaplanamadı — en az {} gözlem gerekir, mevcut {}",
                gereken, var
            ),
            Hesaplanamama::TekZaman { zaman } => format!(
                "hesaplanamadı — tüm gözlemler aynı anda ({})",
                crate::zaman::unix_saniye_rfc3339(*zaman)
            ),
            Hesaplanamama::Bosluklu { kimlikler } => format!(
                "hesaplanamadı — zincirde okunamayan anlık görüntü var ({}); araya değer uydurulmadı",
                kimlikler.join(", ")
            ),
            Hesaplanamama::DegersizAralik => {
                "hesaplanamadı — gözlemlerin tümü aynı anda".to_owned()
            }
        }
    }
}

/// Bir yol için hesaplanmış tahmin.
#[derive(Debug, Clone, PartialEq)]
pub struct Tahmin {
    /// Gözlem sayısı.
    pub gozlem: usize,
    /// Gün başına ortalama değişim (bayt/gün).
    pub egim_bayt_gun: f64,
    /// Serbestlik derecesi.
    pub serbestlik: usize,
    /// Artık standart sapma (bayt).
    pub artik_sapma: f64,
    /// `ileri_gun` gün sonundaki tahmin.
    pub tahmin: f64,
    /// Tahminin alt güven sınırı.
    pub alt: f64,
    /// Tahminin üst güven sınırı.
    pub ust: f64,
    /// Gerçekleşmesi beklenen gün (unix saniye).
    pub hedef_zaman: i64,
    /// Kaç gün ileriye bakıldığı (kullanıcının `--ileri-gun` değeri).
    pub ileri_gun: u32,
    /// Tahminin dayandığı varsayımlar.
    pub varsayimlar: Vec<String>,
    /// Güven düzeyi (0.95).
    pub guven_duzeyi: f64,
}

impl Tahmin {
    /// Güven aralığının genişliğini döndürür.
    pub fn aralik_genisligi(&self) -> f64 {
        self.ust - self.alt
    }

    /// Güven aralığı alt sınırın negatif olduğunu söyler (fiziksel olarak
    /// anlamsız; bu durumda varsayım listesinde uyarı bulunur).
    pub fn alt_negatif_mi(&self) -> bool {
        self.alt < 0.0
    }

    /// Tahminin yuvarlanmış hâlini bayt olarak döndürür (negatifse 0).
    pub fn tahmin_bayt(&self) -> u64 {
        if self.tahmin <= 0.0 {
            0
        } else {
            self.tahmin.round() as u64
        }
    }
}

/// Bir zaman çizelgesinden tahmin hesaplar.
///
/// `ileri_gun` kadar gün sonundaki değer için tahmin, güven aralığı ve varsayım
/// listesi üretir. Yetersiz veride [`Hesaplanamama`] döner.
pub fn hesapla(cizelge: &ZamanCizelgesi, ileri_gun: u32) -> Result<Tahmin, Hesaplanamama> {
    if !cizelge.bosluklar.is_empty() {
        return Err(Hesaplanamama::Bosluklu {
            kimlikler: cizelge.bosluklar.clone(),
        });
    }
    let gozlemler = cizelge.temiz_noktalar();
    const GEREKEN: usize = 3;
    if gozlemler.len() < GEREKEN {
        return Err(Hesaplanamama::YetersizVeri {
            var: gozlemler.len(),
            gereken: GEREKEN,
        });
    }
    let ilk_zaman = gozlemler[0].0;
    let mut xler: Vec<f64> = Vec::with_capacity(gozlemler.len());
    let mut yler: Vec<f64> = Vec::with_capacity(gozlemler.len());
    for (zaman, bayt) in &gozlemler {
        xler.push((*zaman - ilk_zaman) as f64 / GUN_SANIYE);
        yler.push(*bayt as f64);
    }
    let n = xler.len();
    if n < GEREKEN {
        return Err(Hesaplanamama::YetersizVeri {
            var: n,
            gereken: GEREKEN,
        });
    }
    let x_ort = xler.iter().sum::<f64>() / n as f64;
    let y_ort = yler.iter().sum::<f64>() / n as f64;

    let mut sxy = 0.0f64;
    let mut sxx = 0.0f64;
    for (x, y) in xler.iter().zip(yler.iter()) {
        sxy += (x - x_ort) * (y - y_ort);
        sxx += (x - x_ort) * (x - x_ort);
    }
    if sxx <= f64::EPSILON {
        return Err(Hesaplanamama::DegersizAralik);
    }
    let egim = sxy / sxx;
    let kirpim = y_ort - egim * x_ort;

    let mut sse = 0.0f64;
    for (x, y) in xler.iter().zip(yler.iter()) {
        let artik = y - (kirpim + egim * x);
        sse += artik * artik;
    }
    let sd = n.saturating_sub(2);
    let artik_sapma = (sse / sd as f64).sqrt();

    let ileri = ileri_gun as f64;
    let x0 = xler.last().copied().unwrap_or(0.0) + ileri;
    let tahmin = kirpim + egim * x0;

    // Ortalama yanıt için standart hata.
    let se = artik_sapma * (1.0 + 1.0 / n as f64 + (x0 - x_ort).powi(2) / sxx).sqrt();
    let kritik = t_kritik(sd);
    let guven_duzeyi = 0.95;
    let alt = tahmin - kritik * se;
    let ust = tahmin + kritik * se;

    let hedef_zaman = ilk_zaman + (x0 * GUN_SANIYE).round() as i64;
    let mut varsayimlar = vec![
        format!("doğrusal eğilim varsayıldı (bayt/gün = {:.2})", egim),
        format!("{} gözlem, {} serbestlik derecesi", n, sd),
        format!("95% güven aralığı, Student t kritik değeri ≈ {:.3}", kritik),
        "artıklar normal dağılım varsayıldı; ani artışlar (twinning spike) aralığı genişletmez"
            .to_owned(),
    ];
    if alt < 0.0 {
        varsayimlar.push(
            "alt sınır negatif çıktı; fiziksel olarak 0 bayt olarak yorumlanmalıdır".to_owned(),
        );
    }
    if egim < 0.0 {
        varsayimlar.push("eğim negatif: yol küçülüyor, tahmin sonunda sıfıra çakılır".to_owned());
    }
    if n < 6 {
        varsayimlar.push(format!(
            "{} gözlem 6'nın altında: güven aralığı geniştir, tahmin kararlı değildir",
            n
        ));
    }

    Ok(Tahmin {
        gozlem: n,
        egim_bayt_gun: egim,
        serbestlik: sd,
        artik_sapma,
        tahmin,
        alt,
        ust,
        hedef_zaman,
        ileri_gun,
        varsayimlar,
        guven_duzeyi,
    })
}

/// Student t dağılımının iki taraflı 97.5% kritik değeri (serbestlik derecesine göre).
///
/// Küçük bir tablo elle tutulur; `n−2` çok büyük olduğunda normal yaklaşımının
/// değeri (`1.959964`) kullanılır. Bu bir **yaklaşımdır** ve varsayım listesinde
/// belirtilir; kriptografik veya istatistiksel bir doğrulama amacı taşımaz.
pub fn t_kritik(serbestlik: usize) -> f64 {
    match serbestlik {
        0 => f64::NAN,
        1 => 12.706,
        2 => 4.303,
        3 => 3.182,
        4 => 2.776,
        5 => 2.571,
        6 => 2.447,
        7 => 2.365,
        8 => 2.306,
        9 => 2.262,
        10 => 2.228,
        11 => 2.201,
        12 => 2.179,
        13 => 2.160,
        14 => 2.145,
        15 => 2.131,
        16 => 2.120,
        17 => 2.110,
        18 => 2.101,
        19 => 2.093,
        20 => 2.086,
        21 => 2.080,
        22 => 2.074,
        23 => 2.069,
        24 => 2.064,
        25 => 2.060,
        26 => 2.056,
        27 => 2.052,
        28 => 2.048,
        29 => 2.045,
        30 => 2.042,
        _ => 1.959964,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
// expect/unwrap test kodunda bilinçlidir: WORKER_CONTRACT.md § 4.2 bunları
// yalnızca testlerde serbest bırakır. Bir testin expect çağrısının
// başarısız olması, o iddianın yanlış olduğunun en net sinyalidir.
mod tests {
    use super::*;
    use crate::yol::Yol;
    use crate::zamancizelgesi::ZamanCizelgesi;

    fn cizelge_uret(noktalar: &[(i64, u64)]) -> ZamanCizelgesi {
        let mut liste = Vec::new();
        for (i, (zaman, bayt)) in noktalar.iter().enumerate() {
            liste.push(crate::zamancizelgesi::ZamanNoktasi {
                kimlik: format!("T{:04}", i + 1),
                zaman: *zaman,
                bayt: *bayt,
                fark: 0,
                bosluk: false,
                yol_yok: false,
            });
        }
        ZamanCizelgesi {
            yol: Yol::metin_yap("a"),
            noktalar: liste,
            bosluklar: Vec::new(),
        }
    }

    #[test]
    fn yetersiz_veride_hesaplanamaz() {
        let c = cizelge_uret(&[(1_000, 10), (2_000, 20)]);
        let sonuc = hesapla(&c, 30);
        assert_eq!(
            sonuc,
            Err(Hesaplanamama::YetersizVeri { var: 2, gereken: 3 })
        );
        assert!(sonuc
            .expect_err("hata beklenir")
            .aciklama()
            .contains("hesaplanamadı"));
    }

    #[test]
    fn tek_noktada_hesaplanamaz() {
        let c = cizelge_uret(&[(1_000, 10)]);
        assert_eq!(
            hesapla(&c, 30),
            Err(Hesaplanamama::YetersizVeri { var: 1, gereken: 3 })
        );
    }

    #[test]
    fn dogru_egim_lineer_seriden_bulunur() {
        // Günde 100 bayt artış.
        let gün = 86_400i64;
        let c = cizelge_uret(&[
            (0, 1_000),
            (gün, 1_100),
            (2 * gün, 1_200),
            (3 * gün, 1_300),
            (4 * gün, 1_400),
        ]);
        let t = hesapla(&c, 10).expect("hesaplanmalı");
        assert!(
            (t.egim_bayt_gun - 100.0).abs() < 1e-6,
            "eğim: {}",
            t.egim_bayt_gun
        );
        assert_eq!(t.gozlem, 5);
        assert_eq!(t.serbestlik, 3);
        // 14. günde 1_400 + 1_000 = 2_400
        assert!((t.tahmin - 2_400.0).abs() < 1e-6, "tahmin: {}", t.tahmin);
    }

    #[test]
    fn guven_araligi_tahmini_kapsiyor() {
        let gün = 86_400i64;
        let c = cizelge_uret(&[
            (0, 1_000),
            (gün, 1_500),
            (2 * gün, 1_100),
            (3 * gün, 1_800),
            (4 * gün, 1_300),
            (5 * gün, 2_000),
        ]);
        let t = hesapla(&c, 30).expect("hesaplanmalı");
        assert!(t.alt <= t.tahmin, "alt {} > tahmin {}", t.alt, t.tahmin);
        assert!(t.tahmin <= t.ust, "tahmin {} > ust {}", t.tahmin, t.ust);
        assert!(t.aralik_genisligi() > 0.0);
    }

    #[test]
    fn sabit_seride_aralik_sifira_yakin() {
        let gün = 86_400i64;
        let c = cizelge_uret(&[(0, 1_000), (gün, 1_000), (2 * gün, 1_000), (3 * gün, 1_000)]);
        let t = hesapla(&c, 10).expect("hesaplanmalı");
        assert!(t.egim_bayt_gun.abs() < 1e-9);
        assert!(t.artik_sapma < 1e-9);
        assert!(
            t.aralik_genisligi() < 1e-6,
            "genişlik: {}",
            t.aralik_genisligi()
        );
    }

    #[test]
    fn negatif_egim_sifira_cakilir() {
        let gün = 86_400i64;
        let c = cizelge_uret(&[(0, 3_000), (gün, 2_000), (2 * gün, 1_000)]);
        let t = hesapla(&c, 10).expect("hesaplanmalı");
        assert!(t.egim_bayt_gun < 0.0);
        assert!(t.varsayimlar.iter().any(|v| v.contains("eğim negatif")));
        assert_eq!(t.tahmin_bayt(), 0);
    }

    #[test]
    fn bosluklu_seride_tahmin_uretilmez() {
        let mut c = cizelge_uret(&[(0, 1_000), (86_400, 1_100), (2 * 86_400, 1_200)]);
        c.bosluklar.push("T0002".to_owned());
        let sonuc = hesapla(&c, 30);
        assert!(matches!(sonuc, Err(Hesaplanamama::Bosluklu { .. })));
        assert!(sonuc.expect_err("hata").aciklama().contains("uydurulmadı"));
    }

    #[test]
    fn ayni_zamanli_gozlemler_hesaplanamaz() {
        let c = cizelge_uret(&[(1_000, 10), (1_000, 20), (1_000, 30)]);
        assert_eq!(hesapla(&c, 30), Err(Hesaplanamama::DegersizAralik));
    }

    #[test]
    fn az_gozlemde_varsayim_listesi_uyarir() {
        let gün = 86_400i64;
        let c = cizelge_uret(&[(0, 1_000), (gün, 1_200), (2 * gün, 1_500)]);
        let t = hesapla(&c, 7).expect("hesaplanmalı");
        assert!(t.varsayimlar.iter().any(|v| v.contains("6'nın altında")));
        assert!(t.varsayimlar.iter().any(|v| v.contains("doğrusal")));
    }

    #[test]
    fn t_kritik_degerleri_bilincilerle_uyuyor() {
        assert!((t_kritik(1) - 12.706).abs() < 1e-9);
        assert!((t_kritik(10) - 2.228).abs() < 1e-9);
        assert!((t_kritik(100) - 1.959964).abs() < 1e-9);
        assert!(t_kritik(0).is_nan());
    }

    #[test]
    fn guven_duzeyi_ve_aralik_baytla_yorumlanabilir() {
        let gün = 86_400i64;
        let c = cizelge_uret(&[(0, 1_000), (gün, 1_200), (2 * gün, 1_400), (3 * gün, 1_600)]);
        let t = hesapla(&c, 30).expect("hesaplanmalı");
        assert!((t.guven_duzeyi - 0.95).abs() < 1e-9);
        assert_eq!(t.hedef_zaman, 3 * gün + 30 * gün);
        assert!(t.tahmin_bayt() > 0);
    }
}
