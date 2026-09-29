//! Test içinde geçici dosya ve dizin üreten yardımcılar.
//!
//! Neden `tempfile` crate'i yok: bağımlılık politikası
//! (`WORKER_CONTRACT.md` § 3.2) `tempfile`'i hiçbir projede vermez; yardımcı
//! kendi kodumuzla yazılır. Benzersizlik `std::process::id()` + etiket ile
//! sağlanır; rastgelelik crate'i kullanılmaz.
//!
//! `Drop` temizliği bilinçli olarak hata yutar (`let _ = ...`): `Drop` içinden
//! hata döndürülemez ve testin başarısı, geçici klasörün silinip silinmediğine
//! bağlı olmamalıdır.

// Bu dosya iki ayrı test ikilisine (`entegrasyon` ve `veri_guvenligi`) modül
// olarak eklenir. Her ikili de yardımcının **yalnızca bir kısmını** kullanır,
// dolayısıyla kullanılmayan yardımcılar "ölü kod" görünür. Bu bir hata değil,
// paylaşımlı yardımcı modülün doğal sonucudur.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Test içinde geçici dosya/dizin üreten, `Drop` ile temizleyen kapsayıcı.
pub struct GeciciDizin {
    yol: PathBuf,
}

impl GeciciDizin {
    /// `std::env::temp_dir()` altında, etiketten türetilen benzersiz bir dizin
    /// oluşturur. Aynı etiketle ikinci kez açılırsa eski içerik önce silinir.
    pub fn yeni(etiket: &str) -> std::io::Result<Self> {
        let kok =
            std::env::temp_dir().join(format!("timefold-it-{}-{}", etiket, std::process::id()));
        let _ = std::fs::remove_dir_all(&kok);
        std::fs::create_dir_all(&kok)?;
        Ok(Self { yol: kok })
    }

    /// Dizin içine göreli yol döndürür.
    pub fn yol(&self) -> &Path {
        &self.yol
    }

    /// Alt dizin oluşturur ve yolunu döndürür.
    pub fn alt(&self, ad: &str) -> PathBuf {
        let yol = self.yol.join(ad);
        std::fs::create_dir_all(&yol).expect("alt dizin oluşturulabilir");
        yol
    }

    /// Dosyaya içerik yazar; üst dizinleri de oluşturur.
    pub fn yaz(&self, gorece_yol: &str, icerik: &[u8]) -> PathBuf {
        let yol = self.yol.join(gorece_yol);
        if let Some(usta) = yol.parent() {
            std::fs::create_dir_all(usta).expect("üst dizin oluşturulabilir");
        }
        std::fs::write(&yol, icerik).expect("dosya yazılabilir");
        yol
    }

    /// Bir dosyanın boyutunu okur.
    pub fn boyut(&self, gorece_yol: &str) -> u64 {
        std::fs::metadata(self.yol.join(gorece_yol))
            .map(|m| m.len())
            .unwrap_or(0)
    }
}

impl Drop for GeciciDizin {
    fn drop(&mut self) {
        // Temizlik başarısız olsa da testi düşürmemeli; `let _ =` bilinçlidir.
        let _ = std::fs::remove_dir_all(&self.yol);
    }
}

/// Bir dizin ağacının tam dökümü: göreli yol → (boyut, salt-okunur bayrağı).
///
/// **Veri güvenliği testlerinin temel aracıdır:** `scan`, `undo`, `restore` gibi
/// komutlardan sonra bu dökümün **birebir aynı** kalması, aracın kullanıcı
/// dosyalarına dokunmadığını kanıtlar. Boyut *ve* izin (salt-okunur bayrağı)
/// birlikte karşılaştırılır; raporun "tarih, içerik ve izin geri alınır" kuralı
/// ancak bu ikiliyle kanıtlanabilir.
pub fn agac_dokumu(kok: &Path) -> BTreeMap<String, (u64, bool)> {
    let mut harita = BTreeMap::new();
    let mut yigin: Vec<PathBuf> = vec![kok.to_path_buf()];
    while let Some(dizin) = yigin.pop() {
        let Ok(girdiler) = std::fs::read_dir(&dizin) else {
            continue;
        };
        for giris in girdiler.flatten() {
            let yol = giris.path();
            let goreli = yol
                .strip_prefix(kok)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| yol.to_string_lossy().into_owned());
            let Ok(meta) = std::fs::symlink_metadata(&yol) else {
                continue;
            };
            if meta.is_dir() {
                harita.insert(goreli.clone(), (0, meta.permissions().readonly()));
                yigin.push(yol);
            } else {
                harita.insert(goreli, (meta.len(), meta.permissions().readonly()));
            }
        }
    }
    harita
}

/// Bir dosyanın salt-okunur olup olmadığını döndürür.
pub fn salt_okunur(yol: &Path) -> bool {
    std::fs::metadata(yol)
        .map(|m| m.permissions().readonly())
        .unwrap_or(false)
}
