//! RFC 3339 (UTC) zaman damgası üretimi ve ayrıştırma.
//!
//! Bağımlılık politikası `chrono` ve `time` crate'lerini yasaklar; bu modül
//! yalnızca `std::time` kullanır. Hesap, Howard Hinnant'ın kamu malı
//! (`http://www.eda.com/other-resources/htsjump/`) "three r's" (right / roll /
//! round) takvim algoritmasının standart biçimidir ve RFC 3339'un proleptik
//! Gregoryen takvimiyle birebir uyumludur.
//!
//! Tüm damgalar **UTC** olarak saklanır; yerel gösterim yalnızca terminal
//! çıktısının başlığındadır (raporun R11 riski).

use std::time::{SystemTime, UNIX_EPOCH};

use crate::hata::{Hata, Sonuc};

/// `1970-01-01T00:00:00Z` unix saniye değeri.
pub const UNIX_EPOCH_SANIYE: i64 = 0;

/// Günlük dönüşüm tablosunun kullandığı üç sayı.
const GUN: i64 = 86_400;

/// Metin biçimine çevrilecek tek bir UTC anı.
///
/// Alanlar aralık dışındaysa metin üretimi `Hata::GecersizZaman` ile başarısız olur;
/// "uyduğu gibi yazmak" yerine hata vermek tercih edilir, çünkü bozuk bir zaman
/// damgası zaman çizelgesinde sıralamayı bozar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zaman {
    /// Yıl (proleptik Gregoryen).
    pub yil: i64,
    /// Ay, `1..=12`.
    pub ay: u32,
    /// Gün, `1..=31`.
    pub gun: u32,
    /// Saat, `0..=23`.
    pub saat: u32,
    /// Dakika, `0..=59`.
    pub dakika: u32,
    /// Saniye, `0..=60` (artık saniye için 60 kabul edilir).
    pub saniye: u32,
}

impl Zaman {
    /// Unix saniye değerinden UTC anı üretir.
    pub fn unix_saniyeden(saniye: i64) -> Self {
        let gun_sayisi = saniye.div_euclid(GUN);
        let gun_ici = saniye.rem_euclid(GUN);
        let (yil, ay, gun) = tarihsel_gun_kir(gun_sayisi);
        Zaman {
            yil,
            ay,
            gun,
            saat: (gun_ici / 3600) as u32,
            dakika: ((gun_ici % 3600) / 60) as u32,
            saniye: (gun_ici % 60) as u32,
        }
    }

    /// RFC 3339 metninden UTC anını çözer.
    pub fn rfc3339_ayir(metin: &str) -> Sonuc<Self> {
        let hata = |ayrinti: &str| Hata::GecersizZaman {
            ayrinti: format!("{} (beklenen biçim: 2026-09-29T13:05:07Z)", ayrinti),
        };
        if metin.len() < 20 {
            return Err(hata("metin çok kısa"));
        }
        let bayt = metin.as_bytes();
        let rakam = |aralik: std::ops::Range<usize>| -> Sonuc<i64> {
            let parca = metin.get(aralik.clone()).unwrap_or("");
            if parca.len() != aralik.end - aralik.start {
                return Err(hata("sayısal alan eksik"));
            }
            parca
                .parse::<i64>()
                .map_err(|_| hata("sayısal alan çözümlenemedi"))
        };
        if bayt[4] != b'-' || bayt[7] != b'-' || (bayt[10] != b'T' && bayt[10] != b't') {
            return Err(hata("ayraçlar yanlış"));
        }
        if bayt[13] != b':' || bayt[16] != b':' {
            return Err(hata("saat ayraçları yanlış"));
        }
        let son = &metin[19..];
        let ofset_dakika = if son == "Z" || son == "z" {
            0
        } else if son.len() == 6
            && (son.starts_with('+') || son.starts_with('-'))
            && son.as_bytes()[3] == b':'
        {
            let isaret = if son.starts_with('-') { -1 } else { 1 };
            let saat = son[1..3]
                .parse::<i64>()
                .map_err(|_| hata("ofset saati çözümlenemedi"))?;
            let dakika = son[4..6]
                .parse::<i64>()
                .map_err(|_| hata("ofset dakikası çözümlenemedi"))?;
            if saat > 23 || dakika > 59 {
                return Err(hata("ofset değeri geçersiz"));
            }
            isaret * (saat * 60 + dakika)
        } else {
            return Err(hata("UTC ofseti çözümlenemedi"));
        };
        let z = Zaman {
            yil: rakam(0..4)?,
            ay: rakam(5..7)? as u32,
            gun: rakam(8..10)? as u32,
            saat: rakam(11..13)? as u32,
            dakika: rakam(14..16)? as u32,
            saniye: rakam(17..19)? as u32,
        };
        if !(1..=12).contains(&z.ay)
            || !(1..=31).contains(&z.gun)
            || z.saat > 23
            || z.dakika > 59
            || z.saniye > 60
        {
            return Err(hata("alan değeri geçersiz"));
        }
        Ok(z.ofset_uygula(ofset_dakika))
    }

    /// Unix saniye değerine çevirir.
    pub fn unix_saniyeye(&self) -> i64 {
        let gun = proleptik_gun_sayisi(self.yil, self.ay as i64, self.gun as i64);
        gun * GUN + self.saat as i64 * 3600 + self.dakika as i64 * 60 + self.saniye as i64
    }

    /// Verilen UTC ofsetini (dakika) uygulayarak UTC karşılığını döndürür.
    ///
    /// `+01:00` ofsetli bir zaman damgası, UTC'de bir saat **önce**dir; bu yüzden
    /// ofset düşülür.
    pub fn ofset_uygula(&self, ofset_dakika: i64) -> Self {
        let toplam = self.unix_saniyeye() - ofset_dakika * 60;
        Zaman::unix_saniyeden(toplam)
    }

    /// RFC 3339 metnini döndürür.
    pub fn rfc3339_metne(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            self.yil, self.ay, self.gun, self.saat, self.dakika, self.saniye
        )
    }

    /// UTC ay anahtarını (`2026-09`) döndürür; aylık kümelemenin temelidir.
    pub fn ay_anahtari(&self) -> String {
        format!("{:04}-{:02}", self.yil, self.ay)
    }

    /// UTC gün anahtarını (`2026-09-29`) döndürür; günlük saklamanın temelidir.
    pub fn gun_anahtari(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.yil, self.ay, self.gun)
    }
}

/// Sistem saatinin şu anki unix saniye değerini döndürür.
///
/// Saat dilgesi veya yaz saati etkisi yoktur; `SystemTime` zaten UTC'dir.
pub fn simdi_unix_saniye() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(fark) => fark.as_secs() as i64,
        // Saat 1970 öncesinde ayarlanmışsa negatif fark oluşur; işaretsiz taşmayı
        // önlemek için i64 aralığında kalınır.
        Err(hata) => -(hata.duration().as_secs() as i64),
    }
}

/// Unix saniye değerini RFC 3339 metnine çevirir.
pub fn unix_saniye_rfc3339(saniye: i64) -> String {
    Zaman::unix_saniyeden(saniye).rfc3339_metne()
}

/// RFC 3339 metnini unix saniye değerine çevirir.
pub fn rfc3339_unix_saniye(metin: &str) -> Sonuc<i64> {
    Ok(Zaman::rfc3339_ayir(metin)?.unix_saniyeye())
}

/// Verilen unix saniyesini içeren ayın başlangıcını döndürür (UTC).
pub fn ay_baslangici(saniye: i64) -> i64 {
    let z = Zaman::unix_saniyeden(saniye);
    proleptik_gun_sayisi(z.yil, z.ay as i64, 1) * GUN
}

/// İki unix saniyesi arasındaki farkı gün sayısı olarak verir (işaretli).
pub fn gun_farki(baslangic: i64, bitis: i64) -> f64 {
    (bitis - baslangic) as f64 / GUN as f64
}

/// Proleptik Gregoryen takvimde gün sayısından (yıl, ay, gün) üretir.
fn tarihsel_gun_kir(gun_sayisi: i64) -> (i64, u32, u32) {
    let z = gun_sayisi + 719_468;
    let era = z.div_euclid(146_097);
    let yil_ici = z.rem_euclid(146_097);
    let yil_of_era = (yil_ici - yil_ici / 1_460 + yil_ici / 36_524 - yil_ici / 146_096) / 365;
    let yil = yil_of_era + era * 400;
    let gun_yili = yil_ici - (365 * yil_of_era + yil_of_era / 4 - yil_of_era / 100);
    let ay_fazlasi = (5 * gun_yili + 2) / 153;
    let gun = gun_yili - (153 * ay_fazlasi + 2) / 5 + 1;
    let ay = if ay_fazlasi < 10 {
        ay_fazlasi + 3
    } else {
        ay_fazlasi - 9
    };
    let yil = if ay <= 2 { yil + 1 } else { yil };
    (yil, ay as u32, gun as u32)
}

/// (yıl, ay, gün) üçlüsünden 1970-01-01'den bu yana geçen gün sayısını üretir.
fn proleptik_gun_sayisi(yil: i64, ay: i64, gun: i64) -> i64 {
    let y = yil - if ay <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let y_of_era = y - era * 400;
    let ay_fazlasi = if ay > 2 { ay - 3 } else { ay + 9 };
    let gun_yili = (153 * ay_fazlasi + 2) / 5 + gun - 1;
    let gun_yilin_gunu = y_of_era * 365 + y_of_era / 4 - y_of_era / 100 + gun_yili;
    era * 146_097 + gun_yilin_gunu - 719_468
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
// expect/unwrap test kodunda bilinçlidir: WORKER_CONTRACT.md § 4.2 bunları
// yalnızca testlerde serbest bırakır. Bir testin expect çağrısının
// başarısız olması, o iddianın yanlış olduğunun en net sinyalidir.
mod tests {
    use super::*;

    #[test]
    fn unix_baslangic_epocha_esit() {
        let z = Zaman::unix_saniyeden(UNIX_EPOCH_SANIYE);
        assert_eq!(z.yil, 1970);
        assert_eq!(z.ay, 1);
        assert_eq!(z.gun, 1);
        assert_eq!(z.saat, 0);
    }

    #[test]
    fn bilinen_an_rfc3339_donusu_dogru() {
        // 1 700 000 000 = 2023-11-14T22:13:20Z (yaygın olarak bilinen değer).
        assert_eq!(unix_saniye_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(unix_saniye_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_saniye_rfc3339(86_400), "1970-01-02T00:00:00Z");
        assert_eq!(unix_saniye_rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn gidiş_dönüş_korunuyor() {
        for saniye in [0_i64, 1, 86_399, 86_400, 951_782_400, 1_791_285_907] {
            let metin = unix_saniye_rfc3339(saniye);
            let geri = rfc3339_unix_saniye(&metin).expect("ayrıştırılmalı");
            assert_eq!(geri, saniye, "metin: {}", metin);
        }
    }

    #[test]
    fn negatif_zaman_epochtan_once_ayiriliyor() {
        assert_eq!(unix_saniye_rfc3339(-1), "1969-12-31T23:59:59Z");
        assert_eq!(rfc3339_unix_saniye("1969-12-31T23:59:59Z").unwrap(), -1);
    }

    #[test]
    fn ofsetli_zaman_utcye_cevriliyor() {
        assert_eq!(rfc3339_unix_saniye("1970-01-01T01:00:00+01:00").unwrap(), 0);
        assert_eq!(rfc3339_unix_saniye("1969-12-31T23:00:00-01:00").unwrap(), 0);
    }

    #[test]
    fn ofset_uygulama_yerel_saati_utcye_cevirir() {
        let yerel = Zaman {
            yil: 2026,
            ay: 9,
            gun: 29,
            saat: 15,
            dakika: 5,
            saniye: 7,
        };
        // 15:05:07 +02:00 = 13:05:07 UTC
        assert_eq!(
            yerel.ofset_uygula(120).rfc3339_metne(),
            "2026-09-29T13:05:07Z"
        );
        // 15:05:07 -02:00 = 17:05:07 UTC (ofset, UTC'yi ifade eder)
        assert_eq!(
            yerel.ofset_uygula(-120).rfc3339_metne(),
            "2026-09-29T17:05:07Z"
        );
    }

    #[test]
    fn bozuk_zaman_dizesi_hata_veriyor() {
        assert!(rfc3339_unix_saniye("2026-09-29").is_err());
        assert!(rfc3339_unix_saniye("2026-13-29T00:00:00Z").is_err());
        assert!(rfc3339_unix_saniye("2026-09-29T25:00:00Z").is_err());
        assert!(rfc3339_unix_saniye("xx").is_err());
        assert!(rfc3339_unix_saniye("2026-09-29T13:05:07").is_err());
    }

    #[test]
    fn ay_ve_gun_anahtarlari_grup_laniyor() {
        assert_eq!(
            Zaman::unix_saniyeden(1_700_000_000).ay_anahtari(),
            "2023-11"
        );
        assert_eq!(
            Zaman::unix_saniyeden(1_700_000_000).gun_anahtari(),
            "2023-11-14"
        );
        assert_eq!(Zaman::unix_saniyeden(0).gun_anahtari(), "1970-01-01");
        assert_eq!(unix_saniye_rfc3339(0).len(), 20);
    }

    #[test]
    fn ay_baslangici_ayin_ilk_gunune_isaret_ediyor() {
        let t = 1_700_000_000; // 2023-11-14T22:13:20Z
        let bas = ay_baslangici(t);
        assert!(bas <= t);
        assert_eq!(unix_saniye_rfc3339(bas), "2023-11-01T00:00:00Z");
        // Ay başlangıcı, ayın ilk günü 00:00:00'dır.
        let z = Zaman::unix_saniyeden(bas);
        assert_eq!((z.ay, z.gun, z.saat, z.dakika, z.saniye), (11, 1, 0, 0, 0));
    }

    #[test]
    fn gun_farki_isaretli() {
        assert!((gun_farki(0, 86_400) - 1.0).abs() < 1e-9);
        assert!((gun_farki(86_400, 0) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn simdi_pozitif_ve_yakin() {
        let s = simdi_unix_saniye();
        assert!(s > 1_700_000_000);
    }
}
