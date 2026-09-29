//! Satır akışı okuma/yazma, harici sıralama ve akış birleştirme.
//!
//! Bellek bütçesinin anahtar kuralı: **hiçbir adımda tüm ağaç belleğe alınmaz.**
//! Anlık görüntü dosyaları JSONL satırlarıdır ve her zaman *sıralı* saklanır
//! (yol bileşen bileşen sıralı). Bu modül üç işi sağlar:
//!
//! 1. Sabit tamponlu satır okuma/yazma ([`SatirOkuyucu`], [`AkisYazici`]).
//! 2. Sırasız girdiyi **yoluna göre sıralı** hâle getiren harici (dış) sıralama
//!    ([`Sirala`]): girdi `N` satırlık sınırlı tamponlarla "koşu" (run) dosyalarına
//!    yazılır, sonra koşular k-yollu birleştirme ile birleştirilir.
//! 3. İki sıralı akışı **akış hâlinde** birleştirme ([`Birlestir`]), yani
//!    fark hesabı için iki pencereyi belleğe almadan karşılaştırmak.
//!
//! Sıralama gereksiniminin kaynağı `delta` modülüdür: iki anlık görüntü yalnızca
//! ikisi de sıralıysa tek geçişte karşılaştırılabilir.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::hata::{Hata, Sonuc};
use crate::yol::Yol;

/// Okuma tamponunun varsayılan boyutu (bayt).
///
/// 64 KiB, `read` çağrılarının sayısını düşürürken de bir dosya tanıtıcısının
/// tamponunu asgariye indirir.
pub const OKUMA_TAMPONU: usize = 64 * 1024;

/// Yazma tamponunun varsayılan boyutu (bayt).
pub const YAZMA_TAMPONU: usize = 64 * 1024;

/// Harici sıralamada tek seferde belleğe alınan satır sayısı üst sınırı.
///
/// Bu değer bellek bütçesinin en büyük tek belirleyicisidir: 65 536 satır × ~120 bayt
/// ≈ 8 MB. Raporun "tarama kayıt dizisi" ve "yol dizesi havuzu" kalemleriyle
/// karşılaştırılabilir bir bütçedir.
pub const SIRALAMA_TAMPONU: usize = 65_536;

/// Bir dosyadan satır satır okuyan akış okuyucu.
///
/// Satırlar sonundaki `\n` ve `\r\n` ayraçları atılır. Dosya açılamazsa
/// [`Hata::Io`] döner; okuma sırasında hata olursa satır numarası eklenerek
/// bildirilir.
pub struct SatirOkuyucu {
    dosya: PathBuf,
    okuyucu: BufReader<File>,
    satir_no: u64,
    satir_ici: String,
    son: bool,
}

impl SatirOkuyucu {
    /// Verilen dosyayı açarak satır okuyucu üretir.
    pub fn ac(dosya: &Path) -> Sonuc<Self> {
        let dosya_ac = File::open(dosya).map_err(|kaynak| Hata::io("akış aç", dosya, kaynak))?;
        Ok(SatirOkuyucu {
            dosya: dosya.to_path_buf(),
            okuyucu: BufReader::with_capacity(OKUMA_TAMPONU, dosya_ac),
            satir_no: 0,
            satir_ici: String::new(),
            son: false,
        })
    }

    /// Okunan dosyanın yolunu döndürür.
    pub fn dosya(&self) -> &Path {
        &self.dosya
    }

    /// Şu ana kadar okunan satır sayısını döndürür.
    pub fn satir_no(&self) -> u64 {
        self.satir_no
    }

    /// Sonraki satırı döndürür; dosya sonundaysa `None`.
    ///
    /// Satır sonundaki `\n` ve `\r\n` ayraçları atılır. Dosya açılamazsa
    /// [`Hata::Io`] döner; okuma sırasında hata olursa satır numarası eklenerek
    /// bildirilir.
    pub fn sonraki(&mut self) -> Sonuc<Option<String>> {
        // `read_line` tamponu kendisi ilerletir; `fill_buf` **ilerletmez** ve
        // elle `consume` çağrılmazsa sonsuz döngüye girer.
        self.satir_ici.clear();
        let okunan = self
            .okuyucu
            .read_line(&mut self.satir_ici)
            .map_err(|kaynak| Hata::io("akış oku", &self.dosya, kaynak))?;
        if okunan == 0 {
            self.son = true;
            return Ok(None);
        }
        self.satir_no += 1;
        while self.satir_ici.ends_with('\n') || self.satir_ici.ends_with('\r') {
            self.satir_ici.pop();
        }
        Ok(Some(std::mem::take(&mut self.satir_ici)))
    }
}

/// Dosyaya satır satır yazan akış yazıcı.
///
/// Yazma tamponu sabittir; her `yaz` çağrısı tamponu doldurur ve gerekirse
/// diske yazar. Dosya, yazıcı düşürüldüğünde kapatılır ve `bitir` ile
/// açıkça boşaltılabilir.
pub struct AkisYazici {
    dosya: PathBuf,
    yazici: BufWriter<File>,
    yazilan: u64,
}

impl AkisYazici {
    /// Verilen dosyayı **üzerine yazarak** açarak yazıcı üretir.
    pub fn olustur(dosya: &Path) -> Sonuc<Self> {
        if let Some(usta) = dosya.parent() {
            std::fs::create_dir_all(usta)
                .map_err(|kaynak| Hata::io("dizin oluştur", usta, kaynak))?;
        }
        let dosya_ac =
            File::create(dosya).map_err(|kaynak| Hata::io("dosya oluştur", dosya, kaynak))?;
        Ok(AkisYazici {
            dosya: dosya.to_path_buf(),
            yazici: BufWriter::with_capacity(YAZMA_TAMPONU, dosya_ac),
            yazilan: 0,
        })
    }

    /// Yazılan satır sayısını döndürür.
    pub fn yazilan(&self) -> u64 {
        self.yazilan
    }

    /// Yazılan dosyanın yolunu döndürür.
    pub fn dosya(&self) -> &Path {
        &self.dosya
    }

    /// Tek bir satır yazar; satır sonuna `\n` eklenir.
    pub fn yaz(&mut self, satir: &str) -> Sonuc<()> {
        self.yazici
            .write_all(satir.as_bytes())
            .and_then(|_| self.yazici.write_all(b"\n"))
            .map_err(|kaynak| Hata::io("dosyaya yaz", &self.dosya, kaynak))?;
        self.yazilan += 1;
        Ok(())
    }

    /// Tamponu diske yazar ve dosyayı kapatır.
    pub fn bitir(mut self) -> Sonuc<u64> {
        self.yazici
            .flush()
            .map_err(|kaynak| Hata::io("arşiv yaz", &self.dosya, kaynak))?;
        Ok(self.yazilan)
    }
}

impl Drop for AkisYazici {
    fn drop(&mut self) {
        // Dosya düşürme sırasında `bitir` çağrılmamışsa tampon sessizce kaybolur;
        // `bitir` çağrıldığında ise `flush` zaten yapılmıştır ve burada ikinci kez
        // `flush` çağırmak `write` sonrası hata döndürmez.
        let _ = self.yazici.flush();
    }
}

/// Sıralanacak satır: kanonik yol + satırın kendisi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiraliSatir {
    /// Sıralama anahtarı.
    pub yol: Yol,
    /// Aktarılacak ham satır metni.
    pub satir: String,
}

/// Sırasız satırları sıralı hâle getiren harici sıralayıcı.
///
/// Girdi akışı bir kez okunur. Tampon dolduğunda sıralanıp geçici bir "koşu"
///
/// dosyasına yazılır; girdi bitince koşular k-yollu birleştirme ile tek bir
/// sıralı dosyada birleştirilir. Bellek tavanı `koşu_sayısı × 1 satır` +
/// `tampon` boyutudur; tüm girdi hiçbir zaman belleğe alınmaz.
pub struct Sirala {
    gecici_dizin: PathBuf,
    kosular: Vec<PathBuf>,
    tampon: Vec<SiraliSatir>,
    tampon_siniri: usize,
    sayac: u64,
    girdi_sayisi: u64,
}

impl Sirala {
    /// Verilen geçici dizinde yeni sıralayıcı üretir.
    pub fn yeni(gecici_dizin: &Path) -> Self {
        Sirala {
            gecici_dizin: gecici_dizin.to_path_buf(),
            kosular: Vec::new(),
            tampon: Vec::with_capacity(SIRALAMA_TAMPONU.min(1024)),
            tampon_siniri: SIRALAMA_TAMPONU,
            sayac: 0,
            girdi_sayisi: 0,
        }
    }

    /// Tampon eşiğini değiştirir; küçük değerler testlerde ve düşük bellek
    /// senaryolarında kullanılır.
    pub fn tampon_ayarla(&mut self, sinir: usize) {
        self.tampon_siniri = sinir.max(1);
    }

    /// Alınan satır sayısını döndürür.
    pub fn girdi_sayisi(&self) -> u64 {
        self.girdi_sayisi
    }

    /// Üretilen geçici koşu dosyası sayısını döndürür.
    pub fn kosu_sayisi(&self) -> usize {
        self.kosular.len()
    }

    /// Bir satır ekler.
    pub fn ekle(&mut self, satir: SiraliSatir) -> Sonuc<()> {
        self.girdi_sayisi += 1;
        self.tampon.push(satir);
        if self.tampon.len() >= self.tampon_siniri {
            self.tamponu_dok()?;
        }
        Ok(())
    }

    fn tamponu_dok(&mut self) -> Sonuc<()> {
        if self.tampon.is_empty() {
            return Ok(());
        }
        self.tampon.sort_by(|a, b| a.yol.cmp(&b.yol));
        let kosu_yolu = self
            .gecici_dizin
            .join(format!("kosu-{:06}.jsonl", self.kosular.len()));
        let mut yazici = AkisYazici::olustur(&kosu_yolu)?;
        for satir in &self.tampon {
            yazici.yaz(&satir.satir)?;
        }
        yazici.bitir()?;
        self.kosular.push(kosu_yolu);
        self.tampon.clear();
        self.sayac += 1;
        Ok(())
    }

    /// Sıralamayı bitirir ve sıralı çıktı dosyasının yolunu döndürür.
    ///
    /// Boş girdide bile (yok edilecek) bir çıktı dosyası üretilir; çağıran taraf
    /// dosyanın varlığını garanti edebilir.
    pub fn bitir(mut self, cikti: &Path) -> Sonuc<PathBuf> {
        self.tamponu_dok()?;
        let mut birlestirici = Birlestir::ac(self.kosular.clone())?;
        let mut yazici = AkisYazici::olustur(cikti)?;
        while let Some(satir) = birlestirici.sonraki()? {
            yazici.yaz(&satir.satir)?;
        }
        yazici.bitir()?;
        for kosu in &self.kosular {
            let _ = std::fs::remove_file(kosu);
        }
        Ok(cikti.to_path_buf())
    }
}

impl Drop for Sirala {
    fn drop(&mut self) {
        // `bitir` çağrılmamışsa koşu dosyaları diskte kalmasın diye temizlenir.
        for kosu in &self.kosular {
            let _ = std::fs::remove_file(kosu);
        }
    }
}

/// Sıralı akışların k-yollu birleştiricisi.
///
/// Her kaynak için bellekte yalnızca **bir** satır tutulur (`konum` alanı), böylece
/// birleştirme `k` kaynak için `O(k)` bellekle çalışır. Kısayol seçimi doğrusal
/// taramayla yapılır; kaynak sayısı ikiye yakın olduğu için (`delta` hesabı,
/// zaman çizelgesi) yığından daha hızlıdır ve `O(k²)` tarama maliyeti burada
/// ihmal edilebilir.
pub struct Birlestir {
    kaynaklar: Vec<Kaynak>,
}

struct Kaynak {
    okuyucu: SatirOkuyucu,
    mevcut: Option<SiraliSatir>,
}

impl Birlestir {
    /// Verilen sıralı dosyaları açarak birleştiriciyi üretir.
    pub fn ac(yollar: Vec<PathBuf>) -> Sonuc<Self> {
        let mut kaynaklar = Vec::with_capacity(yollar.len());
        for yol in yollar {
            let mut okuyucu = SatirOkuyucu::ac(&yol)?;
            let mevcut = Kaynak::sonraki_satir(&mut okuyucu)?;
            kaynaklar.push(Kaynak { okuyucu, mevcut });
        }
        Ok(Birlestir { kaynaklar })
    }

    /// Birleştiricinin açtığı kaynak sayısını döndürür.
    pub fn kaynak_sayisi(&self) -> usize {
        self.kaynaklar.len()
    }

    /// Sıradaki en küçük satırı yoluna göre döndürür.
    pub fn sonraki(&mut self) -> Sonuc<Option<SiraliSatir>> {
        let mut en_kucuk: Option<usize> = None;
        for (i, kaynak) in self.kaynaklar.iter().enumerate() {
            let Some(satir_yol) = kaynak.mevcut.as_ref().map(|s| &s.yol) else {
                continue;
            };
            let daha_kucuk = match en_kucuk {
                None => true,
                Some(mevcut) => {
                    // `mevcut` indeksi bu turda henüz güncellenmediği için
                    // mevcut kaynağın yolu `self.kaynaklar[mevcut]`'ten okunur.
                    match self.kaynaklar[mevcut].mevcut.as_ref() {
                        Some(mevcut_satir) => satir_yol < &mevcut_satir.yol,
                        None => true,
                    }
                }
            };
            if daha_kucuk {
                en_kucuk = Some(i);
            }
        }
        match en_kucuk {
            None => Ok(None),
            Some(i) => {
                let satir = self.kaynaklar[i]
                    .mevcut
                    .take()
                    .ok_or_else(|| Hata::gecersiz_durum("birleştirici kaynağı boş"))?;
                self.kaynaklar[i].mevcut = Kaynak::sonraki_satir(&mut self.kaynaklar[i].okuyucu)?;
                Ok(Some(satir))
            }
        }
    }
}

impl Kaynak {
    fn sonraki_satir(okuyucu: &mut SatirOkuyucu) -> Sonuc<Option<SiraliSatir>> {
        loop {
            match okuyucu.sonraki()? {
                None => return Ok(None),
                Some(satir) => {
                    if satir.trim().is_empty() {
                        continue;
                    }
                    let yol = satirin_yolunu_coz(&satir)?;
                    return Ok(Some(SiraliSatir { yol, satir }));
                }
            }
        }
    }
}

/// JSON satırından sıralama anahtarı olan yolu çözer.
///
/// Satırın şeması ne olursa olsun (`TamKayit`, `Degisim`, başlık) yol alanı
/// bulunur. Başlık satırında yol yoktur ve `None` döner; başlık sıralamaya girmez.
pub fn satirin_yolunu_coz(satir: &str) -> Sonuc<Yol> {
    let deger: serde_json::Value =
        serde_json::from_str(satir).map_err(|kaynak| Hata::BozukSatir {
            dosya: PathBuf::from("<bellek>"),
            satir: 0,
            ayrinti: format!("JSON çözümlenemedi: {}", kaynak),
        })?;
    let nesne = deger.as_object().ok_or_else(|| Hata::BozukSatir {
        dosya: PathBuf::from("<bellek>"),
        satir: 0,
        ayrinti: "satır JSON nesnesi değil".to_owned(),
    })?;
    let yol_metin = nesne
        .get("yol")
        .and_then(|deger| deger.as_str())
        .or_else(|| {
            nesne
                .get("kayit")
                .and_then(|kayit| kayit.get("yol"))
                .and_then(|deger| deger.as_str())
        })
        .or_else(|| nesne.get("eski_yol").and_then(|deger| deger.as_str()));
    match yol_metin {
        Some(metin) => Ok(Yol::metin_yap(metin)),
        None => Ok(Yol::kok()),
    }
}

/// Bir dosyanın bayt cinsinden boyutunu döndürür; dosya yoksa `0`.
pub fn dosya_boyutu(yol: &Path) -> u64 {
    std::fs::metadata(yol).map(|m| m.len()).unwrap_or(0)
}

/// Bir dizindeki dosya sayısını (özyinelemeli) döndürür.
///
/// Depo büyümesini raporlamak ve "kaç bayt kazanıldı" sorusunu yanıtlamak için
/// kullanılır. Hata durumunda 0 döner: sayım başarısız olursa "kaç dosya
/// silindi" sorusu boşluğu doldurmaktan iyidir.
pub fn dizin_icerigi_say(dizin: &Path) -> u64 {
    let mut sayac = 0u64;
    let Ok(girdiler) = std::fs::read_dir(dizin) else {
        return 0;
    };
    for giris in girdiler.flatten() {
        let yol = giris.path();
        match giris.file_type() {
            Ok(tur) if tur.is_dir() => sayac += dizin_icerigi_say(&yol),
            Ok(_) => sayac += 1,
            Err(_) => {}
        }
    }
    sayac
}

/// Bir dosyanın tamamını bayt olarak okur (küçük dosyalar için).
pub fn dosya_baytlarini_oku(yol: &Path) -> Sonuc<Vec<u8>> {
    let mut dosya = File::open(yol).map_err(|kaynak| Hata::io("dosya aç", yol, kaynak))?;
    let mut veri = Vec::new();
    dosya
        .read_to_end(&mut veri)
        .map_err(|kaynak| Hata::io("dosya oku", yol, kaynak))?;
    Ok(veri)
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
            std::env::temp_dir().join(format!("timefold-akis-{}-{}", etiket, std::process::id()));
        let _ = std::fs::remove_dir_all(&yol);
        std::fs::create_dir_all(&yol).expect("geçici dizin oluşturulabilir");
        yol
    }

    #[test]
    fn satir_okuyucu_satirlari_ayirip_donduruyor() {
        let dizin = gecici("okuyucu");
        let dosya = dizin.join("a.jsonl");
        {
            let mut y = AkisYazici::olustur(&dosya).expect("yazıcı");
            y.yaz("bir").expect("yaz");
            y.yaz("iki").expect("yaz");
            y.yaz("üç").expect("yaz");
            y.bitir().expect("bitir");
        }
        let mut o = SatirOkuyucu::ac(&dosya).expect("okuyucu");
        assert_eq!(o.sonraki().expect("okuma").as_deref(), Some("bir"));
        assert_eq!(o.sonraki().expect("okuma").as_deref(), Some("iki"));
        assert_eq!(o.sonraki().expect("okuma").as_deref(), Some("üç"));
        assert_eq!(o.sonraki().expect("okuma"), None);
        assert_eq!(o.satir_no(), 3);
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn son_satir_sonlandirici_yoksa_da_okunuyor() {
        let dizin = gecici("sonsatir");
        let dosya = dizin.join("a.jsonl");
        std::fs::write(&dosya, "bir\niki").expect("yaz");
        let mut o = SatirOkuyucu::ac(&dosya).expect("okuyucu");
        assert_eq!(o.sonraki().expect("okuma").as_deref(), Some("bir"));
        assert_eq!(o.sonraki().expect("okuma").as_deref(), Some("iki"));
        assert_eq!(o.sonraki().expect("okuma"), None);
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn windows_satir_sonu_temizleniyor() {
        let dizin = gecici("crlf");
        let dosya = dizin.join("a.jsonl");
        std::fs::write(&dosya, "bir\r\niki\r\n").expect("yaz");
        let mut o = SatirOkuyucu::ac(&dosya).expect("okuyucu");
        assert_eq!(o.sonraki().expect("okuma").as_deref(), Some("bir"));
        assert_eq!(o.sonraki().expect("okuma").as_deref(), Some("iki"));
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn okunmayan_dosya_hata_donduruyor() {
        let sonuc = SatirOkuyucu::ac(Path::new("C:/yok/olmayan/dosya.jsonl"));
        assert!(sonuc.is_err());
    }

    #[test]
    fn harici_siralama_cok_tamponlu_dogru_siraliyor() {
        let dizin = gecici("sirala");
        let mut s = Sirala::yeni(&dizin);
        s.tampon_ayarla(2);
        // Akış JSONL satırlarını bekler; sıralama anahtarı `yol` alanından okunur.
        let girdiler = ["c/3", "a/1", "b/2", "a/0", "c/0"];
        for g in girdiler {
            s.ekle(SiraliSatir {
                yol: Yol::metin_yap(g),
                satir: format!(r#"{{"yol":"{}"}}"#, g),
            })
            .expect("ekleme");
        }
        let cikti = dizin.join("cikti.jsonl");
        s.bitir(&cikti).expect("bitir");

        let mut o = SatirOkuyucu::ac(&cikti).expect("okuyucu");
        let mut hepsi = Vec::new();
        while let Some(satir) = o.sonraki().expect("okuma") {
            hepsi.push(satir);
        }
        assert_eq!(
            hepsi,
            vec![
                r#"{"yol":"a/0"}"#,
                r#"{"yol":"a/1"}"#,
                r#"{"yol":"b/2"}"#,
                r#"{"yol":"c/0"}"#,
                r#"{"yol":"c/3"}"#
            ]
        );
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn harici_siralama_bos_girdide_da_dosya_uretir() {
        let dizin = gecici("sirala-bos");
        let s = Sirala::yeni(&dizin);
        let cikti = dizin.join("cikti.jsonl");
        s.bitir(&cikti).expect("bitir");
        assert!(cikti.exists());
        assert_eq!(dosya_boyutu(&cikti), 0);
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn birlestirici_iki_kaynagi_sirayla_birlestiriyor() {
        let dizin = gecici("birlestir");
        let a = dizin.join("a.jsonl");
        let b = dizin.join("b.jsonl");
        {
            let mut ya = AkisYazici::olustur(&a).expect("yazıcı");
            ya.yaz(r#"{"yol":"a/1"}"#).expect("yaz");
            ya.yaz(r#"{"yol":"a/3"}"#).expect("yaz");
            ya.bitir().expect("bitir");
        }
        {
            let mut yb = AkisYazici::olustur(&b).expect("yazıcı");
            yb.yaz(r#"{"yol":"a/0"}"#).expect("yaz");
            yb.yaz(r#"{"yol":"a/2"}"#).expect("yaz");
            yb.bitir().expect("bitir");
        }
        let mut bir = Birlestir::ac(vec![a, b]).expect("birleştirici");
        assert_eq!(bir.kaynak_sayisi(), 2);
        let mut sira = Vec::new();
        while let Some(s) = bir.sonraki().expect("birleştir") {
            sira.push(s.satir);
        }
        assert_eq!(sira[0], r#"{"yol":"a/0"}"#);
        assert_eq!(sira[3], r#"{"yol":"a/3"}"#);
        assert_eq!(sira.len(), 4);
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn satirin_yolu_kayit_ve_degisim_semasindan_okunuyor() {
        assert_eq!(
            satirin_yolunu_coz(r#"{"yol":"a/b","bayt":1}"#)
                .expect("coz")
                .metin(),
            "a/b"
        );
        assert_eq!(
            satirin_yolunu_coz(r#"{"tur":"eklendi","kayit":{"yol":"c/d"}}"#)
                .expect("coz")
                .metin(),
            "c/d"
        );
        assert_eq!(
            satirin_yolunu_coz(r#"{"tur":"ad_degisti","eski_yol":"e/f","yeni_yol":"e/g"}"#)
                .expect("coz")
                .metin(),
            "e/f"
        );
        // Başlık satırında yol yok; sıralama anahtarı kök olur (sıralamaya girmez).
        assert!(satirin_yolunu_coz(r#"{"sema":"timefold/1"}"#)
            .expect("coz")
            .kok_mu());
    }

    #[test]
    fn bozuk_json_satiri_hata_donduruyor() {
        let sonuc = satirin_yolunu_coz("{bozuk");
        assert!(sonuc.is_err());
    }

    #[test]
    fn dizin_icerigi_say_derin_agaci_sayiyor() {
        let dizin = gecici("sayim");
        std::fs::create_dir_all(dizin.join("a/b")).expect("dizin");
        std::fs::write(dizin.join("a/b/1.txt"), "x").expect("yaz");
        std::fs::write(dizin.join("a/2.txt"), "x").expect("yaz");
        std::fs::write(dizin.join("3.txt"), "x").expect("yaz");
        assert_eq!(dizin_icerigi_say(&dizin), 3);
        assert_eq!(dizin_icerigi_say(&dizin.join("yok")), 0);
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn dosya_baytlari_okunuyor() {
        let dizin = gecici("oku");
        let dosya = dizin.join("a.bin");
        std::fs::write(&dosya, b"0123456789").expect("yaz");
        let veri = dosya_baytlarini_oku(&dosya).expect("oku");
        assert_eq!(veri, b"0123456789".to_vec());
        let _ = std::fs::remove_dir_all(&dizin);
    }
}
