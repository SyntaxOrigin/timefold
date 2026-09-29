//! Özyinelemeli dizin gezgini: sembolik bağ koruması, izin hatalarının
//! atlanması, dışlama kuralları ve örnekli saklama (örnekleme).
//!
//! Gezinme **özyinelemeli değildir**: açık bir yığın (stack) kullanılır. Bu iki
//! nedenle zorunludur. Birincisi, raporun R6 riski: çok derin bir ağaçta
//! özyinelemeli uygulama yığın taşırır. İkincisi, `Drop` sırasında çalışan
//! yıkıcılar nedeniyle testlerin geçici dizinlerini yanlışlıkla silme riskini
//! azaltır.
//!
//! Bellek: gezinme sırasında bellekte yalnızca **yığın derinliği** kadar çerçeve
//! tutulur. Klasör toplamları aşağıdan yukarı, çerçeve üzerinde biriktirilerek
//! hesaplanır; hiçbir zaman tüm ağaç belleğe alınmaz. Sıralı çıktı için
//! [`crate::akis::Sirala`] kullanılır.

use std::fs::ReadDir;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::akis::{Sirala, SiraliSatir};
use crate::hata::{Hata, Sonuc};
use crate::kayit::{KayitTuru, TamKayit};
use crate::yol::{Dislama, Yol};

/// Gezinmenin özyinelemeli olmayan yığınında bir klasör çerçevesi.
///
/// Çerçeve, alt ağacı **taranmadan önce** açılır ve taranana kadar yığında
/// kalır. Klasörün toplamı, alt ağacı bittikten sonra (çerçeve yığından
/// düşürülürken) üst çerçeveye aktarılır. Bu yüzden bellekte yalnızca
/// **açık klasörlerin sayısı kadar** çerçeve tutulur; tüm ağaç belleğe
/// alınmaz.
struct Cerceve {
    yol: PathBuf,
    goreli: Yol,
    derinlik: usize,
    /// Bu klasörün kendi dosyaları + alt klasörlerinin toplam baytı.
    toplam: u64,
    dosya: u64,
    degisiklik: i64,
    /// Klasörün içeriği; `None` ise henüz açılmamış (derinlik sınırı).
    okuyucu: Option<ReadDir>,
}

/// Tarama ayarları.
#[derive(Debug, Clone)]
pub struct TaramaAyari {
    /// Taranacak kök dizin.
    pub kok: PathBuf,
    /// Dışlanacak ad desenleri ve gizli dosya kuralı.
    pub dislama: Dislama,
    /// İzlenecek en derin seviye (kök = 1).
    ///
    /// Üst sınır, sembolik bağ olmayan bir ağaçta bile sonsuz derinliği
    /// engeller; bir bağ döngüsü bu sınır sayesinde çerçeve taşmadan biter.
    pub en_fazla_derinlik: usize,
    /// Saklanacak en fazla giriş sayısı. `None` ise sınır yoktur.
    ///
    /// Aşılırsa **örnekleme** devreye girer: dosya girdileri deterministik olarak
    /// seyreltilir, klasör toplamları alt sınır olur ve `orneklendi` işaretlenir.
    pub en_fazla_giris: Option<u64>,
    /// Zaman damgası kaynağı (testlerde sabit zaman enjekte edilir).
    pub zaman: i64,
    /// Taramadan tamamen çıkarılacak mutlak yollar.
    ///
    /// Depo dizini bu listeye eklenir: araç kendi yazdığı dosyaları tararsa
    /// "ne kadar büyüdü" sorusu kendi çıktısıyla şişer ve sonuç yanıltıcı olur.
    pub haric_yollar: Vec<PathBuf>,
}

impl TaramaAyari {
    /// Verilen kök için varsayılan ayarları üretir.
    ///
    /// Varsayılan derinlik sınırı 64, giriş sınırı yoktur (tam tarama).
    pub fn yeni(kok: impl Into<PathBuf>) -> Self {
        TaramaAyari {
            kok: kok.into(),
            dislama: Dislama::yeni(),
            en_fazla_derinlik: 64,
            en_fazla_giris: None,
            zaman: crate::zaman::simdi_unix_saniye(),
            haric_yollar: Vec::new(),
        }
    }

    /// Taramadan çıkarılacak bir mutlak yol ekler.
    pub fn haric_yol_ekle(&mut self, yol: impl Into<PathBuf>) {
        self.haric_yollar.push(yol.into());
    }

    /// Kökün bir dizin olduğunu doğrular.
    pub fn dogrula(&self) -> Sonuc<()> {
        let meta = std::fs::metadata(&self.kok).map_err(|kaynak| Hata::TaramaYoluHatali {
            yol: self.kok.clone(),
            ayrinti: format!("okunamıyor: {}", kaynak),
        })?;
        if !meta.is_dir() {
            return Err(Hata::TaramaYoluHatali {
                yol: self.kok.clone(),
                ayrinti: "bir dizin değil".to_owned(),
            });
        }
        if self.en_fazla_derinlik == 0 {
            return Err(Hata::parametre(
                "en-fazla-derinlik",
                "sıfır veya daha küçük olamaz",
            ));
        }
        if let Some(sinir) = self.en_fazla_giris {
            if sinir == 0 {
                return Err(Hata::parametre(
                    "en-fazla-giris",
                    "sıfır veya daha küçük olamaz (kaldırmak için bayrağı kullanmayın)",
                ));
            }
        }
        Ok(())
    }
}

/// Bir taramadan elde edilen özet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaramaOzeti {
    /// Görülen toplam giriş sayısı (örneklemeden bağımsız).
    pub taranan: u64,
    /// Depoya yazılan kayıt sayısı.
    pub yazilan: u64,
    /// Taranan dosyaların toplam baytı.
    pub toplam_bayt: u64,
    /// Klasör sayısı.
    pub klasor: u64,
    /// Dosya sayısı.
    pub dosya: u64,
    /// İzin hatası nedeniyle atlanan girdi sayısı.
    pub atlanan_izin: u64,
    /// Sembolik bağ nedeniyle atlanan girdi sayısı.
    pub atlanan_sembolik: u64,
    /// Dışlama kuralına takılan girdi sayısı.
    pub atlanan_dislama: u64,
    /// UTF-8 dışı ada dönüştürülen yol sayısı.
    pub yol_kaybi: u64,
    /// Derinlik sınırı nedeniyle yarım kalan ağaç sayısı.
    pub kesilen: u64,
    /// Örnekleme uygulandı mı?
    pub orneklendi: bool,
    /// Kısmi tarama gerekçesi (kısmi değilse boş).
    pub gerekce: Vec<String>,
}

impl TaramaOzeti {
    /// Tarama kısmi mı?
    pub fn kismi_mi(&self) -> bool {
        self.atlanan_izin > 0
            || self.atlanan_sembolik > 0
            || self.kesilen > 0
            || !self.gerekce.is_empty()
    }

    /// Kısmi tarama gerekçesini tek satırda birleştirir.
    pub fn gerekce_metni(&self) -> String {
        if self.gerekce.is_empty() {
            return String::new();
        }
        self.gerekce.join("; ")
    }
}

/// Gezinmenin iç durumu: yığın, özet ve örnekleme sayacı.
struct Gezgin<'a> {
    ayar: &'a TaramaAyari,
    ozet: TaramaOzeti,
    ornek_adimi: Option<u64>,
    sayac: u64,
    ziyaret_edilen: Vec<PathBuf>,
}

/// Tam taramayı belleğe alarak yapar; küçük ağaçlar ve birim testleri içindir.
///
/// Çok büyük ağaçlarda kullanılmamalıdır: tüm kayıtlar bellekte tutulur.
/// Üretim yolu [`tara_akisa`]'dır.
pub fn tara_bellege(ayari: &TaramaAyari) -> Sonuc<(Vec<TamKayit>, TaramaOzeti)> {
    let ornek_adimi = ornek_adimi_hesapla(ayari)?;
    let mut kayitlar = Vec::new();
    let ozet = gez(ayari, ornek_adimi, &mut |kayit| {
        kayitlar.push(kayit);
        Ok(())
    })?;
    // Dosya sistemi girdi sırası platforma göre değişebilir; kanonik sıralama
    // yaparak iki çalıştırmanın aynı listeyi vermesi garanti edilir.
    kayitlar.sort_by(|a, b| a.yol.cmp(&b.yol));
    Ok((kayitlar, ozet))
}

/// Taramayı yapar ve sonucu sıralı JSONL dosyasına akış hâlinde yazar.
///
/// Sıralama dış diskte (koşu dosyaları) yapılır; bellek tavanı
/// [`crate::akis::SIRALAMA_TAMPONU`] satırdır. `gecici_dizin`, koşu dosyalarının
/// yazılacağı dizindir; sonuçta temizlenir.
pub fn tara_akisa(
    ayari: &TaramaAyari,
    gecici_dizin: &Path,
    cikti: &Path,
) -> Sonuc<(PathBuf, TaramaOzeti)> {
    std::fs::create_dir_all(gecici_dizin)
        .map_err(|kaynak| Hata::io("geçici dizin oluştur", gecici_dizin, kaynak))?;
    let ornek_adimi = ornek_adimi_hesapla(ayari)?;
    let mut sirala = Sirala::yeni(gecici_dizin);
    let yol = cikti.to_path_buf();
    let ozet = gez(ayari, ornek_adimi, &mut |kayit| {
        let satir = serde_json::to_string(&kayit).map_err(|kaynak| Hata::BozukSatir {
            dosya: yol.clone(),
            satir: 0,
            ayrinti: format!("kayıt serileştirilemedi: {}", kaynak),
        })?;
        sirala.ekle(SiraliSatir {
            yol: kayit.yol.clone(),
            satir,
        })
    })?;
    sirala.bitir(cikti)?;
    Ok((cikti.to_path_buf(), ozet))
}

/// Gezinmeyi çalıştırır; `ornek_adimi` önceden hesaplanmış olmalıdır (bkz.
/// [`ornek_adimi_hesapla`]).
fn gez<F>(ayar: &TaramaAyari, ornek_adimi: Option<u64>, ziyaretci: &mut F) -> Sonuc<TaramaOzeti>
where
    F: FnMut(TamKayit) -> Sonuc<()>,
{
    ayar.dogrula()?;
    let mut gezgin = Gezgin {
        ayar,
        ozet: TaramaOzeti::default(),
        ornek_adimi,
        sayac: 0,
        ziyaret_edilen: Vec::new(),
    };
    if let Some(adim) = gezgin.ornek_adimi {
        gezgin.ozet.orneklendi = true;
        gezgin.ozet.gerekce.push(format!(
            "{} dosya görüldü, sınır gereği yolun FNV-1a toplamı {} değerine bölünen dosyalar saklandı ({} kat seyreltme); klasör toplamları alt sınırdır",
            gezgin.sayac, adim, adim
        ));
    }
    gezgin.calistir(ziyaretci)?;
    Ok(gezgin.ozet)
}

/// Örneklemeye karar veren iki geçişli ön adım.
///
/// 1. **Sayım geçişi:** hiçbir kayıt saklamadan dosya sayısı ölçülür. Bu geçiş
///    bellek tutmaz; yalnızca `read_dir` maliyetini ikiye katlar.
/// 2. **Adım:** `adım = dosya sayısı / sınır`. `adım == 1` ise örnekleme gerekmez.
///
/// Adım, **içerik tabanlı** seyreltme için kullanılır (bkz.
/// [`Gezgin::ornege_duserse`]); bu yüzden sonuç gezinme sırasından bağımsız ve
/// tekrarlanabilirdir.
fn ornek_adimi_hesapla(ayar: &TaramaAyari) -> Sonuc<Option<u64>> {
    let Some(sinir) = ayar.en_fazla_giris else {
        return Ok(None);
    };
    let mut sayaci = |_kayit: TamKayit| Ok(());
    let ozet = gez(ayar, None, &mut sayaci)?;
    Ok(Some((ozet.dosya / sinir).max(1)))
}

impl<'a> Gezgin<'a> {
    fn calistir<F>(&mut self, ziyaretci: &mut F) -> Sonuc<()>
    where
        F: FnMut(TamKayit) -> Sonuc<()>,
    {
        let kok_meta =
            std::fs::metadata(&self.ayar.kok).map_err(|kaynak| Hata::TaramaYoluHatali {
                yol: self.ayar.kok.clone(),
                ayrinti: format!("okunamıyor: {}", kaynak),
            })?;
        // Kanonik yol koruması: kök bir kez işaretlenir, alt klasörler de
        // aynı listeye eklenir (birleşme noktası / bağ döngüsü koruması).
        if let Ok(kanonik) = std::fs::canonicalize(&self.ayar.kok) {
            self.ziyaret_edilen.push(kanonik);
        }

        let mut yigin: Vec<Cerceve> = vec![Cerceve {
            yol: self.ayar.kok.clone(),
            goreli: Yol::kok(),
            derinlik: 1,
            toplam: 0,
            dosya: 0,
            degisiklik: zaman_damgasi(&kok_meta, self.ayar.zaman),
            okuyucu: None,
        }];

        while !yigin.is_empty() {
            // 1) Yığının tepesindeki klasörün okuyucusu açık mı?
            let acilacak: Option<(PathBuf, bool)> = {
                let cerceve = yigin
                    .last()
                    .ok_or_else(|| Hata::gecersiz_durum("yığın boş"))?;
                if cerceve.okuyucu.is_none() {
                    Some((
                        cerceve.yol.clone(),
                        cerceve.derinlik >= self.ayar.en_fazla_derinlik,
                    ))
                } else {
                    None
                }
            };
            if let Some((yol, sinirda)) = acilacak {
                if sinirda {
                    self.derinlik_sinirina_girdi(&yol);
                    let dusen = yigin
                        .pop()
                        .ok_or_else(|| Hata::gecersiz_durum("yığın boş"))?;
                    self.cerceveyi_kapat(dusen, &mut yigin, ziyaretci)?;
                    continue;
                }
                match std::fs::read_dir(&yol) {
                    Ok(okuyucu) => {
                        if let Some(cerceve) = yigin.last_mut() {
                            cerceve.okuyucu = Some(okuyucu);
                        }
                    }
                    Err(_) => {
                        // İzin hatası: dizin atlanır, tarama sürer.
                        self.ozet.atlanan_izin += 1;
                        let goreli = yigin
                            .last()
                            .map(|c| c.goreli.clone())
                            .unwrap_or_else(Yol::kok);
                        self.ozet
                            .gerekce
                            .push(format!("{} okunamadı, atlandı", goreli));
                        let dusen = yigin
                            .pop()
                            .ok_or_else(|| Hata::gecersiz_durum("yığın boş"))?;
                        self.cerceveyi_kapat(dusen, &mut yigin, ziyaretci)?;
                        continue;
                    }
                }
            }

            // 2) Sıradaki girdiyi oku.
            let sonraki = {
                let cerceve = yigin
                    .last_mut()
                    .ok_or_else(|| Hata::gecersiz_durum("yığın boş"))?;
                match cerceve.okuyucu.as_mut() {
                    Some(okuyucu) => okuyucu.next(),
                    None => None,
                }
            };
            match sonraki {
                None => {
                    let dusen = yigin
                        .pop()
                        .ok_or_else(|| Hata::gecersiz_durum("yığın boş"))?;
                    self.cerceveyi_kapat(dusen, &mut yigin, ziyaretci)?;
                }
                Some(Err(_)) => {
                    self.ozet.atlanan_izin += 1;
                    self.ozet
                        .gerekce
                        .push("dizin girdisi okunamadı, atlandı".to_owned());
                }
                Some(Ok(giris)) => {
                    let yol = giris.path();
                    self.sayac += 1;
                    self.ozet.taranan += 1;

                    let Ok(tur) = std::fs::symlink_metadata(&yol) else {
                        self.ozet.atlanan_izin += 1;
                        self.ozet
                            .gerekce
                            .push(format!("{} okunamadı, atlandı", yol.display()));
                        continue;
                    };
                    if tur.file_type().is_symlink() {
                        // Sembolik bağ hiçbir zaman izlenmez: döngü ve kopya
                        // sayımı riski. Bu, ağaç dışına çıkmanın tek yoludur.
                        self.ozet.atlanan_sembolik += 1;
                        continue;
                    }

                    let (goreli, kayip) = Yol::yoldan(&self.ayar.kok, &yol);
                    if kayip {
                        self.ozet.yol_kaybi += 1;
                    }
                    if self.ayar.dislama.disli_mi(&goreli) {
                        self.ozet.atlanan_dislama += 1;
                        continue;
                    }
                    if self.haric_mi(&yol) {
                        self.ozet.atlanan_dislama += 1;
                        continue;
                    }

                    if tur.is_dir() {
                        let kanonik = std::fs::canonicalize(&yol).unwrap_or_else(|_| yol.clone());
                        if self.ziyaret_edilen.contains(&kanonik) {
                            self.ozet.atlanan_sembolik += 1;
                            continue;
                        }
                        self.ziyaret_edilen.push(kanonik);
                        let derinlik = yigin.last().map(|c| c.derinlik + 1).unwrap_or(2);
                        yigin.push(Cerceve {
                            yol,
                            goreli,
                            derinlik,
                            toplam: 0,
                            dosya: 0,
                            degisiklik: zaman_damgasi(&tur, self.ayar.zaman),
                            okuyucu: None,
                        });
                        continue;
                    }

                    if !tur.is_file() {
                        // Soket, FIFO, cihaz vb.: boyut bilgisi anlamlı değildir.
                        self.ozet.atlanan_dislama += 1;
                        continue;
                    }
                    if self.ornege_duserse(&goreli) {
                        continue;
                    }

                    let boyut = tur.len();
                    let degisiklik = zaman_damgasi(&tur, self.ayar.zaman);
                    if let Some(cerceve) = yigin.last_mut() {
                        cerceve.toplam = cerceve.toplam.saturating_add(boyut);
                        cerceve.dosya += 1;
                    }
                    self.ozet.dosya += 1;
                    self.ozet.yazilan += 1;
                    ziyaretci(TamKayit {
                        yol: goreli,
                        tur: KayitTuru::Dosya,
                        bayt: boyut,
                        degisiklik,
                    })?;
                }
            }
        }
        Ok(())
    }

    /// Verilen mutlak yol, taramadan tamamen çıkarılacak mı?
    ///
    /// Karşılaştırma kanonikleştirilmiş yol üzerinden yapılır; böylece
    /// `depo`, `.\depo` ve `kok/../depo` aynı sayılır.
    fn haric_mi(&self, yol: &Path) -> bool {
        if self.ayar.haric_yollar.is_empty() {
            return false;
        }
        let kanonik = std::fs::canonicalize(yol).unwrap_or_else(|_| yol.to_path_buf());
        self.ayar.haric_yollar.iter().any(|haric| {
            let hedef = std::fs::canonicalize(haric).unwrap_or_else(|_| haric.clone());
            hedef == kanonik
        })
    }

    /// Derinlik sınırına takılan klasörü işaretler.
    ///
    /// Sınırda **klasörün kendisi kaydedilir**, altındaki girdiler gezilmez.
    /// Böylece ağaç yapısı bozulmaz, yalnızca derinlikteki veri eksik olur ve
    /// bu `kesilen` sayacıyla bildirilir.
    fn derinlik_sinirina_girdi(&mut self, yol: &Path) {
        let bos = match std::fs::read_dir(yol) {
            Ok(mut girdiler) => girdiler.next().is_none(),
            // Okunamayan dizin için izin sayacı zaten artırıldı; tekrar saymak
            // `kesilen`in anlamını bozardı.
            Err(_) => true,
        };
        if !bos {
            self.ozet.kesilen += 1;
            self.ozet.gerekce.push(format!(
                "derinlik sınırı ({} seviye) nedeniyle {} altı taranmadı",
                self.ayar.en_fazla_derinlik,
                yol.display()
            ));
        }
    }

    /// Tamamlanan klasör çerçevesini kapatır: kaydı yazar, toplamı üste aktarır.
    ///
    /// Klasör kaydı, alt ağacı gezdikten sonra yazılır; böylece `bayt` alanı
    /// alt ağacın **tam** toplamını taşır.
    fn cerceveyi_kapat<F>(
        &mut self,
        cerceve: Cerceve,
        yigin: &mut [Cerceve],
        ziyaretci: &mut F,
    ) -> Sonuc<()>
    where
        F: FnMut(TamKayit) -> Sonuc<()>,
    {
        self.ozet.klasor += 1;
        self.ozet.yazilan += 1;
        if let Some(ust) = yigin.last_mut() {
            ust.toplam = ust.toplam.saturating_add(cerceve.toplam);
            ust.dosya += cerceve.dosya;
        } else {
            // Kök kapatıldığında genel sayaçlar zaten dosya dosya eklendiği için
            // yalnızca toplam burada yazılır; aksi hâlde her dosya iki kez sayılırdı.
            self.ozet.toplam_bayt = cerceve.toplam;
        }
        ziyaretci(TamKayit {
            yol: cerceve.goreli,
            tur: KayitTuru::Klasor,
            bayt: cerceve.toplam,
            degisiklik: cerceve.degisiklik,
        })
    }

    /// Örnekleme kuralı: `adım > 1` ise dosya **içeriğine göre** seyreltilir.
    ///
    /// Karar, dosyanın göreli yolunun FNV-1a sağlama toplamına dayanır:
    /// `toplam % adım == 0` olan dosyalar saklanır. Bu, gezinme sırasından
    /// bağımsız olduğu için iki çalıştırma **aynı** dosyaları saklar; sıraya
    /// dayalı bir "her n. dosya" kuralı ise dosya sistemi girdi sırasına bağlı
    /// olduğu için tekrarlanabilir olmazdı.
    ///
    /// Klasörler **her zaman** saklanır; aksi hâlde ağaç yapısı ve klasör
    /// toplamları kaybolurdu. Bu yüzden örneklenmiş taramada klasör toplamları
    /// bir **alt sınırdır**: ölçekleme yapılmaz, çünkü ölçeklemek tahmin
    /// üretmek anlamına gelirdi.
    fn ornege_duserse(&mut self, goreli: &Yol) -> bool {
        let Some(adim) = self.ornek_adimi else {
            return false;
        };
        let mut saglama = crate::kayit::Saglama::yeni();
        saglama.yol_ekle(goreli);
        if saglama.deger() % adim == 0 {
            return false;
        }
        true
    }
}

fn zaman_damgasi(meta: &std::fs::Metadata, varsayilan: i64) -> i64 {
    match meta.modified() {
        Ok(an) => match an.duration_since(UNIX_EPOCH) {
            Ok(fark) => fark.as_secs() as i64,
            Err(_) => varsayilan,
        },
        Err(_) => varsayilan,
    }
}

/// Bir kaydın okunabilir tek satırlık özetini üretir (terminal çıktısı için).
pub fn kayit_ozeti(kayit: &TamKayit) -> String {
    format!(
        "{} {} {} bayt",
        kayit.tur.etiket(),
        kayit.yol,
        bayt_metni(kayit.bayt)
    )
}

/// Bayt sayısını okunabilir birimle yazar (B/KB/MB/GB/TB).
pub fn bayt_metni(bayt: u64) -> String {
    const BİRİMLER: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut deger = bayt as f64;
    let mut sira = 0usize;
    while deger >= 1024.0 && sira + 1 < BİRİMLER.len() {
        deger /= 1024.0;
        sira += 1;
    }
    if sira == 0 {
        format!("{} {}", bayt, BİRİMLER[0])
    } else {
        format!("{:.1} {}", deger, BİRİMLER[sira])
    }
}

/// Verilen unix saniyesini okunabilir biçimde yazar.
pub fn zaman_metni(saniye: i64) -> String {
    crate::zaman::unix_saniye_rfc3339(saniye)
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
            std::env::temp_dir().join(format!("timefold-tarama-{}-{}", etiket, std::process::id()));
        let _ = fs::remove_dir_all(&yol);
        fs::create_dir_all(&yol).expect("geçici dizin");
        yol
    }

    fn yaz(yol: &Path, icerik: &str) {
        if let Some(usta) = yol.parent() {
            fs::create_dir_all(usta).expect("üst dizin");
        }
        fs::write(yol, icerik).expect("dosya yaz");
    }

    #[test]
    fn bos_klasor_tek_kayit_uretir() {
        let dizin = gecici("bos");
        let ayar = TaramaAyari::yeni(&dizin);
        let (kayitlar, ozet) = tara_bellege(&ayar).expect("tarama");
        assert_eq!(kayitlar.len(), 1);
        assert_eq!(kayitlar[0].yol, Yol::kok());
        assert_eq!(kayitlar[0].bayt, 0);
        assert_eq!(ozet.toplam_bayt, 0);
        assert!(!ozet.kismi_mi());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn tek_dosya_boyutu_dogru() {
        let dizin = gecici("tek");
        yaz(&dizin.join("a.txt"), "0123456789");
        let ayar = TaramaAyari::yeni(&dizin);
        let (kayitlar, ozet) = tara_bellege(&ayar).expect("tarama");
        assert_eq!(kayitlar.len(), 2);
        assert_eq!(ozet.toplam_bayt, 10);
        assert_eq!(ozet.dosya, 1);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn ic_ice_derin_agac_toplamlari_toplaniyor() {
        let dizin = gecici("derin");
        yaz(&dizin.join("a/b/c/d.txt"), "12345");
        yaz(&dizin.join("a/b/e.txt"), "123");
        yaz(&dizin.join("f.txt"), "1");
        let ayar = TaramaAyari::yeni(&dizin);
        let (kayitlar, ozet) = tara_bellege(&ayar).expect("tarama");

        let kok = kayitlar
            .iter()
            .find(|k| k.yol.kok_mu())
            .expect("kok kaydı var");
        assert_eq!(kok.bayt, 9);
        let a = kayitlar
            .iter()
            .find(|k| k.yol.metin() == "a")
            .expect("a kaydı var");
        assert_eq!(a.bayt, 8);
        assert_eq!(ozet.toplam_bayt, 9);
        assert_eq!(ozet.dosya, 3);
        assert_eq!(ozet.klasor, 4);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn gizli_dosyalar_varsayilan_taranir() {
        let dizin = gecici("gizli");
        yaz(&dizin.join(".gizli"), "12345");
        let mut ayar = TaramaAyari::yeni(&dizin);
        ayar.dislama.gizlileri_haric_ayarla(false);
        let (_, ozet) = tara_bellege(&ayar).expect("tarama");
        assert_eq!(ozet.dosya, 1);
        ayar.dislama.gizlileri_haric_ayarla(true);
        let (_, ozet2) = tara_bellege(&ayar).expect("tarama");
        assert_eq!(ozet2.dosya, 0);
        assert_eq!(ozet2.atlanan_dislama, 1);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn arsiv_dislama_deseni_uygulaniyor() {
        let dizin = gecici("disla");
        yaz(&dizin.join("a.tmp"), "1");
        yaz(&dizin.join("b.txt"), "1");
        let mut ayar = TaramaAyari::yeni(&dizin);
        ayar.dislama.desen_ekle("*.tmp");
        let (_, ozet) = tara_bellege(&ayar).expect("tarama");
        assert_eq!(ozet.dosya, 1);
        assert_eq!(ozet.atlanan_dislama, 1);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn sembolik_bag_izlenmiyor() {
        let dizin = gecici("sembol");
        yaz(&dizin.join("gercek/a.txt"), "12345");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dizin.join("gercek"), dizin.join("bag")).expect("bağ");
        }
        #[cfg(windows)]
        {
            // Windows'ta sembolik bağ oluşturma yönetici/developer modu gerektirir;
            // oluşturulamazsa test, "bağ yok" durumunda da geçerli olacak şekilde
            // yazılır: tarama yine de tek bir kopya saymalıdır.
            let _ = std::os::windows::fs::symlink_dir(dizin.join("gercek"), dizin.join("bag"));
        }
        let ayar = TaramaAyari::yeni(&dizin);
        let (_, ozet) = tara_bellege(&ayar).expect("tarama");
        assert_eq!(ozet.dosya, 1, "sembolik bağ ikinci kez sayılmamalı");
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn derinlik_siniri_yarim_tarama_olusum_tesbit_ediliyor() {
        let dizin = gecici("sinir");
        yaz(&dizin.join("a/b/c/d.txt"), "12345");
        let mut ayar = TaramaAyari::yeni(&dizin);
        ayar.en_fazla_derinlik = 2;
        let (_, ozet) = tara_bellege(&ayar).expect("tarama");
        assert!(ozet.kismi_mi());
        assert!(!ozet.gerekce.is_empty());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn ornekleme_giris_sinirini_uzuyor() {
        let dizin = gecici("ornek");
        for i in 0..20 {
            yaz(&dizin.join(format!("dosya{:02}.txt", i)), "x");
        }
        let mut ayar = TaramaAyari::yeni(&dizin);
        ayar.en_fazla_giris = Some(5);
        let (kayitlar, ozet) = tara_bellege(&ayar).expect("tarama");
        assert!(ozet.orneklendi);
        assert!(ozet.dosya < 20, "örnekleme dosya sayısını azaltmalı");
        assert!(!kayitlar.is_empty());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn gecersiz_kok_hata_veriyor() {
        let ayar = TaramaAyari::yeni("C:/yok/olmayan/kok");
        assert!(tara_bellege(&ayar).is_err());
    }

    #[test]
    fn dosya_olarak_verilen_kok_reddediliyor() {
        let dizin = gecici("dosyakok");
        let dosya = dizin.join("a.txt");
        yaz(&dosya, "x");
        let ayar = TaramaAyari::yeni(&dosya);
        assert!(tara_bellege(&ayar).is_err());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn sifir_sinirlar_reddediliyor() {
        let dizin = gecici("sifir");
        let mut ayar = TaramaAyari::yeni(&dizin);
        ayar.en_fazla_derinlik = 0;
        assert!(tara_bellege(&ayar).is_err());
        let mut ayar2 = TaramaAyari::yeni(&dizin);
        ayar2.en_fazla_giris = Some(0);
        assert!(tara_bellege(&ayar2).is_err());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn akisa_tarama_sirali_cikti_uretir() {
        let dizin = gecici("akis");
        yaz(&dizin.join("z/b.txt"), "12");
        yaz(&dizin.join("a.txt"), "1");
        // Geçici dizin taranan ağacın **dışında** olmalı; aksi hâlde sıralama
        // koşu dosyalarını da kaydeder ve sonuç kendi çıktısıyla şişer.
        let gecici_dizin =
            std::env::temp_dir().join(format!("timefold-gecici-{}", std::process::id()));
        let cikti = dizin.join("cikti.jsonl");
        let ayar = TaramaAyari::yeni(&dizin);
        let (_, ozet) = tara_akisa(&ayar, &gecici_dizin, &cikti).expect("akış tarama");
        assert_eq!(ozet.toplam_bayt, 3);
        let metin = fs::read_to_string(&cikti).expect("okuma");
        let yollar: Vec<String> = metin
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|v| v.get("yol").and_then(|y| y.as_str()).map(str::to_owned))
            .collect();
        assert_eq!(yollar, vec!["", "a.txt", "z", "z/b.txt"]);
        assert!(!gecici_dizin.join("kosu-000000.jsonl").exists());
        let _ = fs::remove_dir_all(&dizin);
        let _ = fs::remove_dir_all(&gecici_dizin);
    }

    #[test]
    fn bayt_metni_birimleri_dogru() {
        assert_eq!(bayt_metni(0), "0 B");
        assert_eq!(bayt_metni(512), "512 B");
        assert_eq!(bayt_metni(1024), "1.0 KB");
        assert_eq!(bayt_metni(1024 * 1024), "1.0 MB");
        assert_eq!(bayt_metni(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn kayit_ozeti_okunabilir() {
        let kayit = TamKayit {
            yol: Yol::metin_yap("a/b.txt"),
            tur: KayitTuru::Dosya,
            bayt: 2048,
            degisiklik: 0,
        };
        assert_eq!(kayit_ozeti(&kayit), "dosya a/b.txt 2.0 KB bayt");
    }

    #[test]
    fn iki_kez_tarama_ayni_sonucu_uretir() {
        let dizin = gecici("iki");
        yaz(&dizin.join("a.txt"), "12");
        let ayar = TaramaAyari::yeni(&dizin);
        let (b1, o1) = tara_bellege(&ayar).expect("1");
        let (b2, o2) = tara_bellege(&ayar).expect("2");
        assert_eq!(b1, b2);
        assert_eq!(o1, o2);
        let _ = fs::remove_dir_all(&dizin);
    }
}
