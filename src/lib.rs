//! Timefold — klasör/dosya boyut zaman çizelgesi ve ısı haritası analiz aracı.
//!
//! Araç, bir dizin ağacını periyodik olarak tarar; her taramayı bir **zaman noktası**
//! olarak sürümlü bir depoya yazar. Her anlık görüntü bir bütün kayıt değil, bir önceki
//! anlık görüntüye göre **fark (delta) kaydıdır**; "o tarihte ne vardı" sorusu zincir
//! okunurken tersine uygulanarak çözülür. Bu, raporun değişim-mekân takası kararının
//! birebir karşılığıdır.
//!
//! **Güvenlik modeli:** Timefold hiçbir komutta kullanıcı dosyasını silmez, taşımaz
//! veya değiştirmez. Tek yazma hedefi aracın kendi deposudur; `restore` ve `undo`
//! dahil hiçbir komut tarama köküne yazmaz. Bu davranış `tests/veri_guvenligi.rs`
//! içindeki testlerle kanıtlanır.
//!
//! Modüller:
//!
//! - [`hata`]: hata tipi ve `Sonuc` takma adı.
//! - [`yol`]: göreli yol kimliği, derinlik ve joker/öneki dışlama.
//! - [`zaman`]: RFC 3339 (UTC) zaman damgası üretimi ve ayrıştırma.
//! - [`kayit`]: anlık görüntü başlığı, tam kayıt ve delta değişim kayıtları.
//! - [`akis`]: satır akışı okuma/yazma, harici sıralama ve akış birleştirme.
//! - [`tarama`]: özyinelemeli dizin gezgini, sembolik bağ koruması, örnekleme.
//! - [`depo`]: sürümlü depo indeksi, anlık görüntü yazma/okuma, undo/redo/restore.
//! - [`delta`]: iki anlık görüntüyü karşılaştırma ve sınıflandırma.
//! - [`zamancizelgesi`]: yol başına boyut geçmişi ve boşluk işaretleme.
//! - [`isiharitasi`]: ısı skoru (değişim büyüklüğü × derinlik) ve bantlama.
//! - [`tahmin`]: en küçük kareler eğilimi, güven aralığı ve varsayım listesi.
//! - [`saklama`]: örnekli saklama politikası (`--tut N`, `--gunluk`).
//! - [`rapor`]: metin ve JSON çıktı biçimlendirme.
//! - [`uygulama`]: komut satırı ile çekirdek arasındaki komut yürütücü.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]

pub mod akis;
pub mod delta;
pub mod depo;
pub mod hata;
pub mod isiharitasi;
pub mod kayit;
pub mod rapor;
pub mod saklama;
pub mod tahmin;
pub mod tarama;
pub mod uygulama;
pub mod yol;
pub mod zaman;
pub mod zamancizelgesi;

pub use akis::{AkisYazici, SatirOkuyucu};
pub use delta::FarkOzeti;
pub use depo::{AnlikGoruntuBilgisi, Depo, DepoOzeti};
pub use hata::{Hata, Sonuc};
pub use isiharitasi::IsiSatiri;
pub use kayit::{AnlikGoruntuBasi, Degisim, KayitTuru, TamKayit};
pub use rapor::RaporCikti;
pub use saklama::SaklamaPolitikasi;
pub use tahmin::Tahmin;
pub use tarama::{TaramaAyari, TaramaOzeti};
pub use zamancizelgesi::ZamanCizelgesi;

/// Aracın sürüm dizesi.
pub const SURUM: &str = env!("CARGO_PKG_VERSION");

/// Anlık görüntü deposu dosya biçiminin sürümü.
///
/// Biçim uyuşmazlığında araç eski anlık görüntüyü **okumaz ve silmez**; hata verir
/// (bkz. [`Hata::SurumUyusmazligi`]). Böylece eski bir depo yanlışlıkla bozulmaz.
pub const BICIM_SURUMU: u32 = 1;
