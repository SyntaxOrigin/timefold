//! Anlık görüntü başlığı, tam kayıt ve delta değişim kayıtları.
//!
//! Anlık görüntü deposunun JSONL biçimi iki tür satır taşır: bir **başlık**
//! satırı (`AnlikGoruntuBasi`) ve kayıt satırları. İlk anlık görüntünün kayıtları
//! `TamKayit`, sonrakiler `Degisim` olur. Ayrım başlıktaki `tur` alanından yapılır;
//! okuyucu satırları tek tek çözer ve şemaya uymayan satırı atlayıp sayar
//! (raporun "bozuk kayıt atlanır" kuralı).

use serde::{Deserialize, Serialize};

use crate::yol::Yol;

/// Depo satırının şema ayracı.
///
/// JSONL'de ayraç zorunlu değildir; bu alan, `serde_json` çözümlemesinde
/// "bu satır bizim şemamız mı" sorusunu ucuz yanıtlamak için taşınır.
pub const SEMA: &str = "timefold/1";

/// Bir kaydın türü.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KayitTuru {
    /// Bir dosya.
    Dosya,
    /// Bir klasör (kendi boyutu = alt ağacının toplam baytı).
    Klasor,
}

impl KayitTuru {
    /// `tur` alanının metinsel karşılığı.
    pub fn etiket(self) -> &'static str {
        match self {
            KayitTuru::Dosya => "dosya",
            KayitTuru::Klasor => "klasor",
        }
    }
}

/// Tam (bütün) anlık görüntüdeki tek bir yolun durumu.
///
/// `bayt` alanı klasörlerde **alt ağacın toplamını** taşır; bu, "hangi klasör ne
/// zaman şişti" sorusunu tek bir alanla yanıtlamak için raporun `dirTotals`
/// kararının sadeleştirilmiş hâlidir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TamKayit {
    /// Tarama köküne göreli kanonik yol.
    pub yol: Yol,
    /// Dosya mı klasör mü.
    pub tur: KayitTuru,
    /// Bayt cinsinden boyut (klasörlerde alt ağaç toplamı).
    pub bayt: u64,
    /// Dosya sistemi değişiklik zamanı, unix saniye (UTC).
    pub degisiklik: i64,
}

/// İki anlık görüntü arasındaki tek bir değişim.
///
/// JSON'da ayraç alanı `cesit` olarak adlandırılır (kayıt türü anlamındaki
/// `tur` alanıyla karışmasın diye). Yeni sürümlerde bu enum'a çeşit eklenebilir;
/// okuyucu bilinmeyen `cesit` değerini hata olarak sayar ve satırı atlar,
/// böylece eski sürüm yeni veriyi sessizce yanlış yorumlamaz.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cesit", rename_all = "snake_case")]
pub enum Degisim {
    /// Önceki anlık görüntüde olmayan, şimdi olan yol.
    Eklendi {
        /// Yeni yolun durumu.
        kayit: TamKayit,
    },
    /// Önceki anlık görüntüde olan, şimdi olmayan yol.
    Silindi {
        /// Silinen yol.
        yol: Yol,
        /// Silinen kaydın türü.
        tur: KayitTuru,
        /// Silinmeden önceki boyutu (delta'da korunur; boyut farkı hesabında gerekir).
        son_bayt: u64,
    },
    /// Yol korunmuş ama boyutu değişmiş.
    Degisti {
        /// Etkilenen yol.
        yol: Yol,
        /// Etkilenen kaydın türü.
        tur: KayitTuru,
        /// Önceki boyut.
        onceki_bayt: u64,
        /// Yeni boyut.
        yeni_bayt: u64,
    },
    /// Bir yol aynı boyutla başka bir yola taşınmış.
    ///
    /// **Bu çeşit şu anda üretilmez** (bkz. [`crate::delta`]): dosya sistemi bize
    /// "taşındı" bilgisini vermez ve aynı boyutlu silme + ekleme çiftini yeniden
    /// adlandırma saymak uydurma cevap olurdu. Varyant, ileride gerçek bir kanıt
    /// kaynağı (USN/inotify) eklendiğinde kullanılmak üzere şemada durur; o güne
    /// kadar okuyucu bu değeri görürse "değişmedi" kabul eder.
    AdDegisti {
        /// Eski yol.
        eski_yol: Yol,
        /// Yeni yol.
        yeni_yol: Yol,
        /// Taşınan kaydın yeni boyutu.
        #[allow(dead_code)]
        bayt: u64,
    },
}

impl Degisim {
    /// Değişimin etkilediği yolun **yeni** hâli (yoksa eski hâli).
    pub fn yol(&self) -> &Yol {
        match self {
            Degisim::Eklendi { kayit } => &kayit.yol,
            Degisim::Silindi { yol, .. } => yol,
            Degisim::Degisti { yol, .. } => yol,
            Degisim::AdDegisti { yeni_yol, .. } => yeni_yol,
        }
    }

    /// Değişimin etkilediği yolun **eski** hâli (yoksa yeni hâli).
    pub fn eski_yol(&self) -> &Yol {
        match self {
            Degisim::Eklendi { kayit } => &kayit.yol,
            Degisim::Silindi { yol, .. } => yol,
            Degisim::Degisti { yol, .. } => yol,
            Degisim::AdDegisti { eski_yol, .. } => eski_yol,
        }
    }
    /// Değişimin bayt etkisini döndürür.
    ///
    /// Silinen yol için negatif işaret kullanılır; böylece "net büyüme" tek bir
    /// toplamla ifade edilebilir.
    ///
    /// **Klasör kayıtları `0` döndürür.** Klasörlerin `bayt` alanı alt ağacın
    /// **toplamıdır**; onları da toplama katmak, ağaç derinliği kadar katla
    /// sayım yapardı. Bu yüzden net etki yalnızca **dosya** kayıtlarından
    /// hesaplanır ve aynı klasör silinse bile altındaki dosyaların baytı doğru
    /// sayılır.
    pub fn bayt_etkisi(&self) -> i64 {
        match self {
            Degisim::Eklendi { kayit } => {
                if kayit.tur == KayitTuru::Dosya {
                    kayit.bayt as i64
                } else {
                    0
                }
            }
            Degisim::Silindi { tur, son_bayt, .. } => {
                if *tur == KayitTuru::Dosya {
                    -(*son_bayt as i64)
                } else {
                    0
                }
            }
            Degisim::Degisti {
                tur,
                onceki_bayt,
                yeni_bayt,
                ..
            } => {
                if *tur == KayitTuru::Dosya {
                    *yeni_bayt as i64 - *onceki_bayt as i64
                } else {
                    0
                }
            }
            Degisim::AdDegisti { .. } => 0,
        }
    }

    /// Serileştirilmiş hâlinde kullanılacak kısa etiket.
    pub fn etiket(&self) -> &'static str {
        match self {
            Degisim::Eklendi { .. } => "eklendi",
            Degisim::Silindi { .. } => "silindi",
            Degisim::Degisti { .. } => "degisti",
            Degisim::AdDegisti { .. } => "ad_degisti",
        }
    }

    /// Serileştirilmiş hâlinde kullanılacak uzun etiket (test çıktısı ve JSON).
    pub fn uzun_etiket(&self) -> &'static str {
        match self {
            Degisim::Eklendi { .. } => "eklendi",
            Degisim::Silindi { .. } => "silindi",
            Degisim::Degisti { .. } => "degisti",
            Degisim::AdDegisti { .. } => "ad_degisti",
        }
    }
}

/// Anlık görüntü dosyasının ilk satırı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnlikGoruntuBasi {
    /// Şema ayracı, [`SEMA`] sabiti.
    pub sema: String,
    /// Biçim sürümü; [`crate::BICIM_SURUMU`] ile eşleşmelidir.
    pub surum: u32,
    /// Anlık görüntünün kimliği (depoda benzersiz, sıralı dize).
    pub kimlik: String,
    /// Bu anlık görüntünün farkı hangi anlık görüntüye göre hesaplandı.
    pub temel: Option<String>,
    /// Taramanın UTC unix saniye cinsinden zamanı.
    pub zaman: i64,
    /// Ağaç kökünün o anki mutlak yolu (taşınabilirlik uyarısı için).
    pub kok: String,
    /// Kayıt satırı sayısı (başlık hariç).
    pub kayit_sayisi: u64,
    /// Kayıtların bayt cinsinden sağlama toplamı (FNV-1a 64).
    pub saglama: u64,
    /// Kayıt türü: `tam` (ilk tarama) ya da `fark` (sonrakiler).
    pub tur: BasitTur,
    /// Tarama kısmi miydi (izin hatası, örnekleme, sembolik bağ atlama).
    pub kismi: bool,
    /// Kısmi tarama gerekçesi (kısmi değilse boş).
    pub kismi_gerekce: String,
    /// Örnekleme kullanıldı mı.
    pub orneklendi: bool,
    /// Taranan toplam giriş sayısı (örnekleme sonrası saklanan değil).
    pub taranan: u64,
}

/// Anlık görüntünün türü.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasitTur {
    /// İlk tarama: tüm yollar.
    Tam,
    /// Sonraki tarama: yalnızca değişen yollar.
    Fark,
}

/// FNV-1a 64 bit sağlama toplamı çalıştırıcısı.
///
/// Bu bir kimlik doğrulama (MAC) değildir; yalnızca **kayıt sırasının ve
/// içeriğin** değişip değişmediğini yakalamak içindir. Kriptografik bir sağlama
/// değildir ve güvenlik sınırı olarak kullanılmaz; `WORKER_CONTRACT.md` § 3.2
/// gereği kripto crate'i (09 için) kullanılamaz, bu yüzden bilinçli olarak
/// hızlı ve kriptografik olmayan bir toplam seçilmiştir.
#[derive(Debug, Clone)]
pub struct Saglama {
    deger: u64,
}

impl Saglama {
    /// FNV-1a başlangıç ofsetini kullanarak yeni çalıştırıcı üretir.
    pub fn yeni() -> Self {
        Saglama {
            deger: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// Bayt dizisini toplama dâhil eder.
    pub fn bayt_ekle(&mut self, bayt: &[u8]) {
        for b in bayt {
            self.deger ^= *b as u64;
            self.deger = self.deger.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// Tam kaydın kanonik alanlarını toplama dâhil eder.
    pub fn kayit_ekle(&mut self, kayit: &TamKayit) {
        self.yol_ekle(&kayit.yol);
        self.bayt_ekle(kayit.tur.etiket().as_bytes());
        self.bayt_ekle(&kayit.bayt.to_le_bytes());
        self.bayt_ekle(&kayit.degisiklik.to_le_bytes());
    }

    /// Yolun kanonik metnini toplama dâhil eder.
    pub fn yol_ekle(&mut self, yol: &Yol) {
        self.bayt_ekle(yol.metin().as_bytes());
    }

    /// Güncel toplamı döndürür.
    pub fn deger(&self) -> u64 {
        self.deger
    }
}

impl Default for Saglama {
    fn default() -> Self {
        Saglama::yeni()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
// expect/unwrap test kodunda bilinçlidir: WORKER_CONTRACT.md § 4.2 bunları
// yalnızca testlerde serbest bırakır. Bir testin expect çağrısının
// başarısız olması, o iddianın yanlış olduğunun en net sinyalidir.
mod tests {
    use super::*;

    fn ornek() -> TamKayit {
        TamKayit {
            yol: Yol::metin_yap("a/b.txt"),
            tur: KayitTuru::Dosya,
            bayt: 12,
            degisiklik: 1_000,
        }
    }

    #[test]
    fn tam_kayit_json_gidis_donusu_korunuyor() {
        let k = ornek();
        let metin = serde_json::to_string(&k).expect("serileştirilebilir");
        let geri: TamKayit = serde_json::from_str(&metin).expect("çözümlenebilir");
        assert_eq!(k, geri);
    }

    #[test]
    fn degisim_turleri_json_gidis_donusu_korunuyor() {
        let liste = vec![
            Degisim::Eklendi { kayit: ornek() },
            Degisim::Silindi {
                yol: Yol::metin_yap("a/c.txt"),
                tur: KayitTuru::Dosya,
                son_bayt: 40,
            },
            Degisim::Degisti {
                yol: Yol::metin_yap("a/d.txt"),
                tur: KayitTuru::Dosya,
                onceki_bayt: 10,
                yeni_bayt: 25,
            },
            Degisim::AdDegisti {
                eski_yol: Yol::metin_yap("a/e.txt"),
                yeni_yol: Yol::metin_yap("a/f.txt"),
                bayt: 7,
            },
        ];
        for d in liste {
            let metin = serde_json::to_string(&d).expect("serileştirilebilir");
            let geri: Degisim = serde_json::from_str(&metin).expect("çözümlenebilir");
            assert_eq!(d, geri);
        }
    }

    #[test]
    fn bilinmeyen_degisim_cesidi_cozumlenmiyor() {
        // `cesit` ayracı bilinmeyen bir değer: eski sürüm yeni veriyi sessizce
        // yanlış yorumlamamalı, hata vermelidir.
        let metin = r#"{"cesit":"uydurma","yol":"a"}"#;
        assert!(serde_json::from_str::<Degisim>(metin).is_err());
    }

    #[test]
    fn klasor_kaydinin_bayt_etkisi_sifirdir() {
        // Klasör baytı alt ağacın **toplamıdır**; toplama katılırsa ağaç
        // derinliği kadar katla sayım olurdu.
        let klasor = TamKayit {
            yol: Yol::metin_yap("a"),
            tur: KayitTuru::Klasor,
            bayt: 9_999,
            degisiklik: 0,
        };
        assert_eq!(
            Degisim::Eklendi { kayit: klasor }.bayt_etkisi(),
            0,
            "klasör eklenmesi net baytı değiştirmez"
        );
        assert_eq!(
            Degisim::Degisti {
                yol: Yol::metin_yap("a"),
                tur: KayitTuru::Klasor,
                onceki_bayt: 10,
                yeni_bayt: 20,
            }
            .bayt_etkisi(),
            0
        );
    }

    #[test]
    fn degisim_bayt_etkisi_isaretli() {
        assert_eq!(Degisim::Eklendi { kayit: ornek() }.bayt_etkisi(), 12);
        assert_eq!(
            Degisim::Silindi {
                yol: Yol::kok(),
                tur: KayitTuru::Dosya,
                son_bayt: 30
            }
            .bayt_etkisi(),
            -30
        );
        assert_eq!(
            Degisim::Degisti {
                yol: Yol::kok(),
                tur: KayitTuru::Dosya,
                onceki_bayt: 5,
                yeni_bayt: 9
            }
            .bayt_etkisi(),
            4
        );
        assert_eq!(
            Degisim::AdDegisti {
                eski_yol: Yol::kok(),
                yeni_yol: Yol::metin_yap("b"),
                bayt: 3
            }
            .bayt_etkisi(),
            0
        );
    }

    #[test]
    fn degisim_yol_hatasi_yeni_ve_eski_yolu_veriyor() {
        let d = Degisim::AdDegisti {
            eski_yol: Yol::metin_yap("eski"),
            yeni_yol: Yol::metin_yap("yeni"),
            bayt: 1,
        };
        assert_eq!(d.yol().metin(), "yeni");
        assert_eq!(d.eski_yol().metin(), "eski");
        assert_eq!(d.etiket(), d.uzun_etiket());
    }

    #[test]
    fn bilinmeyen_degisim_turu_cozumlenmiyor() {
        let metin = r#"{"tur":"uydurma","yol":"a"}"#;
        assert!(serde_json::from_str::<Degisim>(metin).is_err());
    }

    #[test]
    fn baslik_json_gidis_donusu_korunuyor() {
        let b = AnlikGoruntuBasi {
            sema: SEMA.to_owned(),
            surum: crate::BICIM_SURUMU,
            kimlik: "T0002".to_owned(),
            temel: Some("T0001".to_owned()),
            zaman: 1_791_285_907,
            kok: "C:/veri".to_owned(),
            kayit_sayisi: 3,
            saglama: 42,
            tur: BasitTur::Fark,
            kismi: true,
            kismi_gerekce: "1 izin hatası".to_owned(),
            orneklendi: false,
            taranan: 9,
        };
        let metin = serde_json::to_string(&b).expect("serileştirilebilir");
        let geri: AnlikGoruntuBasi = serde_json::from_str(&metin).expect("çözümlenebilir");
        assert_eq!(b, geri);
    }

    #[test]
    fn saglama_siraya_duyarli() {
        let a = ornek();
        let mut b = ornek();
        b.bayt = 13;

        let mut s1 = Saglama::yeni();
        s1.kayit_ekle(&a);
        let mut s2 = Saglama::yeni();
        s2.kayit_ekle(&b);
        assert_ne!(s1.deger(), s2.deger());
    }

    #[test]
    fn saglama_ayni_kayit_ayni_toplam() {
        let a = ornek();
        let mut s1 = Saglama::yeni();
        s1.kayit_ekle(&a);
        let mut s2 = Saglama::yeni();
        s2.yol_ekle(&a.yol);
        s2.bayt_ekle(b"dosya");
        s2.bayt_ekle(&a.bayt.to_le_bytes());
        s2.bayt_ekle(&a.degisiklik.to_le_bytes());
        assert_eq!(s1.deger(), s2.deger());
    }

    #[test]
    fn kayit_turu_etiketleri_kucuk_harfle_ayniyor() {
        let metin = serde_json::to_string(&ornek()).expect("serileştirilebilir");
        assert!(metin.contains("\"dosya\""));
        assert_eq!(KayitTuru::Klasor.etiket(), "klasor");
    }
}
