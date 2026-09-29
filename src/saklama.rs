//! Örnekli saklama politikası: son N anlık görüntü ve günlük seyreltme.
//!
//! Raporun R1 riski: sıkıştırma olmadan depo hızla büyür. Azaltma, **örneklemli
//! saklama**dır:
//!
//! 1. `--tut N` → son N anlık görüntü korunur.
//! 2. `--gunluk` → bundan eski anlık görüntüler arasından, **her UTC günü için
//!    en fazla bir tane** bırakılır (günlük kapsama).
//!
//! Politika **yalnızca aracın kendi deposunda** uygulanır. Silinen bayt miktarı
//! her seferinde raporlanır (raporun R5 azaltması) ve kullanıcı dosyalarına
//! hiç dokunulmaz.
//!
//! Politikanın ikinci yarısı, raporun S6 senaryosudur: "Örneklenmiş eski
//! anlık görüntüler silinir, günlük veri korunur."

use std::collections::BTreeMap;

use crate::zaman::Zaman;

/// Örnekli saklama politikası.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaklamaPolitikasi {
    /// Korunacak en yeni anlık görüntü sayısı.
    ///
    /// `None` → sınır yoktur (yalnızca günlük kural uygulanır).
    pub tut: Option<usize>,
    /// Günlük seyreltme açık mı?
    pub gunluk: bool,
}

impl SaklamaPolitikasi {
    /// Hiçbir kural içermeyen politika (saklama yapılmaz).
    pub fn yoksun() -> Self {
        SaklamaPolitikasi {
            tut: None,
            gunluk: false,
        }
    }

    /// Son `n` anlık görüntüyü koruyan politika.
    pub fn son_n(n: usize) -> Self {
        SaklamaPolitikasi {
            tut: Some(n),
            gunluk: false,
        }
    }

    /// Son `n` anlık görüntüyü + günlük seyreltmeyi uygulayan politika.
    pub fn gunluk_ile(n: usize) -> Self {
        SaklamaPolitikasi {
            tut: Some(n),
            gunluk: true,
        }
    }

    /// Politikanın herhangi bir iş kuralı var mı?
    pub fn etkin_mi(&self) -> bool {
        self.tut.is_some() || self.gunluk
    }

    /// Politika açıklaması (rapor başlığı ve JSON çıktısı için).
    pub fn aciklama(&self) -> String {
        match (self.tut, self.gunluk) {
            (None, false) => "kural yok".to_owned(),
            (Some(n), false) => format!("son {} anlık görüntü korunur", n),
            (None, true) => "günlük seyreltme (her gün en fazla bir tane)".to_owned(),
            (Some(n), true) => format!(
                "son {} anlık görüntü korunur, eskiler günlük seyreltilir",
                n
            ),
        }
    }

    /// Verilen kimlik listesinden **silinecek** kimlikleri, oluş sırasına göre
    /// döndürür.
    ///
    /// `kimlikler` ve `zamanlar` paralel dizilerdir: `zamanlar[i]` kimliklerin
    /// `[i]`'inci anlık görüntüsünün UTC unix saniyesidir.
    pub fn silinecekler(&self, zamanlar: &[(String, i64)]) -> Vec<String> {
        if !self.etkin_mi() || zamanlar.is_empty() {
            return Vec::new();
        }
        let toplam = zamanlar.len();
        let korunacak_esik = match self.tut {
            Some(n) => toplam.saturating_sub(n),
            None => 0,
        };

        let mut silinecek: Vec<String> = Vec::new();
        if !self.gunluk {
            // Kural yoksa: korunan pencerenin dışındaki her şey gider.
            for (kimlik, _) in zamanlar.iter().take(korunacak_esik) {
                silinecek.push(kimlik.clone());
            }
            return silinecek;
        }

        // Günlük seyreltme: korunan pencerenin dışındaki anlık görüntülerden,
        // **her UTC günü için en yeni tane** korunur. Böylece günlük kapsama
        // bozulmaz; yalnızca aynı günün eski tekrarları silinir.
        let mut gun_basi: BTreeMap<String, i64> = BTreeMap::new();
        for (_, zaman) in zamanlar.iter().take(korunacak_esik) {
            let gun = Zaman::unix_saniyeden(*zaman).gun_anahtari();
            // `zamanlar` oluş sırasına göre artan olduğu için son yazan kazanır.
            gun_basi.insert(gun, *zaman);
        }
        let en_eski_kimlik = zamanlar
            .first()
            .map(|(kimlik, _)| kimlik.clone())
            .unwrap_or_default();

        for (kimlik, zaman) in zamanlar.iter().take(korunacak_esik) {
            if *kimlik == en_eski_kimlik {
                // Zincirin başı her zaman korunur: "geçmiş" sıfıra düşmesin.
                continue;
            }
            let gun = Zaman::unix_saniyeden(*zaman).gun_anahtari();
            if gun_basi.get(&gun) == Some(zaman) {
                continue;
            }
            silinecek.push(kimlik.clone());
        }
        silinecek
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
// expect/unwrap test kodunda bilinçlidir: WORKER_CONTRACT.md § 4.2 bunları
// yalnızca testlerde serbest bırakır. Bir testin expect çağrısının
// başarısız olması, o iddianın yanlış olduğunun en net sinyalidir.
mod tests {
    use super::*;

    fn gun_saniye(gun: u64) -> i64 {
        (gun * 86_400) as i64
    }

    #[test]
    fn kuralsiz_politika_hicbir_sey_silmez() {
        let p = SaklamaPolitikasi::yoksun();
        let veri = vec![
            ("T0001".to_owned(), gun_saniye(1)),
            ("T0002".to_owned(), gun_saniye(2)),
        ];
        assert!(p.silinecekler(&veri).is_empty());
        assert!(!p.etkin_mi());
        assert_eq!(p.aciklama(), "kural yok");
    }

    #[test]
    fn son_n_koruyor_eskileri_siliyor() {
        let p = SaklamaPolitikasi::son_n(2);
        let veri: Vec<(String, i64)> = (1..=5)
            .map(|i| (format!("T{:04}", i), gun_saniye(i)))
            .collect();
        let silinecek = p.silinecekler(&veri);
        assert_eq!(silinecek, vec!["T0001", "T0002", "T0003"]);
        assert!(p.etkin_mi());
    }

    #[test]
    fn son_n_listeden_fazlaysa_hicbir_sey_silinmez() {
        let p = SaklamaPolitikasi::son_n(10);
        let veri: Vec<(String, i64)> = (1..=3)
            .map(|i| (format!("T{:04}", i), gun_saniye(i)))
            .collect();
        assert!(p.silinecekler(&veri).is_empty());
    }

    #[test]
    fn gunluk_kural_her_gunden_bir_tane_koruyor() {
        let p = SaklamaPolitikasi::gunluk_ile(2);
        // 1. gün: 3 tarama, 2. gün: 3 tarama, 3. gün: 2 tarama.
        let veri: Vec<(String, i64)> = vec![
            ("T0001".to_owned(), gun_saniye(1)),
            ("T0002".to_owned(), gun_saniye(1) + 100),
            ("T0003".to_owned(), gun_saniye(1) + 200),
            ("T0004".to_owned(), gun_saniye(2)),
            ("T0005".to_owned(), gun_saniye(2) + 100),
            ("T0006".to_owned(), gun_saniye(3)),
            ("T0007".to_owned(), gun_saniye(3) + 100),
        ];
        let silinecek = p.silinecekler(&veri);
        // Son 2 (T0006, T0007) korunur. Eski kısımda gün 1'in en yenisi (T0003)
        // ve gün 2'nin en yenisi (T0005) korunur; en eski (T0001) de korunur.
        assert!(silinecek.contains(&"T0002".to_owned()));
        assert!(silinecek.contains(&"T0004".to_owned()));
        assert!(
            !silinecek.contains(&"T0001".to_owned()),
            "en eski korunmalı"
        );
        assert!(!silinecek.contains(&"T0003".to_owned()));
        assert!(!silinecek.contains(&"T0005".to_owned()));
    }

    #[test]
    fn gunluk_tek_gunda_tum_veriler_korunur() {
        let p = SaklamaPolitikasi::gunluk_ile(1);
        let veri: Vec<(String, i64)> = (1..=3)
            .map(|i| (format!("T{:04}", i), gun_saniye(1)))
            .collect();
        let silinecek = p.silinecekler(&veri);
        // Aynı gün: en eski hariç her şey aynı gün sayılır; en eski korunur.
        assert!(silinecek.len() < 3);
        assert!(!silinecek.contains(&"T0001".to_owned()));
    }

    #[test]
    fn bos_listede_hicbir_sey_silinmez() {
        let p = SaklamaPolitikasi::son_n(1);
        assert!(p.silinecekler(&[]).is_empty());
    }

    #[test]
    fn politika_aciklamasi_her_durumu_yineler() {
        assert_eq!(SaklamaPolitikasi::yoksun().aciklama(), "kural yok");
        assert!(SaklamaPolitikasi::son_n(5).aciklama().contains("son 5"));
        assert!(SaklamaPolitikasi::gunluk_ile(5)
            .aciklama()
            .contains("günlük"));
    }
}
