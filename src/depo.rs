//! Sürümlü anlık görüntü deposu: indeks, yazma/okuma, undo/redo ve restore.
//!
//! Depo düzeni (raporun "klasör yapısı" şemasının sadeleştirilmiş hâli):
//!
//! ```text
//! <depo>/
//!   index.json                    sürüm + kimlik listesi
//!   anlik_goruntuler/
//!     T0001.tam.jsonl             ilk tarama: tüm yollar
//!     T0002.fark.jsonl            sonraki taramalar: yalnızca değişenler
//!     T0002.tam.jsonl             zincir yeniden kurulabilsin diye tam kopya
//!   geri/                         undo edilen anlık görüntüler (redo yığını)
//! ```
//!
//! **Neden her anlık görüntünün tam kopyası da tutulur?** Fark tabanlı depo tek
//! yönlüdür: `T5`'i okumak için `T1..T4` okunmalıdır. Arşivleme (saklama politikası)
//! bir ara anlık görüntüyü sildiğinde zincir kırılır. Bu yüzden her adımda
//! önceki hâlin tam kopyası yazılır; `delta` dosyası okuma hızını, `tam` kopyası
//! ise zincirin sürekliliğini garanti eder. Maliyet, "zaman-mekân takası"nın bilinçli
//! bedelidir ve README'de ölçülen sayılarla verilmiştir.
//!
//! **Veri güvenliği:** bu modül yalnızca `<depo>` dizini altında yazma yapar.
//! Tarama köküne hiçbir komutta dokunulmaz; `tests/veri_guvenligi.rs` bunu
//! kanıtlar.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::akis::{AkisYazici, SatirOkuyucu};
use crate::delta;
use crate::hata::{Hata, Sonuc};
use crate::kayit::{AnlikGoruntuBasi, BasitTur, Degisim, KayitTuru, Saglama, TamKayit, SEMA};
use crate::tarama::{TaramaAyari, TaramaOzeti};
use crate::yol::Yol;

/// Depo indeks dosyasının adı.
pub const INDEKS_ADI: &str = "index.json";

/// Anlık görüntü dosyalarının bulunduğu alt dizin.
pub const ANLIK_DIZIN: &str = "anlik_goruntuler";

/// Undo edilen anlık görüntülerin tutulduğu alt dizin (redo yığını).
pub const GERI_DIZIN: &str = "geri";

/// Depo indeks dosyasının şeması.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepoIndeksi {
    /// Şema ayracı.
    pub sema: String,
    /// Biçim sürümü.
    pub surum: u32,
    /// Aracın sürüm dizesi (hata ayıklama için; okuma mantığına girmez).
    pub uretici: String,
    /// İzlenen kök dizinin **o anki** mutlak yolu.
    ///
    /// Depo başka makineye taşındığında bu yol geçersiz olur; bu durum
    /// `uyumsuz` olarak işaretlenir ve yalnızca istatistik sorgusu yapılabilir.
    pub kok: String,
    /// Anlık görüntü kimliklerinin oluş sırasına göre listesi.
    pub anlik_goruntuler: Vec<String>,
    /// Bir sonraki anlık görüntünün sıra numarası (monotonik artar).
    ///
    /// Kimlik `T{sıra}` biçimindedir ve **sayıdan** değil bu sayaçtan üretilir.
    /// Saklama politikası eski anlık görüntüleri kaldırdığında liste kısalır;
    /// sayaç artmadığı için kimlikler yeniden kullanılmaz (aksi hâlde yeni
    /// tarama, silinmiş bir anlık görüntünün üzerine yazardı).
    pub sayac: u64,
}

impl DepoIndeksi {
    /// Yeni bir depo indeksi üretir.
    pub fn yeni(kok: &Path) -> Self {
        DepoIndeksi {
            sema: SEMA.to_owned(),
            surum: crate::BICIM_SURUMU,
            uretici: format!("timefold {}", crate::SURUM),
            kok: kok.to_string_lossy().into_owned(),
            anlik_goruntuler: Vec::new(),
            sayac: 0,
        }
    }

    /// Verilen kimliğin sıra numarasını çözer (`T0007` → `7`).
    pub fn sira_no(kimlik: &str) -> Option<u64> {
        kimlik.strip_prefix('T')?.parse::<u64>().ok()
    }
}

/// Depo indeksinin okunmuş ve doğrulanmış hâli.
#[derive(Debug, Clone)]
pub struct Depo {
    dizin: PathBuf,
    indeks: DepoIndeksi,
}

impl Depo {
    /// Verilen dizindeki depoyu açar (yoksa hata verir).
    pub fn ac(dizin: &Path) -> Sonuc<Self> {
        let indeks_yolu = dizin.join(INDEKS_ADI);
        if !indeks_yolu.is_file() {
            return Err(Hata::DepoBosOrBozuk {
                dizin: dizin.to_path_buf(),
                ayrinti: format!("{} bulunamadı", INDEKS_ADI),
            });
        }
        let metin = std::fs::read_to_string(&indeks_yolu)
            .map_err(|kaynak| Hata::io("indeks oku", &indeks_yolu, kaynak))?;
        let indeks: DepoIndeksi =
            serde_json::from_str(&metin).map_err(|kaynak| Hata::DepoBosOrBozuk {
                dizin: dizin.to_path_buf(),
                ayrinti: format!("indeks çözümlenemedi: {}", kaynak),
            })?;
        if indeks.sema != SEMA {
            return Err(Hata::DepoBosOrBozuk {
                dizin: dizin.to_path_buf(),
                ayrinti: format!("beklenmeyen şema: {}", indeks.sema),
            });
        }
        if indeks.surum != crate::BICIM_SURUMU {
            return Err(Hata::SurumUyusmazligi {
                dosya: indeks_yolu,
                bulunan: indeks.surum,
                beklenen: crate::BICIM_SURUMU,
            });
        }
        let mut depo = Depo {
            dizin: dizin.to_path_buf(),
            indeks,
        };
        // Sayaç, listeden türetilir: elle düzenlenmiş veya eski bir indeks
        // dosyasında liste, sayaca göre daha ilerideyse kimlik çakışması
        // yaşanmasın diye en büyük sıra esas alınır.
        let listeden = depo
            .indeks
            .anlik_goruntuler
            .iter()
            .filter_map(|k| DepoIndeksi::sira_no(k))
            .max();
        if let Some(en_buyuk) = listeden {
            if en_buyuk > depo.indeks.sayac {
                depo.indeks.sayac = en_buyuk;
            }
        }
        Ok(depo)
    }

    /// Verilen dizinde yoksa boş bir depo oluşturur.
    ///
    /// `kok`, indeksin tutacağı taranan kök yoludur; henüz doğrulanmaz.
    pub fn olustur_veya_ac(dizin: &Path, kok: &Path) -> Sonuc<Self> {
        if dizin.join(INDEKS_ADI).is_file() {
            return Depo::ac(dizin);
        }
        std::fs::create_dir_all(dizin.join(ANLIK_DIZIN))
            .map_err(|kaynak| Hata::io("depo dizini oluştur", dizin, kaynak))?;
        std::fs::create_dir_all(dizin.join(GERI_DIZIN))
            .map_err(|kaynak| Hata::io("geri dizini oluştur", dizin, kaynak))?;
        let indeks = DepoIndeksi::yeni(kok);
        let mut depo = Depo {
            dizin: dizin.to_path_buf(),
            indeks,
        };
        depo.indeksi_yaz()?;
        Ok(depo)
    }

    /// Depo dizinini döndürür.
    pub fn dizin(&self) -> &Path {
        &self.dizin
    }

    /// İndeksi döndürür.
    pub fn indeks(&self) -> &DepoIndeksi {
        &self.indeks
    }

    /// Anlık görüntü kimliklerini oluş sırasına göre döndürür.
    pub fn kimlikler(&self) -> &[String] {
        &self.indeks.anlik_goruntuler
    }

    /// Depodaki anlık görüntü sayısını döndürür.
    pub fn adet(&self) -> usize {
        self.indeks.anlik_goruntuler.len()
    }

    /// Depo boş mu?
    pub fn bos_mu(&self) -> bool {
        self.indeks.anlik_goruntuler.is_empty()
    }

    /// Sıradaki anlık görüntü kimliğini üretir (`T0001`, `T0002`, ...).
    ///
    /// Kimlik monotonik sayaca dayanır; saklama politikası geçmişi kısaltsa
    /// bile numara yeniden kullanılmaz.
    pub fn sonraki_kimlik(&self) -> String {
        format!("T{:04}", self.indeks.sayac + 1)
    }

    /// En yeni anlık görüntünün kimliğini döndürür; depo boşsa `None`.
    pub fn en_yeni(&self) -> Option<&str> {
        self.indeks.anlik_goruntuler.last().map(String::as_str)
    }

    /// Bir anlık görüntünün tam kayıt dosyasının yolunu döndürür.
    pub fn tam_yol(&self, kimlik: &str) -> PathBuf {
        self.dizin
            .join(ANLIK_DIZIN)
            .join(format!("{}.tam.jsonl", kimlik))
    }

    /// Bir anlık görüntünün fark dosyasının yolunu döndürür.
    pub fn fark_yol(&self, kimlik: &str) -> PathBuf {
        delta::fark_dosyasi_yolu(&self.dizin, kimlik)
    }

    /// Kimliğin depoda var olduğunu doğrular.
    pub fn kimlik_var_mi(&self, kimlik: &str) -> bool {
        self.indeks.anlik_goruntuler.iter().any(|k| k == kimlik)
    }

    /// İndeksi diske yazar.
    ///
    /// Yazma **geçici dosyaya yapılıp taşınır**: güç kesintisi indeksi yarım
    /// bırakmaz ve bir sonraki açılışta depo okunabilir kalır (raporun R8 riski).
    fn indeksi_yaz(&mut self) -> Sonuc<()> {
        let metin =
            serde_json::to_string_pretty(&self.indeks).map_err(|k| Hata::DepoBosOrBozuk {
                dizin: self.dizin.clone(),
                ayrinti: format!("indeks serileştirilemedi: {}", k),
            })?;
        let gecici = self.dizin.join(format!("{}.tmp", INDEKS_ADI));
        let hedef = self.dizin.join(INDEKS_ADI);
        std::fs::write(&gecici, metin.as_bytes())
            .map_err(|kaynak| Hata::io("indeks yaz", &gecici, kaynak))?;
        std::fs::rename(&gecici, &hedef)
            .map_err(|kaynak| Hata::io("indeks taşı", &hedef, kaynak))?;
        Ok(())
    }

    /// **Depo oluşturmadan** kuru tarama yapar.
    ///
    /// `--kuru-sur` bayrağının vaadi "hiçbir şey yazmamak"tır; bu yüzden depo
    /// dizini yokken `olustur_veya_ac` çağrılmaz, hiçbir dizin veya dosya
    /// oluşturulmaz. Tarama yine de gerçekten yürütülür ve fark, depoda varsa
    /// mevcut son anlık görüntüye göre hesaplanır.
    pub fn kuru_tarama(
        ayar: &TaramaAyari,
        depo_dizini: &Path,
        gecici_dizin: &Path,
    ) -> Sonuc<TaramaSonucu> {
        let yeni_tam = gecici_dizin.join("kuru.tam.jsonl");
        let gecici_fark = gecici_dizin.join("kuru.fark.jsonl");
        let (_yol, ozet) = crate::tarama::tara_akisa(ayar, gecici_dizin, &yeni_tam)?;

        // Depo yoksa ilk tarama (fark = hepsi eklendi); varsa son anlık görüntüye
        // göre fark hesaplanır.
        let temel: Option<delta::Kaynak> = if depo_dizini.join(INDEKS_ADI).is_file() {
            let depo = Depo::ac(depo_dizini)?;
            depo.en_yeni()
                .map(str::to_owned)
                .filter(|k| depo.tam_yol(k).is_file())
                .map(|k| delta::Kaynak::baslikli(depo.tam_yol(&k)))
        } else {
            None
        };
        let fark_ozeti = delta::karsilastir(
            temel.as_ref(),
            &delta::Kaynak::basliksiz(&yeni_tam),
            &gecici_fark,
        )?;
        let _ = std::fs::remove_file(&yeni_tam);
        let _ = std::fs::remove_file(&gecici_fark);
        Ok(TaramaSonucu {
            kimlik: "T0000".to_owned(),
            uygulandi: false,
            ozet,
            fark: fark_ozeti,
        })
    }

    /// Bir tarama yapar ve sonucu depoya yeni bir anlık görüntü olarak ekler.
    ///
    /// Akış: tarama → sıralı tam dosya → önceki tam dosyayla fark → iki kopyayı da
    /// yaz → indeksi güncelle. `--dry-run` karşılığı `kuru_sur` bayrağıdır;
    /// o durumda hiçbir dosya yazılmaz ve sonuç yalnızca raporlanır.
    pub fn tarama_ekle(
        &mut self,
        ayar: &TaramaAyari,
        gecici_dizin: &Path,
        kuru_sur: bool,
    ) -> Sonuc<TaramaSonucu> {
        let kimlik = self.sonraki_kimlik();
        let onceki = self.en_yeni().map(str::to_owned);
        let yeni_tam = self
            .dizin
            .join("gecici")
            .join(format!("{}.tam.jsonl", kimlik));
        let gecici = gecici_dizin.to_path_buf();

        let (_yol, ozet) = crate::tarama::tara_akisa(ayar, &gecici, &yeni_tam)?;

        let eski_tam = onceki
            .as_ref()
            .map(|k| delta::Kaynak::baslikli(self.tam_yol(k)));
        let gecici_fark = self
            .dizin
            .join("gecici")
            .join(format!("{}.fark.jsonl", kimlik));
        let fark_ozeti = delta::karsilastir(
            eski_tam.as_ref(),
            &delta::Kaynak::basliksiz(&yeni_tam),
            &gecici_fark,
        )?;

        if kuru_sur {
            let _ = std::fs::remove_file(&yeni_tam);
            let _ = std::fs::remove_file(&gecici_fark);
            return Ok(TaramaSonucu {
                kimlik,
                uygulandi: false,
                ozet,
                fark: fark_ozeti,
            });
        }

        let baslik = AnlikGoruntuBasi {
            sema: SEMA.to_owned(),
            surum: crate::BICIM_SURUMU,
            kimlik: kimlik.clone(),
            temel: onceki.clone(),
            zaman: ayar.zaman,
            kok: ayar.kok.to_string_lossy().into_owned(),
            kayit_sayisi: ozet.yazilan,
            saglama: saglama_hesapla(&yeni_tam)?,
            tur: if onceki.is_some() {
                BasitTur::Fark
            } else {
                BasitTur::Tam
            },
            kismi: ozet.kismi_mi(),
            kismi_gerekce: ozet.gerekce_metni(),
            orneklendi: ozet.orneklendi,
            taranan: ozet.taranan,
        };

        let tam_hedef = self.tam_yol(&kimlik);
        let fark_hedef = self.fark_yol(&kimlik);
        std::fs::create_dir_all(self.dizin.join(ANLIK_DIZIN))
            .map_err(|kaynak| Hata::io("anlık görüntü dizini", &self.dizin, kaynak))?;
        std::fs::create_dir_all(self.dizin.join("gecici"))
            .map_err(|kaynak| Hata::io("geçici dizin oluştur", &self.dizin, kaynak))?;
        std::fs::rename(&yeni_tam, &tam_hedef)
            .map_err(|kaynak| Hata::io("anlık görüntü taşı", &yeni_tam, kaynak))?;
        std::fs::rename(&gecici_fark, &fark_hedef)
            .map_err(|kaynak| Hata::io("fark dosyası taşı", &gecici_fark, kaynak))?;

        let baslik_satiri = serde_json::to_string(&baslik).map_err(|k| Hata::BozukSatir {
            dosya: tam_hedef.clone(),
            satir: 0,
            ayrinti: k.to_string(),
        })?;
        basligi_yaz(&tam_hedef, &baslik_satiri)?;
        basligi_yaz(&fark_hedef, &baslik_satiri)?;

        self.indeks.anlik_goruntuler.push(kimlik.clone());
        self.indeks.sayac += 1;
        self.indeks.kok = ayar.kok.to_string_lossy().into_owned();
        self.indeksi_yaz()?;
        let _ = std::fs::remove_dir_all(&gecici);

        Ok(TaramaSonucu {
            kimlik,
            uygulandi: true,
            ozet,
            fark: fark_ozeti,
        })
    }

    /// Bir anlık görüntünün başlığını okur.
    ///
    /// Sürüm uyuşmazlığında [`Hata::SurumUyusmazligi`] döner; dosya **okunmaz ve
    /// değiştirilmez**, böylece eski depo yanlışlıkla bozulmaz.
    pub fn basligi_oku(&self, kimlik: &str) -> Sonuc<AnlikGoruntuBasi> {
        let yol = self.tam_yol(kimlik);
        let mut okuyucu = SatirOkuyucu::ac(&yol)?;
        let satir = match okuyucu.sonraki()? {
            None => {
                return Err(Hata::DepoBosOrBozuk {
                    dizin: self.dizin.clone(),
                    ayrinti: format!("{} boş", kimlik),
                })
            }
            Some(satir) => satir,
        };
        let baslik: AnlikGoruntuBasi =
            serde_json::from_str(&satir).map_err(|k| Hata::BozukSatir {
                dosya: yol.clone(),
                satir: 1,
                ayrinti: format!("başlık çözümlenemedi: {}", k),
            })?;
        if baslik.surum != crate::BICIM_SURUMU {
            return Err(Hata::SurumUyusmazligi {
                dosya: yol,
                bulunan: baslik.surum,
                beklenen: crate::BICIM_SURUMU,
            });
        }
        Ok(baslik)
    }

    /// Bir anlık görüntünün tam kayıtlarını döndürür (başlık hariç).
    pub fn tam_kayitlari(&self, kimlik: &str) -> Sonuc<Vec<TamKayit>> {
        self.kimligi_dogrula(kimlik)?;
        delta::tam_kayitlari_oku(&self.tam_yol(kimlik), true)
    }

    /// Bir anlık görüntünün fark kayıtlarını döndürür.
    ///
    /// Bozuk satırlar atlanır; sayı [`bozuk_fark_satiri`] ile ayrıca bildirilir.
    pub fn fark_kayitlari(&self, kimlik: &str) -> Sonuc<Vec<Degisim>> {
        Ok(self.fark_kayitlari_ayrinti(kimlik)?.0)
    }

    /// Bir anlık görüntünün fark kayıtlarını ve atlanan bozuk satır sayısını
    /// birlikte döndürür.
    pub fn fark_kayitlari_ayrinti(&self, kimlik: &str) -> Sonuc<(Vec<Degisim>, u64)> {
        self.kimligi_dogrula(kimlik)?;
        let yol = self.fark_yol(kimlik);
        if !yol.is_file() {
            return Ok((Vec::new(), 0));
        }
        delta::farklari_oku(&yol, true)
    }

    /// Bir anlık görüntünün fark dosyasındaki bozuk satır sayısını döndürür.
    pub fn bozuk_fark_satiri(&self, kimlik: &str) -> u64 {
        match self.fark_kayitlari_ayrinti(kimlik) {
            Ok((_, atlanan)) => atlanan,
            Err(_) => 0,
        }
    }

    /// İndekteki her anlık görüntünün durumunu döndürür.
    ///
    /// Eksik veya bozuk dosyalar **gizlenmez**; [`BilgiDurumu::Bozuk`] olarak
    /// raporlanır. Bu, raporun "bozuk kayıt atlanır, boşluk işaretlenir" kuralının
    /// uygulanmasıdır: çağıran taraf boşluğu görür ve araya değer uydurmaz.
    pub fn bilgiler(&self) -> Vec<BilgiDurumu> {
        self.indeks
            .anlik_goruntuler
            .iter()
            .map(|kimlik| match self.basligi_oku(kimlik) {
                Ok(baslik) => BilgiDurumu::Ok(AnlikGoruntuBilgisi {
                    kimlik: kimlik.clone(),
                    baslik,
                }),
                Err(hata) => BilgiDurumu::Bozuk {
                    kimlik: kimlik.clone(),
                    ayrinti: hata.to_string(),
                },
            })
            .collect()
    }

    /// Depoda **okunabilir** anlık görüntü bilgilerini döndürür.
    ///
    /// Eksik dosyalar listede bulunmaz; boşlukları görmek isteyen çağıran taraf
    /// [`bilgiler`](Self::bilgiler) kullanmalıdır.
    pub fn var_olan_bilgiler(&self) -> Vec<AnlikGoruntuBilgisi> {
        self.bilgiler()
            .into_iter()
            .filter_map(|durum| match durum {
                BilgiDurumu::Ok(bilgi) => Some(bilgi),
                BilgiDurumu::Bozuk { .. } => None,
            })
            .collect()
    }

    /// Depo istatistiğini döndürür (bayt cinsinden).
    pub fn ozet(&self) -> Sonuc<DepoOzeti> {
        let mut toplam = 0u64;
        let mut dosya = 0u64;
        for kimlik in &self.indeks.anlik_goruntuler {
            for yol in [self.tam_yol(kimlik), self.fark_yol(kimlik)] {
                if yol.is_file() {
                    dosya += 1;
                    toplam += crate::akis::dosya_boyutu(&yol);
                }
            }
        }
        let geri = self.dizin.join(GERI_DIZIN);
        let geri_adet = crate::akis::dizin_icerigi_say(&geri);
        Ok(DepoOzeti {
            anlik_goruntu_sayisi: self.adet(),
            dosya_sayisi: dosya,
            toplam_bayt: toplam,
            undo_edilebilir: self.adet() > 0,
            geri_yigininda: geri_adet,
        })
    }

    /// Yeni bir tarama yapmadan önce son anlık görüntüyü geri alır (undo).
    ///
    /// Bu işlem **yalnızca deponun kendi dosyalarını** taşır: anlık görüntü
    /// `geri/` dizinine taşınır ve indeks güncellenir. Kullanıcı dosyalarına
    /// dokunulmaz, çünkü tarama köküne hiçbir zaman yazılmaz.
    pub fn undo(&mut self, kuru_sur: bool) -> Sonuc<UndoSonucu> {
        let Some(kimlik) = self.en_yeni().map(str::to_owned) else {
            return Err(Hata::gecersiz_durum("undo edilecek anlık görüntü yok"));
        };
        if self.adet() < 2 {
            return Err(Hata::gecersiz_durum(
                "ilk anlık görüntü geri alınamaz; zincirin başıdır",
            ));
        }
        let tam = self.tam_yol(&kimlik);
        let fark = self.fark_yol(&kimlik);
        let boyut = crate::akis::dosya_boyutu(&tam) + crate::akis::dosya_boyutu(&fark);
        if kuru_sur {
            return Ok(UndoSonucu {
                kimlik,
                uygulandi: false,
                serbest_bayt: boyut,
            });
        }
        let geri = self.dizin.join(GERI_DIZIN);
        std::fs::create_dir_all(&geri).map_err(|kaynak| Hata::io("geri dizini", &geri, kaynak))?;
        for yol in [tam, fark] {
            if yol.is_file() {
                let hedef = geri.join(
                    yol.file_name()
                        .map(|a| a.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "bilinmeyen".to_owned()),
                );
                std::fs::rename(&yol, &hedef)
                    .map_err(|kaynak| Hata::io("geri taşı", &yol, kaynak))?;
            }
        }
        self.indeks.anlik_goruntuler.pop();
        self.indeksi_yaz()?;
        Ok(UndoSonucu {
            kimlik,
            uygulandi: true,
            serbest_bayt: boyut,
        })
    }

    /// Son geri alınan anlık görüntüyü yeniden uygular (redo).
    pub fn redo(&mut self, kuru_sur: bool) -> Sonuc<UndoSonucu> {
        let geri = self.dizin.join(GERI_DIZIN);
        let Ok(girdiler) = std::fs::read_dir(&geri) else {
            return Err(Hata::gecersiz_durum("geri yığını yok"));
        };
        let mut adaylar: Vec<PathBuf> = girdiler
            .flatten()
            .map(|g| g.path())
            .filter(|p| p.is_file())
            .collect();
        adaylar.sort();
        let tam_aday = adaylar
            .iter()
            .find(|p| p.to_string_lossy().ends_with(".tam.jsonl"));
        let Some(tam_aday) = tam_aday.cloned() else {
            return Err(Hata::gecersiz_durum(
                "geri yığınında geri alınmış anlık görüntü yok",
            ));
        };
        let kimlik = tam_aday
            .file_name()
            .and_then(|a| a.to_str())
            .and_then(|a| a.split('.').next())
            .map(str::to_owned)
            .ok_or_else(|| Hata::gecersiz_durum("anlık görüntü adı çözümlenemedi"))?;
        let boyut = adaylar
            .iter()
            .map(|p| crate::akis::dosya_boyutu(p))
            .sum::<u64>();
        if kuru_sur {
            return Ok(UndoSonucu {
                kimlik,
                uygulandi: false,
                serbest_bayt: boyut,
            });
        }
        std::fs::create_dir_all(self.dizin.join(ANLIK_DIZIN))
            .map_err(|kaynak| Hata::io("anlık görüntü dizini", &self.dizin, kaynak))?;
        for yol in adaylar {
            let ad = yol
                .file_name()
                .map(|a| a.to_string_lossy().into_owned())
                .unwrap_or_else(|| "bilinmeyen".to_owned());
            let hedef = self.dizin.join(ANLIK_DIZIN).join(&ad);
            std::fs::rename(&yol, &hedef)
                .map_err(|kaynak| Hata::io("geri uygula", &yol, kaynak))?;
        }
        self.indeks.anlik_goruntuler.push(kimlik.clone());
        self.indeks.sayac += 1;
        self.indeksi_yaz()?;
        Ok(UndoSonucu {
            kimlik,
            uygulandi: true,
            serbest_bayt: boyut,
        })
    }

    /// Belirtilen anlık görüntüye geri döner; sonrasındaki tüm anlık görüntüler
    /// depodan kaldırılır.
    ///
    /// Bu, "anlık görüntüyü eski hâline döndürme"nin birincil yoludur. Silinen
    /// tek şey aracın **kendi arşividir**; kullanıcı dosyaları hiç etkilenmez.
    pub fn geri_al_hedefe(&mut self, hedef: &str, kuru_sur: bool) -> Sonuc<RestoreSonucu> {
        self.kimligi_dogrula(hedef)?;
        let sira = self
            .indeks
            .anlik_goruntuler
            .iter()
            .position(|k| k == hedef)
            .ok_or_else(|| Hata::AnlikGoruntuYok {
                istenen: hedef.to_owned(),
                en_yeni: self.en_yeni().map(str::to_owned),
            })?;
        let silinecekler: Vec<String> = self.indeks.anlik_goruntuler[sira + 1..].to_vec();
        let mut serbest = 0u64;
        for kimlik in &silinecekler {
            serbest += crate::akis::dosya_boyutu(&self.tam_yol(kimlik));
            serbest += crate::akis::dosya_boyutu(&self.fark_yol(kimlik));
        }
        if kuru_sur {
            return Ok(RestoreSonucu {
                hedef: hedef.to_owned(),
                silinen: silinecekler,
                serbest_bayt: serbest,
                uygulandi: false,
            });
        }
        for kimlik in &silinecekler {
            for yol in [self.tam_yol(kimlik), self.fark_yol(kimlik)] {
                if yol.is_file() {
                    std::fs::remove_file(&yol)
                        .map_err(|kaynak| Hata::io("arşivden kaldır", &yol, kaynak))?;
                }
            }
        }
        self.indeks.anlik_goruntuler.truncate(sira + 1);
        self.indeksi_yaz()?;
        Ok(RestoreSonucu {
            hedef: hedef.to_owned(),
            silinen: silinecekler,
            serbest_bayt: serbest,
            uygulandi: true,
        })
    }

    /// Saklama politikasının bu depoda silinecek saydığı kimlikleri döndürür.
    ///
    /// Politika zaman damgası ister; başlığı okunamayan anlık görüntü zamanı 0
    /// kabul edilir, yani "en eski" sayılır ve günlük korumadan düşer. Bu
    /// bilinçli bir tercihtir: okunamayan kayıt zaten güvenilmezdir.
    pub fn saklama_hedefleri(&self, politika: &crate::saklama::SaklamaPolitikasi) -> Vec<String> {
        if !politika.etkin_mi() {
            return Vec::new();
        }
        let zamanlar: Vec<(String, i64)> = self
            .indeks
            .anlik_goruntuler
            .iter()
            .map(|kimlik| {
                let zaman = self.basligi_oku(kimlik).map(|b| b.zaman).unwrap_or(0);
                (kimlik.clone(), zaman)
            })
            .collect();
        politika.silinecekler(&zamanlar)
    }

    /// Saklama politikasına göre eski anlık görüntüleri kaldırır.
    ///
    /// Politika yalnızca **deponun kendi dosyalarına** uygulanır; silinen bayt
    /// miktarı döner, böylece kullanıcı "ne kadar geçmiş gitti" sorusunun
    /// cevabını alır (raporun R5 azaltması).
    pub fn saklama_uygula(
        &mut self,
        politika: &crate::saklama::SaklamaPolitikasi,
        kuru_sur: bool,
    ) -> Sonuc<SaklamaSonucu> {
        let silinecek = self.saklama_hedefleri(politika);
        if silinecek.is_empty() {
            return Ok(SaklamaSonucu {
                silinen: Vec::new(),
                serbest_bayt: 0,
                uygulandi: false,
            });
        }
        let mut serbest = 0u64;
        for kimlik in &silinecek {
            serbest += crate::akis::dosya_boyutu(&self.tam_yol(kimlik));
            serbest += crate::akis::dosya_boyutu(&self.fark_yol(kimlik));
        }
        if kuru_sur {
            return Ok(SaklamaSonucu {
                silinen: silinecek,
                serbest_bayt: serbest,
                uygulandi: false,
            });
        }
        for kimlik in &silinecek {
            for yol in [self.tam_yol(kimlik), self.fark_yol(kimlik)] {
                if yol.is_file() {
                    std::fs::remove_file(&yol)
                        .map_err(|kaynak| Hata::io("arşivden kaldır", &yol, kaynak))?;
                }
            }
        }
        self.indeks
            .anlik_goruntuler
            .retain(|k| !silinecek.contains(k));
        self.indeksi_yaz()?;
        Ok(SaklamaSonucu {
            silinen: silinecek,
            serbest_bayt: serbest,
            uygulandi: true,
        })
    }

    fn kimligi_dogrula(&self, kimlik: &str) -> Sonuc<()> {
        if self.kimlik_var_mi(kimlik) {
            Ok(())
        } else {
            Err(Hata::AnlikGoruntuYok {
                istenen: kimlik.to_owned(),
                en_yeni: self.en_yeni().map(str::to_owned),
            })
        }
    }
}

fn basligi_yaz(dosya: &Path, baslik_satiri: &str) -> Sonuc<()> {
    let icerik =
        std::fs::read(dosya).map_err(|kaynak| Hata::io("anlık görüntü oku", dosya, kaynak))?;
    let gecici = dosya.with_extension("tmp");
    let mut yazici = AkisYazici::olustur(&gecici)?;
    yazici.yaz(baslik_satiri)?;
    let metin = String::from_utf8_lossy(&icerik);
    for satir in metin.lines() {
        yazici.yaz(satir)?;
    }
    yazici.bitir()?;
    std::fs::rename(&gecici, dosya)
        .map_err(|kaynak| Hata::io("anlık görüntü yaz", dosya, kaynak))?;
    Ok(())
}

fn saglama_hesapla(dosya: &Path) -> Sonuc<u64> {
    let mut okuyucu = SatirOkuyucu::ac(dosya)?;
    let mut saglama = Saglama::yeni();
    while let Some(satir) = okuyucu.sonraki()? {
        if satir.trim().is_empty() {
            continue;
        }
        if let Ok(kayit) = serde_json::from_str::<TamKayit>(&satir) {
            saglama.kayit_ekle(&kayit);
        }
    }
    Ok(saglama.deger())
}

/// Bir taramayı depoya eklemenin sonucu.
#[derive(Debug, Clone)]
pub struct TaramaSonucu {
    /// Oluşturulan anlık görüntü kimliği.
    pub kimlik: String,
    /// Yazma gerçekleşti mi (kuru çalıştırmada `false`).
    pub uygulandi: bool,
    /// Tarama özeti.
    pub ozet: TaramaOzeti,
    /// Önceki anlık görüntüye göre fark özeti.
    pub fark: delta::FarkOzeti,
}

/// Bir anlık görüntünün başlığıyla birlikte taşınan bilgisi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnlikGoruntuBilgisi {
    /// Anlık görüntü kimliği.
    pub kimlik: String,
    /// Dosyanın başlığı.
    pub baslik: AnlikGoruntuBasi,
}

/// İndekteki bir anlık görüntünün okunabilirlik durumu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BilgiDurumu {
    /// Dosya okundu.
    Ok(AnlikGoruntuBilgisi),
    /// Dosya eksik ya da bozuk; zincirde **boşluk** oluşur.
    Bozuk {
        /// Anlık görüntü kimliği.
        kimlik: String,
        /// Hata açıklaması.
        ayrinti: String,
    },
}

impl AnlikGoruntuBilgisi {
    /// `YYYY-MM-DDTHH:MM:SSZ` biçiminde zaman metni döndürür.
    pub fn zaman_metni(&self) -> String {
        crate::zaman::unix_saniye_rfc3339(self.baslik.zaman)
    }
}

/// Depo istatistiği.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DepoOzeti {
    /// Anlık görüntü sayısı.
    pub anlik_goruntu_sayisi: usize,
    /// Depodaki dosya sayısı.
    pub dosya_sayisi: u64,
    /// Depo dosyalarının toplam baytı.
    pub toplam_bayt: u64,
    /// Undo edilebilir mi (en az iki anlık görüntü gerekir).
    pub undo_edilebilir: bool,
    /// Geri yığınındaki dosya sayısı.
    pub geri_yigininda: u64,
}

/// Undo/redo sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoSonucu {
    /// Etkilenen anlık görüntü kimliği.
    pub kimlik: String,
    /// İşlem gerçekleşti mi.
    pub uygulandi: bool,
    /// Arşivde serbest kalan bayt.
    pub serbest_bayt: u64,
}

/// Restore sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreSonucu {
    /// Gidilen anlık görüntü.
    pub hedef: String,
    /// Kaldırılan anlık görüntü kimlikleri (zaman sırasına göre).
    pub silinen: Vec<String>,
    /// Arşivde serbest kalan bayt.
    pub serbest_bayt: u64,
    /// İşlem gerçekleşti mi.
    pub uygulandi: bool,
}

/// Saklama sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaklamaSonucu {
    /// Kaldırılan anlık görüntü kimlikleri.
    pub silinen: Vec<String>,
    /// Arşivde serbest kalan bayt.
    pub serbest_bayt: u64,
    /// İşlem gerçekleşti mi.
    pub uygulandi: bool,
}

/// Yükleme sırasında okunan yol → kayıt eşlemesi (küçük ağaçlar için).
pub fn yollari_kayitlara_esle(kayitlar: &[TamKayit]) -> BTreeMap<Yol, TamKayit> {
    let mut harita = BTreeMap::new();
    for kayit in kayitlar {
        harita.insert(kayit.yol.clone(), kayit.clone());
    }
    harita
}

/// Bir kayıt kümesindeki en büyük klasörü döndürür (`tur == Klasor` olan ilk kayıt).
pub fn ilk_klasor(kayitlar: &[TamKayit]) -> Option<&TamKayit> {
    kayitlar.iter().find(|k| k.tur == KayitTuru::Klasor)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
// expect/unwrap test kodunda bilinçlidir: WORKER_CONTRACT.md § 4.2 bunları
// yalnızca testlerde serbest bırakır. Bir testin expect çağrısının
// başarısız olması, o iddianın yanlış olduğunun en net sinyalidir.
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn gecici(etiket: &str) -> PathBuf {
        let yol =
            std::env::temp_dir().join(format!("timefold-depo-{}-{}", etiket, std::process::id()));
        let _ = fs::remove_dir_all(&yol);
        fs::create_dir_all(&yol).expect("geçici dizin");
        yol
    }

    fn ornek_agac(dizin: &Path) {
        fs::create_dir_all(dizin.join("a")).expect("dizin");
        fs::write(dizin.join("a/x.txt"), "12345").expect("yaz");
        fs::write(dizin.join("b.txt"), "123").expect("yaz");
    }

    fn depo_kur(kok: &Path, depo: &Path) -> Depo {
        Depo::olustur_veya_ac(depo, kok).expect("depo")
    }

    #[test]
    fn bos_depo_olusturulur_ve_acilir() {
        let dizin = gecici("bos");
        let kok = dizin.join("kok");
        fs::create_dir_all(&kok).expect("kok");
        let depo_yolu = dizin.join("depo");
        let depo = depo_kur(&kok, &depo_yolu);
        assert!(depo.bos_mu());
        assert_eq!(depo.sonraki_kimlik(), "T0001");
        let yeniden = Depo::ac(&depo_yolu).expect("yeniden aç");
        assert!(yeniden.bos_mu());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn ilk_tarama_tam_kayit_uretir() {
        let dizin = gecici("ilk");
        let kok = dizin.join("kok");
        ornek_agac(&kok);
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let mut ayar = TaramaAyari::yeni(&kok);
        ayar.zaman = 1_000;
        let sonuc = depo
            .tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("tarama");
        assert_eq!(sonuc.kimlik, "T0001");
        assert!(sonuc.uygulandi);
        assert_eq!(sonuc.fark.eklendi, 4);
        let kayitlar = depo.tam_kayitlari("T0001").expect("kayıtlar");
        assert_eq!(kayitlar.len(), 4);
        let baslik = depo.basligi_oku("T0001").expect("başlık");
        assert_eq!(baslik.tur, BasitTur::Tam);
        assert_eq!(baslik.temel, None);
        assert!(!baslik.kismi);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn ikinci_tarama_fark_uretir() {
        let dizin = gecici("ikinci");
        let kok = dizin.join("kok");
        ornek_agac(&kok);
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let mut ayar = TaramaAyari::yeni(&kok);
        ayar.zaman = 1_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("1");
        fs::write(kok.join("a/x.txt"), "1234567890").expect("değiştir");
        ayar.zaman = 2_000;
        let sonuc = depo
            .tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("2");
        assert_eq!(sonuc.kimlik, "T0002");
        // "" , "a" ve "a/x.txt" boyut değiştirdi; "a" klasörü de değişti.
        assert_eq!(sonuc.fark.degisti, 3);
        let baslik = depo.basligi_oku("T0002").expect("başlık");
        assert_eq!(baslik.tur, BasitTur::Fark);
        assert_eq!(baslik.temel.as_deref(), Some("T0001"));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn kuru_surma_depoya_yazmaz() {
        let dizin = gecici("kuru");
        let kok = dizin.join("kok");
        ornek_agac(&kok);
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let ayar = TaramaAyari::yeni(&kok);
        let sonuc = depo
            .tarama_ekle(&ayar, &dizin.join("gecici"), true)
            .expect("kuru tarama");
        assert!(!sonuc.uygulandi);
        assert!(depo.bos_mu());
        assert!(!depo.tam_yol("T0001").exists());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn surum_uyusmazligi_hata_donduruyor() {
        let dizin = gecici("surum");
        let kok = dizin.join("kok");
        fs::create_dir_all(&kok).expect("kok");
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let ayar = TaramaAyari::yeni(&kok);
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("tarama");
        // Başlıktaki sürümü elle boz.
        let tam = depo.tam_yol("T0001");
        let metin = fs::read_to_string(&tam).expect("okuma");
        let bozuk = metin.replace("\"surum\":1", "\"surum\":99");
        fs::write(&tam, &bozuk).expect("yazma");
        let sonuc = depo.basligi_oku("T0001");
        assert!(matches!(
            sonuc,
            Err(Hata::SurumUyusmazligi {
                bulunan: 99,
                beklenen: 1,
                ..
            })
        ));
        // Dosya okunmamış olmalı: içerik olduğu gibi duruyor.
        assert_eq!(fs::read_to_string(&tam).expect("okuma"), bozuk);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn surum_uyusmazligi_ile_olmayan_depo_dosyalari_corrupt_olmaz() {
        let dizin = gecici("indekssurum");
        let kok = dizin.join("kok");
        fs::create_dir_all(&kok).expect("kok");
        let depo_yolu = dizin.join("depo");
        let _ = depo_kur(&kok, &depo_yolu);
        let indeks = depo_yolu.join(INDEKS_ADI);
        let metin = fs::read_to_string(&indeks).expect("okuma");
        fs::write(&indeks, metin.replace("\"surum\": 1", "\"surum\": 42")).expect("yazma");
        let sonuc = Depo::ac(&depo_yolu);
        assert!(matches!(
            sonuc,
            Err(Hata::SurumUyusmazligi { bulunan: 42, .. })
        ));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn undo_ve_redo_arsivi_geri_getirir() {
        let dizin = gecici("undo");
        let kok = dizin.join("kok");
        ornek_agac(&kok);
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let mut ayar = TaramaAyari::yeni(&kok);
        ayar.zaman = 1_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("1");
        fs::write(kok.join("a/y.txt"), "abc").expect("ekle");
        ayar.zaman = 2_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("2");
        assert_eq!(depo.adet(), 2);

        let undo = depo.undo(false).expect("undo");
        assert_eq!(undo.kimlik, "T0002");
        assert!(undo.uygulandi);
        assert!(undo.serbest_bayt > 0);
        assert_eq!(depo.adet(), 1);
        assert!(!depo.tam_yol("T0002").exists());

        let redo = depo.redo(false).expect("redo");
        assert_eq!(redo.kimlik, "T0002");
        assert!(depo.tam_yol("T0002").exists());
        assert_eq!(depo.adet(), 2);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn undo_kuru_surma_arsive_dokunmuyor() {
        let dizin = gecici("undokuru");
        let kok = dizin.join("kok");
        ornek_agac(&kok);
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let mut ayar = TaramaAyari::yeni(&kok);
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("1");
        ayar.zaman = 2_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("2");
        let onceki = depo.adet();
        let undo = depo.undo(true).expect("kuru undo");
        assert!(!undo.uygulandi);
        assert_eq!(depo.adet(), onceki);
        assert!(depo.tam_yol("T0002").exists());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn undo_yoksa_hata_donduruyor() {
        let dizin = gecici("undoyok");
        let kok = dizin.join("kok");
        fs::create_dir_all(&kok).expect("kok");
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        assert!(depo.undo(false).is_err());
        // Yalnızca bir anlık görüntü varken de geri alınamaz (zincirin başı).
        let ayar = TaramaAyari::yeni(&kok);
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("1");
        assert!(depo.undo(false).is_err());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn restore_hedefe_geridonus_yapar() {
        let dizin = gecici("restore");
        let kok = dizin.join("kok");
        ornek_agac(&kok);
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let mut ayar = TaramaAyari::yeni(&kok);
        ayar.zaman = 1_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("1");
        for i in 2..=4 {
            fs::write(kok.join(format!("f{}.txt", i)), "x").expect("ekle");
            ayar.zaman = i as i64 * 1000;
            depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
                .expect("tarama");
        }
        assert_eq!(depo.adet(), 4);
        let sonuc = depo.geri_al_hedefe("T0002", false).expect("restore");
        assert_eq!(sonuc.silinen, vec!["T0003", "T0004"]);
        assert!(sonuc.serbest_bayt > 0);
        assert_eq!(depo.adet(), 2);
        assert!(!depo.tam_yol("T0004").exists());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn restore_kuru_surma_arsive_dokunmuyor() {
        let dizin = gecici("restorekuru");
        let kok = dizin.join("kok");
        ornek_agac(&kok);
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let mut ayar = TaramaAyari::yeni(&kok);
        ayar.zaman = 1_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("1");
        fs::write(kok.join("yeni.txt"), "x").expect("ekle");
        ayar.zaman = 2_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("2");
        let sonuc = depo.geri_al_hedefe("T0001", true).expect("kuru restore");
        assert!(!sonuc.uygulandi);
        assert_eq!(sonuc.silinen, vec!["T0002"]);
        assert!(sonuc.serbest_bayt > 0);
        assert_eq!(depo.adet(), 2, "kuru çalıştırma indeksi değiştirmemeli");
        assert!(depo.tam_yol("T0002").exists());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn olmayan_kimlik_hata_donduruyor() {
        let dizin = gecici("kimlik");
        let kok = dizin.join("kok");
        fs::create_dir_all(&kok).expect("kok");
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        assert!(depo.tam_kayitlari("T0009").is_err());
        assert!(depo.geri_al_hedefe("T0009", false).is_err());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn depo_ozeti_dogru_hesaplaniyor() {
        let dizin = gecici("ozet");
        let kok = dizin.join("kok");
        ornek_agac(&kok);
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let mut ayar = TaramaAyari::yeni(&kok);
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("1");
        ayar.zaman = 2_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("2");
        let ozet = depo.ozet().expect("özet");
        assert_eq!(ozet.anlik_goruntu_sayisi, 2);
        assert_eq!(ozet.dosya_sayisi, 4);
        assert!(ozet.toplam_bayt > 0);
        assert!(ozet.undo_edilebilir);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn indeks_bozulunca_hata_donduruyor() {
        let dizin = gecici("bozukindeks");
        let kok = dizin.join("kok");
        fs::create_dir_all(&kok).expect("kok");
        let depo_yolu = dizin.join("depo");
        let _ = depo_kur(&kok, &depo_yolu);
        fs::write(depo_yolu.join(INDEKS_ADI), "{bozuk").expect("yaz");
        assert!(Depo::ac(&depo_yolu).is_err());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn indeks_olmayan_dizin_acilmaz() {
        let dizin = gecici("indeksyok");
        assert!(Depo::ac(&dizin.join("yok")).is_err());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn depodaki_baslik_satiri_bozuk_kayit_sayilmaz() {
        // Depo dosyalarının ilk satırı başlıktır; karşılaştırma sırasında bu
        // satır atlanmalıdır. Aksi hâlde her tarama "1 bozuk kayıt" uyarısı
        // üretir — kullanıcıya **yanlış** bir uyarı.
        let dizin = gecici("baslik");
        let kok = dizin.join("kok");
        ornek_agac(&kok);
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let mut ayar = TaramaAyari::yeni(&kok);
        ayar.zaman = 1_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("1");
        fs::write(kok.join("yeni.txt"), "x").expect("ekle");
        ayar.zaman = 2_000;
        let sonuc = depo
            .tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("2");
        assert_eq!(sonuc.fark.bozuk_satir, 0, "başlık bozuk sayılmamalı");
        // Yalnızca `yeni.txt` eklendi; kök klasörün toplamı 8 → 9 olduğu için
        // o **değişti** olarak sınıflanır (klasör toplamı alt ağacın toplamıdır).
        assert_eq!(sonuc.fark.eklendi, 1);
        assert_eq!(sonuc.fark.degisti, 1);
        assert_eq!(
            sonuc.fark.net_bayt, 1,
            "net etki yalnızca dosya baytlarından"
        );
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn var_olan_bilgiler_eksik_dosyayi_atlar() {
        let dizin = gecici("eksik");
        let kok = dizin.join("kok");
        ornek_agac(&kok);
        let depo_yolu = dizin.join("depo");
        let mut depo = depo_kur(&kok, &depo_yolu);
        let mut ayar = TaramaAyari::yeni(&kok);
        ayar.zaman = 1_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("1");
        ayar.zaman = 2_000;
        depo.tarama_ekle(&ayar, &dizin.join("gecici"), false)
            .expect("2");
        fs::remove_file(depo.tam_yol("T0002")).expect("sil");
        let bilgiler = depo.var_olan_bilgiler();
        assert_eq!(bilgiler.len(), 1);
        assert_eq!(bilgiler[0].kimlik, "T0001");
        // Eksik dosya **gizlenmez**: `bilgiler()` boşluğu açıkça bildirir.
        let durumlar = depo.bilgiler();
        assert_eq!(durumlar.len(), 2);
        assert!(matches!(
            durumlar[1],
            BilgiDurumu::Bozuk { ref kimlik, .. } if kimlik == "T0002"
        ));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn yollar_kayitlara_esleniyor() {
        let kayitlar = vec![
            TamKayit {
                yol: Yol::metin_yap("a"),
                tur: KayitTuru::Klasor,
                bayt: 10,
                degisiklik: 0,
            },
            TamKayit {
                yol: Yol::kok(),
                tur: KayitTuru::Klasor,
                bayt: 10,
                degisiklik: 0,
            },
        ];
        let harita = yollari_kayitlara_esle(&kayitlar);
        assert_eq!(harita.len(), 2);
        assert_eq!(ilk_klasor(&kayitlar).map(|k| k.yol.metin()), Some("a"));
    }
}
