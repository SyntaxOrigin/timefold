//! İki anlık görüntüyü karşılaştırma ve değişim sınıflandırması.
//!
//! Karşılaştırma **akış hâlinde** yapılır: iki sıralı JSONL dosyası aynı anda
//! okunur ve fark dosyasına yazılır. Bellek tavanı **iki kayıttır**; iki dev
//! ağaç belleğe alınmaz. Bu, raporun "delta çalışma kümesi (iki pencere)" bellek
//! kaleminin saf Rust karşılığıdır.
//!
//! Sınıflandırma kuralı:
//!
//! | Önceki | Şimdi | Sınıf |
//! |---|---|---|
//! | yok | var | `eklendi` |
//! | var | yok | `silindi` |
//! | var, bayt aynı | var, bayt aynı | *kayıt yok* (fark boş) |
//! | var | var, bayt farklı | `degisti` |
//!
//! **`ad_degisti` üretilmez.** Dosya sistemi bize "taşındı" bilgisini vermez; aynı
//! boyutlu bir silme + ekleme çiftini yeniden adlandırma saymak, "ne oldu"
//! sorusuna uydurma cevap vermek olurdu ve raporun en güçlü dürüstlük kuralıyla
//! (boşluğu doldurma yasağı) çelişirdi. `Degisim::AdDegisti` şeması ileride
//! gerçek bir kanıt kaynağı (USN/inotify) eklendiğinde kullanılmak üzere durur.

use std::path::{Path, PathBuf};

use crate::akis::{AkisYazici, SatirOkuyucu};
use crate::hata::{Hata, Sonuc};
use crate::kayit::{Degisim, TamKayit};
use crate::yol::Yol;

/// Karşılaştırma sonucunda toplanan sayaçlar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FarkOzeti {
    /// Eklenen yol sayısı.
    pub eklendi: u64,
    /// Silinen yol sayısı.
    pub silindi: u64,
    /// Boyutu değişen yol sayısı.
    pub degisti: u64,
    /// Okunamayan (bozuk) satır sayısı.
    pub bozuk_satir: u64,
    /// Net bayt değişimi (eklenen eksi silinen artı değişen fark).
    pub net_bayt: i64,
}

impl FarkOzeti {
    /// Toplam değişim kaydı sayısını döndürür.
    pub fn toplam(&self) -> u64 {
        self.eklendi + self.silindi + self.degisti
    }

    /// Farkın boş olup olmadığını söyler.
    pub fn bos_mu(&self) -> bool {
        self.toplam() == 0
    }
}

/// Bir tarafı okuyan, "mevcut kayda" gören tanık.
struct Tanik {
    okuyucu: SatirOkuyucu,
    mevcut: Option<TamKayit>,
    bozuk: u64,
}

impl Tanik {
    /// Yeni tanık açar, isteğe bağlı başlık satırını atlar ve ilk kaydı okur.
    fn ac(kaynak: &Kaynak) -> Sonuc<Self> {
        let mut okuyucu = SatirOkuyucu::ac(&kaynak.yol)?;
        if kaynak.basligi_var {
            okuyucu.sonraki()?;
        }
        let mut bozuk = 0u64;
        let ilk = Self::sonraki_gecerli(&mut okuyucu, &mut bozuk)?;
        Ok(Tanik {
            okuyucu,
            mevcut: ilk,
            bozuk,
        })
    }

    /// Bir sonraki geçerli kaydı okur; bozuk satırları atlayıp sayar.
    fn sonraki_gecerli(okuyucu: &mut SatirOkuyucu, bozuk: &mut u64) -> Sonuc<Option<TamKayit>> {
        loop {
            match okuyucu.sonraki()? {
                None => return Ok(None),
                Some(satir) if satir.trim().is_empty() => continue,
                Some(satir) => match serde_json::from_str::<TamKayit>(&satir) {
                    Ok(kayit) => return Ok(Some(kayit)),
                    Err(_) => {
                        // Bozuk satır atlanır ve sayılır (raporun "bozuk kayıt
                        // atlanır ve uyarılır" kuralı).
                        *bozuk += 1;
                        continue;
                    }
                },
            }
        }
    }

    /// Mevcut kaydı alır ve sıradakini yükler.
    fn al(&mut self) -> Sonuc<Option<TamKayit>> {
        let mevcut = self.mevcut.take();
        let mut bozuk = self.bozuk;
        self.mevcut = Self::sonraki_gecerli(&mut self.okuyucu, &mut bozuk)?;
        self.bozuk = bozuk;
        Ok(mevcut)
    }
}

/// İki sıralı anlık görüntü dosyasını karşılaştırıp fark dosyası üretir.
///
/// `eski` ve `yeni` yoluna göre sıralı JSONL olmalıdır; çıktı da sıralıdır.
/// `eski` dosyası yoksa (ilk tarama) tüm yeni kayıtlar `eklendi` olur.
/// Bozuk satırlar atlanır ve `bozuk_satir` sayacında bildirilir.
pub fn karsilastir(eski: Option<&Kaynak>, yeni: &Kaynak, cikti: &Path) -> Sonuc<FarkOzeti> {
    let mut ozet = FarkOzeti::default();
    let mut yazici = AkisYazici::olustur(cikti)?;
    let mut sol_taraf: Option<Tanik> = match eski {
        Some(kaynak) => Some(Tanik::ac(kaynak)?),
        None => None,
    };
    let mut sag_taraf = Tanik::ac(yeni)?;

    loop {
        // İki tarafın mevcut kayıtlarını karşılaştır; daha küçük yolun bulunduğu
        // taraf ilerletilir, yollar eşitse ikisi birlikte tüketilir.
        let sol_yol = sol_taraf
            .as_ref()
            .and_then(|t| t.mevcut.as_ref())
            .map(|k| &k.yol);
        let sag_yol = sag_taraf.mevcut.as_ref().map(|k| &k.yol);

        let sol_once = match (sol_yol, sag_yol) {
            (None, None) => break,
            (Some(a), Some(b)) => a <= b,
            (Some(_), None) => true,
            (None, Some(_)) => false,
        };

        let degisim = if sol_once {
            let sol_kayit = sol_taraf
                .as_mut()
                .map(|t| t.al())
                .transpose()?
                .flatten()
                .ok_or_else(|| Hata::gecersiz_durum("sol tanık beklenen kaydı vermedi"))?;
            // `sag_yol`, sol kaydı tüketilmeden **önce** yakalandı; bu yüzden
            // doğru eşleşme kontrolü yapılabilir.
            match sag_yol {
                Some(yol) if *yol == sol_kayit.yol => {
                    let sag_kayit = sag_taraf
                        .al()?
                        .ok_or_else(|| Hata::gecersiz_durum("eşleşen sağ kaydı yok"))?;
                    if sag_kayit.bayt == sol_kayit.bayt {
                        // Boyut aynı: fark kaydı üretilmez (fark boş kalabilir).
                        continue;
                    }
                    Degisim::Degisti {
                        yol: sag_kayit.yol.clone(),
                        tur: sag_kayit.tur,
                        onceki_bayt: sol_kayit.bayt,
                        yeni_bayt: sag_kayit.bayt,
                    }
                }
                _ => Degisim::Silindi {
                    yol: sol_kayit.yol.clone(),
                    tur: sol_kayit.tur,
                    son_bayt: sol_kayit.bayt,
                },
            }
        } else {
            let kayit = sag_taraf
                .al()?
                .ok_or_else(|| Hata::gecersiz_durum("sağ tanık beklenen kaydı vermedi"))?;
            Degisim::Eklendi { kayit }
        };

        match &degisim {
            Degisim::Eklendi { .. } => ozet.eklendi += 1,
            Degisim::Silindi { .. } => ozet.silindi += 1,
            Degisim::Degisti { .. } => ozet.degisti += 1,
            Degisim::AdDegisti { .. } => {}
        }
        // Net etki **yalnızca dosya** kayıtlarından hesaplanır: klasör
        // kayıtlarının `bayt` alanı alt ağacın toplamıdır ve katlanırsa ağaç
        // derinliği kadar katla sayım yapılırdı.
        ozet.net_bayt += degisim.bayt_etkisi();
        yaz(cikti, &mut yazici, &degisim)?;
    }

    if let Some(t) = sol_taraf.as_ref() {
        ozet.bozuk_satir += t.bozuk;
    }
    ozet.bozuk_satir += sag_taraf.bozuk;
    yazici.bitir()?;
    Ok(ozet)
}

/// Karşılaştırmada kullanılacak bir anlık görüntü dosyası.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kaynak {
    /// Dosya yolu.
    pub yol: PathBuf,
    /// Dosyanın ilk satırı başlık mı?
    ///
    /// Depo içindeki `.tam.jsonl` dosyaları bir başlık satırıyla başlar; geçici
    /// dosyalar başlıksızdır. Bu bayrak olmazsa başlık, çözümlenemeyen bir
    /// kayıt gibi sayılır ve kullanıcıya **yanlış** bir "bozuk kayıt" uyarısı
    /// gösterilir.
    pub basligi_var: bool,
}

impl Kaynak {
    /// Başlıklı depo dosyası (`.tam.jsonl`) için kaynak.
    pub fn baslikli(yol: impl Into<PathBuf>) -> Self {
        Kaynak {
            yol: yol.into(),
            basligi_var: true,
        }
    }

    /// Başlıksız geçici dosya için kaynak.
    pub fn basliksiz(yol: impl Into<PathBuf>) -> Self {
        Kaynak {
            yol: yol.into(),
            basligi_var: false,
        }
    }
}

fn yaz(cikti: &Path, yazici: &mut AkisYazici, degisim: &Degisim) -> Sonuc<()> {
    let satir = serde_json::to_string(degisim).map_err(|k| Hata::BozukSatir {
        dosya: cikti.to_path_buf(),
        satir: 0,
        ayrinti: k.to_string(),
    })?;
    yazici.yaz(&satir)
}

/// Sıralı bir fark dosyasını belleğe okur ve **bozuk satırları atlayarak sayar**.
///
/// Dönüş değeri `(kayıtlar, atlanan_satir_sayısı)`. Bozuk satır **hata değildir**:
/// raporun kuralı gereği atlanır ve sayılır; çağıran taraf sayıyı kullanıcıya
/// bildirir. Bu, güç kesintisiyle yarım kalan bir kaydın tüm zinciri okunamaz
/// yapmasını önler.
///
/// `basligi_atla` `true` ise ilk satır (anlık görüntü başlığı) yok sayılır;
/// depo içindeki dosyalar başlıklı, geçici dosyalar başlıksızdır.
pub fn farklari_oku(cikti: &Path, basligi_atla: bool) -> Sonuc<(Vec<Degisim>, u64)> {
    let mut okuyucu = SatirOkuyucu::ac(cikti)?;
    if basligi_atla {
        okuyucu.sonraki()?;
    }
    let mut sonuc: Vec<Degisim> = Vec::new();
    let mut atlanan = 0u64;
    while let Some(satir) = okuyucu.sonraki()? {
        if satir.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Degisim>(&satir) {
            Ok(degisim) => sonuc.push(degisim),
            Err(_) => atlanan += 1,
        }
    }
    Ok((sonuc, atlanan))
}

/// Sıralı bir tam kayıt dosyasını okur (testler ve küçük ağaçlar).
///
/// `basligi_atla` için [`farklari_oku`] ile aynı kural geçerlidir.
pub fn tam_kayitlari_oku(dosya: &Path, basligi_atla: bool) -> Sonuc<Vec<TamKayit>> {
    let mut okuyucu = SatirOkuyucu::ac(dosya)?;
    if basligi_atla {
        okuyucu.sonraki()?;
    }
    let mut sonuc: Vec<TamKayit> = Vec::new();
    while let Some(satir) = okuyucu.sonraki()? {
        if satir.trim().is_empty() {
            continue;
        }
        sonuc.push(
            serde_json::from_str::<TamKayit>(&satir).map_err(|k| Hata::BozukSatir {
                dosya: dosya.to_path_buf(),
                satir: okuyucu.satir_no(),
                ayrinti: k.to_string(),
            })?,
        );
    }
    Ok(sonuc)
}

/// Bir farkın iki anlık görüntü arasında ürettiği yol kümesini döndürür.
pub fn etkilenen_yollar(farklar: &[Degisim]) -> Vec<Yol> {
    let mut yollar: Vec<Yol> = Vec::new();
    for fark in farklar {
        for yol in [fark.yol(), fark.eski_yol()] {
            if !yollar.contains(yol) {
                yollar.push(yol.clone());
            }
        }
    }
    yollar.sort();
    yollar.dedup();
    yollar
}

/// İki kayıt arasındaki bayt farkını döndürür.
pub fn bayt_farki(eski: &TamKayit, yeni: &TamKayit) -> i64 {
    yeni.bayt as i64 - eski.bayt as i64
}

/// Fark dosyasının yolunu üretir (depo içinde standart adlandırma).
pub fn fark_dosyasi_yolu(depo: &Path, kimlik: &str) -> PathBuf {
    depo.join("anlik_goruntuler")
        .join(format!("{}.fark.jsonl", kimlik))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
// expect/unwrap test kodunda bilinçlidir: WORKER_CONTRACT.md § 4.2 bunları
// yalnızca testlerde serbest bırakır. Bir testin expect çağrısının
// başarısız olması, o iddianın yanlış olduğunun en net sinyalidir.
mod tests {
    use super::*;
    use crate::kayit::KayitTuru;
    use std::fs;

    fn gecici(etiket: &str) -> PathBuf {
        let yol =
            std::env::temp_dir().join(format!("timefold-delta-{}-{}", etiket, std::process::id()));
        let _ = fs::remove_dir_all(&yol);
        fs::create_dir_all(&yol).expect("geçici dizin");
        yol
    }

    fn kayitlari_yaz(dosya: &Path, kayitlar: &[TamKayit]) {
        let mut yazici = AkisYazici::olustur(dosya).expect("yazıcı");
        for k in kayitlar {
            yazici
                .yaz(&serde_json::to_string(k).expect("seri"))
                .expect("yaz");
        }
        yazici.bitir().expect("bitir");
    }

    fn kayit(yol: &str, bayt: u64) -> TamKayit {
        TamKayit {
            yol: Yol::metin_yap(yol),
            tur: KayitTuru::Dosya,
            bayt,
            degisiklik: 0,
        }
    }

    #[test]
    fn ilk_taramada_her_sey_eklenir() {
        let dizin = gecici("ilk");
        let yeni = dizin.join("yeni.jsonl");
        let cikti = dizin.join("fark.jsonl");
        kayitlari_yaz(&yeni, &[kayit("a", 10), kayit("b", 20)]);
        let ozet = karsilastir(None, &Kaynak::basliksiz(&yeni), &cikti).expect("karşılaştır");
        assert_eq!(ozet.eklendi, 2);
        assert_eq!(ozet.silindi, 0);
        assert_eq!(ozet.net_bayt, 30);
        assert_eq!(farklari_oku(&cikti, false).expect("oku").0.len(), 2);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn silinen_dosya_negatif_etki_uretir() {
        let dizin = gecici("silinen");
        let eski = dizin.join("eski.jsonl");
        let yeni = dizin.join("yeni.jsonl");
        let cikti = dizin.join("fark.jsonl");
        kayitlari_yaz(&eski, &[kayit("a", 10), kayit("b", 20)]);
        kayitlari_yaz(&yeni, &[kayit("a", 10)]);
        let ozet = karsilastir(
            Some(&Kaynak::basliksiz(&eski)),
            &Kaynak::basliksiz(&yeni),
            &cikti,
        )
        .expect("karşılaştır");
        assert_eq!(ozet.silindi, 1);
        assert_eq!(ozet.degisti, 0);
        assert_eq!(ozet.net_bayt, -20);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn degisen_dosya_fark_uretir() {
        let dizin = gecici("degisen");
        let eski = dizin.join("eski.jsonl");
        let yeni = dizin.join("yeni.jsonl");
        let cikti = dizin.join("fark.jsonl");
        kayitlari_yaz(&eski, &[kayit("a", 10)]);
        kayitlari_yaz(&yeni, &[kayit("a", 25)]);
        let ozet = karsilastir(
            Some(&Kaynak::basliksiz(&eski)),
            &Kaynak::basliksiz(&yeni),
            &cikti,
        )
        .expect("karşılaştır");
        assert_eq!(ozet.degisti, 1);
        assert_eq!(ozet.net_bayt, 15);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn ayni_boyut_farkli_icerik_degisiklik_uretmez() {
        let dizin = gecici("ayni");
        let eski = dizin.join("eski.jsonl");
        let yeni = dizin.join("yeni.jsonl");
        let cikti = dizin.join("fark.jsonl");
        kayitlari_yaz(&eski, &[kayit("a", 10)]);
        kayitlari_yaz(&yeni, &[kayit("a", 10)]);
        let ozet = karsilastir(
            Some(&Kaynak::basliksiz(&eski)),
            &Kaynak::basliksiz(&yeni),
            &cikti,
        )
        .expect("karşılaştır");
        assert!(ozet.bos_mu());
        assert_eq!(ozet.net_bayt, 0);
        assert!(farklari_oku(&cikti, false).expect("oku").0.is_empty());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn karisik_klasor_agaci_dogru_siniflandiriliyor() {
        let dizin = gecici("klasor");
        let eski = dizin.join("eski.jsonl");
        let yeni = dizin.join("yeni.jsonl");
        let cikti = dizin.join("fark.jsonl");
        kayitlari_yaz(
            &eski,
            &[
                kayit("", 30),
                kayit("a", 20),
                kayit("a/x", 20),
                kayit("b", 10),
            ],
        );
        kayitlari_yaz(&yeni, &[kayit("", 35), kayit("a", 25), kayit("a/x", 25)]);
        let ozet = karsilastir(
            Some(&Kaynak::basliksiz(&eski)),
            &Kaynak::basliksiz(&yeni),
            &cikti,
        )
        .expect("karşılaştır");
        // b silindi; "", "a" ve "a/x" değişti.
        assert_eq!(ozet.silindi, 1);
        assert_eq!(ozet.degisti, 3);
        assert_eq!(ozet.eklendi, 0);
        // b silindi (-10); "", "a" ve "a/x" 5'er bayt büyüdü (+15).
        let beklenen: i64 = -10 + 5 * 3;
        assert_eq!(ozet.net_bayt, beklenen);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn eklenen_ve_silinen_birlikte_yaziliyor() {
        let dizin = gecici("karisik");
        let eski = dizin.join("eski.jsonl");
        let yeni = dizin.join("yeni.jsonl");
        let cikti = dizin.join("fark.jsonl");
        kayitlari_yaz(&eski, &[kayit("a", 1), kayit("b", 2), kayit("c", 3)]);
        kayitlari_yaz(&yeni, &[kayit("b", 2), kayit("c", 9), kayit("d", 4)]);
        let ozet = karsilastir(
            Some(&Kaynak::basliksiz(&eski)),
            &Kaynak::basliksiz(&yeni),
            &cikti,
        )
        .expect("karşılaştır");
        assert_eq!(ozet.eklendi, 1);
        assert_eq!(ozet.silindi, 1);
        assert_eq!(ozet.degisti, 1);
        let farklar = farklari_oku(&cikti, false).expect("oku").0;
        let yollar: Vec<String> = farklar.iter().map(|f| f.yol().metin().to_owned()).collect();
        assert!(yollar.contains(&"a".to_owned()));
        assert!(yollar.contains(&"d".to_owned()));
        assert!(yollar.contains(&"c".to_owned()));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn bozuk_satir_atlanip_sayiliyor() {
        let dizin = gecici("bozuk");
        let eski = dizin.join("eski.jsonl");
        let yeni = dizin.join("yeni.jsonl");
        let cikti = dizin.join("fark.jsonl");
        {
            let mut y = AkisYazici::olustur(&eski).expect("yaz");
            y.yaz("bozuk satir").expect("yaz");
            y.yaz(&serde_json::to_string(&kayit("a", 1)).expect("seri"))
                .expect("yaz");
            y.bitir().expect("bitir");
        }
        kayitlari_yaz(&yeni, &[kayit("a", 1)]);
        let ozet = karsilastir(
            Some(&Kaynak::basliksiz(&eski)),
            &Kaynak::basliksiz(&yeni),
            &cikti,
        )
        .expect("karşılaştır");
        assert!(ozet.bos_mu(), "bozuk satır yanlış eşleşme üretmemeli");
        assert_eq!(ozet.bozuk_satir, 1);
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn etkilenen_yollar_birlestirilip_siraliyor() {
        let farklar = vec![
            Degisim::Eklendi {
                kayit: kayit("b", 1),
            },
            Degisim::AdDegisti {
                eski_yol: Yol::metin_yap("a"),
                yeni_yol: Yol::metin_yap("c"),
                bayt: 1,
            },
        ];
        let yollar = etkilenen_yollar(&farklar);
        let liste: Vec<&str> = yollar.iter().map(|y| y.metin()).collect();
        assert_eq!(liste, vec!["a", "b", "c"]);
    }

    #[test]
    fn bayt_farki_isaretli_hesaplaniyor() {
        let eski = kayit("a", 10);
        let yeni = kayit("a", 4);
        assert_eq!(bayt_farki(&eski, &yeni), -6);
    }

    #[test]
    fn fark_dosyasi_yolu_standart_ad_kullanir() {
        let yol = fark_dosyasi_yolu(Path::new("/depo"), "T0003");
        assert!(yol.to_string_lossy().ends_with("T0003.fark.jsonl"));
    }
}
