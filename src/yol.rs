//! Göreli yol kimliği ve dışlama kuralları.
//!
//! Anlık görüntü deposu **mutlak yol saklamaz**; tarama köküne göreli, `/` ile
//! ayrılmış bir kimlik saklar. Böylece depo başka bir makineye kopyalandığında
//! istatistik sorgulanabilir (raporun R7 riski ve "yol bağımsızlığı" taşınabilirlik
//! gereksinimi).
//!
//! Bu modül dosya sistemine yazmaz ve dosya sisteminden okumaz; yalnızca
//! `std::path` değerlerini kanonik metne çevirir ve metin üzerinde çalışır.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Tarama köküne göreli, `/` ile ayrılmış kanonik yol.
///
/// Boş dize kökü temsil eder. Karşılaştırma ve sıralama **bileşen bileşen**
/// yapılır: `a/b` ile `a.txt` karşılaştırıldığında ilk bileşenler (`a`, `a.txt`)
/// dikkate alınır, ham metin (`/` = 0x2F, `.` = 0x2E) değil. Bu sıralama, adı
/// geçen derin-önce-düzenli gezinme (DFS) çıktısıyla birebir aynı olduğundan
/// akış tabanlı birleştirme mümkün olur ve ayrı bir sıralama adımı gerekmez.
///
/// JSON'da **düz metin** olarak serileştirilir (`"a/b.txt"`); iç içe yapı
/// taşımak, anlık görüntü dosyalarının okunabilirliğini bozardı.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Yol(String);

impl PartialOrd for Yol {
    fn partial_cmp(&self, diger: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(diger))
    }
}

impl Ord for Yol {
    /// Bileşen bileşen sıralama.
    ///
    /// Ham metin sıralaması kullanılmaz: `a.txt` ve `a/b` yollarında `a.txt`
    /// önce gelmelidir, çünkü ilk bileşenleri `a.txt` ve `a` karşılaştırıldığında
    /// `a` daha küçüktür. Ham metinde ise `/` (0x2F) `.`'den (0x2E) büyük
    /// olduğu için sıra tersine döner ve gezinme çıktısıyla uyuşmazdı.
    fn cmp(&self, diger: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let mut sol = self.0.split('/');
        let mut sag = diger.0.split('/');
        loop {
            match (sol.next(), sag.next()) {
                (None, None) => return Ordering::Equal,
                // Bir yol diğerinin ön ekiyse kısa olan önce gelir: bu, bir
                // klasörün kendisinin altından önce sıralanmasını sağlar.
                (None, Some(_)) => return Ordering::Less,
                (Some(_), None) => return Ordering::Greater,
                (Some(a), Some(b)) => match a.cmp(b) {
                    Ordering::Equal => continue,
                    diger => return diger,
                },
            }
        }
    }
}

impl Yol {
    /// Kök yolu temsil eden boş yolu üretir.
    pub fn kok() -> Self {
        Yol(String::new())
    }

    /// Zaten kanonik metin olarak yazılmış yolu üretir.
    ///
    /// Girdi `\` ve `/` işaretlerinin karışımını ve `//` tekrarını içeriyorsa
    /// bunlar `/` olarak sadeleştirilir; baştaki ve sondaki `/` atılır.
    pub fn metin_yap(girdi: &str) -> Self {
        let mut parcalar: Vec<&str> = Vec::new();
        for parca in girdi.split(['/', '\\']) {
            if parca.is_empty() || parca == "." {
                continue;
            }
            parcalar.push(parca);
        }
        Yol(parcalar.join("/"))
    }

    /// Kanonik metin içeriğini döndürür.
    pub fn metin(&self) -> &str {
        &self.0
    }

    /// Kanonik metin tüketerek döndürür.
    pub fn metine_alis(self) -> String {
        self.0
    }

    /// Yolun kök olup olmadığını söyler.
    pub fn kok_mu(&self) -> bool {
        self.0.is_empty()
    }

    /// Bileşen sayısını döndürür; kök için `0`.
    pub fn derinlik(&self) -> usize {
        if self.0.is_empty() {
            0
        } else {
            self.0.split('/').count()
        }
    }

    /// Son bileşeni döndürür; kökte `None`.
    pub fn son_bilesen(&self) -> Option<&str> {
        self.0.rsplit('/').next().filter(|p| !p.is_empty())
    }

    /// Bu yolun belirtilen üst yolün altında olup olmadığını söyler.
    ///
    /// Bir yol kendisiyle de "altındadır"; bu, tek bir kaydın alt ağacı toplamını
    /// sorgularken `altinda()` kullanılabilmesi için böyledir.
    pub fn altinda(&self, ust: &Yol) -> bool {
        if ust.0.is_empty() {
            return true;
        }
        if self.0 == ust.0 {
            return true;
        }
        self.0.len() > ust.0.len()
            && self.0.starts_with(ust.0.as_str())
            && self.0.as_bytes()[ust.0.len()] == b'/'
    }

    /// Yeni bir bileşen ekleyerek alt yolu üretir.
    pub fn altina(&self, bileşen: &str) -> Self {
        if self.0.is_empty() {
            Yol::metin_yap(bileşen)
        } else if bileşen.is_empty() {
            self.clone()
        } else {
            Yol(format!("{}/{}", self.0, bileşen))
        }
    }

    /// Yolun ilk `n` bileşeninden oluşan üst yolu döndürür.
    ///
    /// `n` derinliği aşarsa yolun kendisi döndürülür; bu, ısı haritasında
    /// "hangi üst klasöre yuvarlanacak" sorusunun taşmasını önler.
    pub fn ust_n(&self, n: usize) -> Self {
        if n == 0 {
            return Yol::kok();
        }
        let parcalar: Vec<&str> = self.0.split('/').take(n).collect();
        Yol(parcalar.join("/"))
    }

    /// `std::path::Path` değerini kanonik göreli yola çevirir.
    ///
    /// Dosya adı geçerli UTF-8 değilse `U+FFFD` ile değiştirilir ve
    /// `kayip` bayrağı `true` olur; çağıran taraf bu kaybı raporlar.
    pub fn yoldan(kok: &Path, yol: &Path) -> (Self, bool) {
        let mut kayip = false;
        let mut parcalar: Vec<String> = Vec::new();
        match yol.strip_prefix(kok) {
            Ok(gorece) => {
                for bileşen in gorece.components() {
                    let ad = match bileşen {
                        std::path::Component::Normal(ad) => ad.to_str().map(str::to_owned),
                        std::path::Component::CurDir => continue,
                        _ => None,
                    };
                    match ad {
                        Some(ad) => parcalar.push(ad),
                        None => {
                            kayip = true;
                            parcalar.push("\u{FFFD}".to_owned());
                        }
                    }
                }
            }
            Err(_) => {
                // Kökün dışına çıkan yol: mutlak gösterimi kanonikleştirip işaretle.
                kayip = true;
                for bileşen in yol.components() {
                    if let std::path::Component::Normal(ad) = bileşen {
                        parcalar.push(ad.to_string_lossy().into_owned());
                    }
                }
            }
        }
        (Yol(parcalar.join("/")), kayip)
    }

    /// Kanonik yolu `std::path::PathBuf` değerine çevirir.
    pub fn yola(&self, kok: &Path) -> PathBuf {
        let mut sonuc = kok.to_path_buf();
        if !self.0.is_empty() {
            for parca in self.0.split('/') {
                sonuc.push(parca);
            }
        }
        sonuc
    }
}

impl std::fmt::Display for Yol {
    fn fmt(&self, bicik: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        bicik.write_str(&self.0)
    }
}

impl From<&str> for Yol {
    fn from(deger: &str) -> Self {
        Yol::metin_yap(deger)
    }
}

/// Dışlanacak ad desenlerinin listesi ve eşleştirici.
///
/// Desenler jokerli: `*` tek bir ad bileşeni içinde herhangi bir diziyle eşleşir,
/// `?` tek karakterle eşleşir, `**` sınır tanımadan eşleşir. Büyük/küçük harf
/// duyarsız karşılaştırma kullanılır; bu, Windows ve Linux arasında aynı kural
/// dosyasının aynı davranışı vermesini sağlar.
#[derive(Debug, Clone, Default)]
pub struct Dislama {
    desenler: Vec<String>,
    gizlileri_haric: bool,
}

impl Dislama {
    /// Boş dışlama listesi üretir (hiçbir şey dışlanmaz).
    pub fn yeni() -> Self {
        Dislama {
            desenler: Vec::new(),
            gizlileri_haric: false,
        }
    }

    /// Bir jokerli ad deseni ekler.
    pub fn desen_ekle(&mut self, desen: &str) {
        self.desenler.push(desen.to_ascii_lowercase());
    }

    /// Nokta ile başlayan adlar dışlansın mı ayarlar.
    pub fn gizlileri_haric_ayarla(&mut self, deger: bool) {
        self.gizlileri_haric = deger;
    }

    /// Herhangi bir dışlama kuralı var mı?
    pub fn bos_mu(&self) -> bool {
        self.desenler.is_empty() && !self.gizlileri_haric
    }

    /// Verilen yol dışlanıyor mu?
    ///
    /// Dışlanan bir klasörün **altındaki hiçbir yol** da dışlanmaz sayılır; aksi hâlde
    /// dışlanan klasörün içi yeniden gezilir ve boşuna zaman harcanır.
    pub fn disli_mi(&self, yol: &Yol) -> bool {
        if yol.kok_mu() {
            return false;
        }
        if self.gizlileri_haric
            && yol
                .0
                .split('/')
                .any(|ad| ad.starts_with('.') && ad.len() > 1)
        {
            return true;
        }
        self.desenler.iter().any(|desen| desen_eslesir(desen, yol))
    }
}

/// Tek bir jokerli desenin kanonik yola karşı eşleşip eşleşmediğini hesaplar.
///
/// Desen, yolun **son `n` bileşeniyle** eşleşmelidir; deneme, yolun her
/// başlangıç konumundan yapılır. Böylece `*.tmp` deseni `a/b/c.tmp` ile
/// eşleşirken `a/*.tmp` deseni `a/b/c.tmp` ile eşleşmez; `**/node_modules/**`
/// ise her derinlikteki `node_modules` klasörünü yakalar.
pub fn desen_eslesir(desen: &str, yol: &Yol) -> bool {
    let desen = desen.to_ascii_lowercase();
    let desen_parcalar: Vec<&str> = desen.split('/').collect();
    let parcalar: Vec<&str> = yol.0.split('/').collect();
    (0..=parcalar.len())
        .any(|baslangic| desen_bilesenlerine_bol(&desen_parcalar, &parcalar[baslangic..]))
}

fn desen_bilesenlerine_bol(desen_parcalar: &[&str], parcalar: &[&str]) -> bool {
    let Some((ilk, kalan)) = desen_parcalar.split_first() else {
        return parcalar.is_empty();
    };
    if *ilk == "**" {
        // `**` sıfır veya daha fazla bileşen tüketir; sonraki desen kalıntıyla
        // eşleşmelidir.
        for atla in 0..=parcalar.len() {
            if desen_bilesenlerine_bol(kalan, &parcalar[atla..]) {
                return true;
            }
        }
        return false;
    }
    let Some((gercek, kalan_yol)) = parcalar.split_first() else {
        return false;
    };
    tek_ad_eslesir(ilk, gercek) && desen_bilesenlerine_bol(kalan, kalan_yol)
}

/// Tek bir ad bileşenini `*` ve `?` jokerleriyle eşleştirir.
///
/// Klasik "iki işaretçi + geri izleme" algoritmasıdır: `*` görüldüğünde konumu
/// kaydedilir, sonraki uyuşmazlıklarda `*`in kapsadığı aralık genişletilir.
/// Karşılaştırma büyük/küçük harf duyarsızdır; böylece aynı kural dosyası
/// Windows ve Linux'ta aynı davranışı gösterir.
pub fn tek_ad_eslesir(kalip: &str, ad: &str) -> bool {
    let kalip: Vec<char> = kalip.chars().collect();
    let ad: Vec<char> = ad.chars().collect();
    let mut i = 0usize;
    let mut j = 0usize;
    let mut yildiz: Option<usize> = None;
    let mut yildiz_sonrasi = 0usize;
    while i < ad.len() {
        let eslesti = j < kalip.len() && (kalip[j] == '?' || kalip[j].eq_ignore_ascii_case(&ad[i]));
        if eslesti {
            i += 1;
            j += 1;
        } else if j < kalip.len() && kalip[j] == '*' {
            yildiz = Some(j);
            yildiz_sonrasi = i;
            j += 1;
        } else if let Some(yildiz_yeri) = yildiz {
            j = yildiz_yeri + 1;
            yildiz_sonrasi += 1;
            i = yildiz_sonrasi;
        } else {
            return false;
        }
    }
    while j < kalip.len() && kalip[j] == '*' {
        j += 1;
    }
    j == kalip.len()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
// expect/unwrap test kodunda bilinçlidir: WORKER_CONTRACT.md § 4.2 bunları
// yalnızca testlerde serbest bırakır. Bir testin expect çağrısının
// başarısız olması, o iddianın yanlış olduğunun en net sinyalidir.
mod tests {
    use super::*;

    #[test]
    fn kok_yolu_bos_metindir() {
        let y = Yol::kok();
        assert_eq!(y.metin(), "");
        assert!(y.kok_mu());
        assert_eq!(y.derinlik(), 0);
    }

    #[test]
    fn metin_yap_ayiraclari_sadelesirir() {
        assert_eq!(Yol::metin_yap("a\\b//c/").metin(), "a/b/c");
        assert_eq!(Yol::metin_yap("./a/./b").metin(), "a/b");
        assert_eq!(Yol::metin_yap("").metin(), "");
    }

    #[test]
    fn derinlik_bilesen_sayisiyle_ayni() {
        assert_eq!(Yol::metin_yap("a/b/c").derinlik(), 3);
        assert_eq!(Yol::metin_yap("a").derinlik(), 1);
    }

    #[test]
    fn son_bilesen_dogru_donuyor() {
        assert_eq!(Yol::metin_yap("a/b/c.txt").son_bilesen(), Some("c.txt"));
        assert_eq!(Yol::kok().son_bilesen(), None);
    }

    #[test]
    fn altinda_kendisi_ve_cocuklari_kapsiyor() {
        let us = Yol::metin_yap("a/b");
        assert!(Yol::metin_yap("a/b").altinda(&us));
        assert!(Yol::metin_yap("a/b/c").altinda(&us));
        assert!(!Yol::metin_yap("a/bc").altinda(&us));
        assert!(!Yol::metin_yap("a").altinda(&us));
        assert!(Yol::metin_yap("her/yer").altinda(&Yol::kok()));
    }

    #[test]
    fn ust_n_yolu_kirpar() {
        let y = Yol::metin_yap("a/b/c/d");
        assert_eq!(y.ust_n(0).metin(), "");
        assert_eq!(y.ust_n(2).metin(), "a/b");
        assert_eq!(y.ust_n(9).metin(), "a/b/c/d");
    }

    #[test]
    fn siralama_bilesen_bilesen_calisiyor() {
        // Ham metin sıralaması `a.txt`'i `a/b`'den önce koyardı (`.` < `/`).
        // Bileşen sıralaması ise ilk bileşenleri karşılaştırır: `a` < `a.txt`.
        let liste = [Yol::metin_yap("a.txt"), Yol::metin_yap("a/b")];
        let mut sirali = liste.to_vec();
        sirali.sort();
        assert_eq!(sirali[0].metin(), "a/b");
        assert_eq!(sirali[1].metin(), "a.txt");

        // Bir klasör, kendi altından da önce gelir (önek sıralaması).
        let onlu = [Yol::metin_yap("a/b"), Yol::metin_yap("a")];
        let mut s = onlu.to_vec();
        s.sort();
        assert_eq!(s[0].metin(), "a");
        assert_eq!(s[1].metin(), "a/b");
    }

    #[test]
    fn joker_yildiz_tek_bilesende_calisiyor() {
        assert!(tek_ad_eslesir("*.tmp", "a.tmp"));
        assert!(tek_ad_eslesir("*.tmp", "ABC.TMP"));
        assert!(!tek_ad_eslesir("*.tmp", "a.txt"));
        assert!(tek_ad_eslesir("a?c", "abc"));
    }

    #[test]
    fn joker_geri_izlemeli_eslestiriyor() {
        // Joker deseni **adın tamamıyla** eşleşmelidir: sonunda `*` yoksa
        // sondaki karakterler artık kalıp dışıdır.
        assert!(tek_ad_eslesir("*a*b", "xxayb"));
        assert!(!tek_ad_eslesir("*a*b", "xxaybzz"));
        assert!(!tek_ad_eslesir("*a*b", "xxbyzz"));
        assert!(tek_ad_eslesir("**", "karsilik"));
        assert!(tek_ad_eslesir("*.txt", "a.txt"));
        assert!(!tek_ad_eslesir("*.txt", "a.txtx"));
        assert!(tek_ad_eslesir("a*b*c", "a_b__c"));
    }

    #[test]
    fn desen_yalnizca_son_bilesenlere_bakar() {
        let y = Yol::metin_yap("a/b/c.tmp");
        assert!(desen_eslesir("*.tmp", &y));
        assert!(!desen_eslesir("a/*.tmp", &y));
        assert!(desen_eslesir("a/b/*.tmp", &y));
    }

    #[test]
    fn cift_yildiz_bilesen_sinirini_asmaz() {
        let y = Yol::metin_yap("a/node_modules/b/c.js");
        assert!(desen_eslesir("**/node_modules/**", &y));
        assert!(!desen_eslesir("**/node_modules", &y));
    }

    #[test]
    fn dislama_listesi_uygulanir() {
        let mut d = Dislama::yeni();
        assert!(d.bos_mu());
        d.desen_ekle("*.tmp");
        d.desen_ekle("**/node_modules/**");
        assert!(!d.bos_mu());
        assert!(d.disli_mi(&Yol::metin_yap("a/b.tmp")));
        assert!(d.disli_mi(&Yol::metin_yap("p/node_modules/q/r.js")));
        assert!(!d.disli_mi(&Yol::metin_yap("a/b.txt")));
        assert!(!d.disli_mi(&Yol::kok()));
    }

    #[test]
    fn gizli_dosya_kurali_ayri_ayarlanir() {
        let mut d = Dislama::yeni();
        d.gizlileri_haric_ayarla(true);
        assert!(d.disli_mi(&Yol::metin_yap(".git")));
        assert!(d.disli_mi(&Yol::metin_yap("a/.gizli/dosya")));
        assert!(!d.disli_mi(&Yol::metin_yap("a/normal")));
        assert!(d.disli_mi(&Yol::metin_yap("a/b/.x")));
        // Kök hiçbir zaman dışlanmaz; aksi hâlde tüm tarama boşa çıkardı.
        assert!(!d.disli_mi(&Yol::kok()));
    }

    #[test]
    fn yoldan_donus_kok_disi_yolu_isaretler() {
        let kok = Path::new("/veri/kok");
        let (y, kayip) = Yol::yoldan(kok, Path::new("/veri/kok/a/b.txt"));
        assert_eq!(y.metin(), "a/b.txt");
        assert!(!kayip);

        let (y2, kayip2) = Yol::yoldan(kok, Path::new("/baska/yer/c.txt"));
        assert!(kayip2);
        assert!(y2.metin().ends_with("c.txt"));
    }

    #[test]
    fn yola_donus_yolu_yeniden_olusturur() {
        let kok = Path::new("/veri/kok");
        let y = Yol::metin_yap("a/b.txt");
        assert_eq!(y.yola(kok), PathBuf::from("/veri/kok/a/b.txt"));
        assert_eq!(Yol::kok().yola(kok), kok);
    }
}
