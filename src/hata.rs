//! Hata tipi ve sonuç takma adı.
//!
//! Bu modül yalnızca hata taşır: hiçbir iş yürütmez, hiçbir dosyaya dokunmaz.
//! Tarama kökü bulunamazsa, depo bozuksa, sürüm uyuşmazlığı olursa veya komut
//! satırından tutarsız bir parametre gelirse üretilen hatalar burada toplanır;
//! çağıran taraf `Display` çıktısını kullanıcıya gösterir.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Tüm genel işlemlerin sonuç tipi.
pub type Sonuc<T> = Result<T, Hata>;

/// Timefold'un ürettiği hataların tamamı.
#[derive(Debug)]
#[non_exhaustive]
pub enum Hata {
    /// Dosya sistemi işlemi başarısız oldu.
    Io {
        /// Yapılmak istenen işlem ("dizin tarama", "depo aç" ...).
        eylem: &'static str,
        /// İşlemin konusu olan yol.
        yol: PathBuf,
        /// Altta yatan işletim sistemi hatası.
        kaynak: io::Error,
    },
    /// Tarama kökü bir dizin değil veya hiç yok.
    TaramaYoluHatali {
        /// Verilen yol.
        yol: PathBuf,
        /// Neden uygun olmadığı.
        ayrinti: String,
    },
    /// Depo dizini bulunamadı veya depoda okunabilir bir anlık görüntü yok.
    DepoBosOrBozuk {
        /// Depo dizininin yolu.
        dizin: PathBuf,
        /// Ayrıntılı açıklama.
        ayrinti: String,
    },
    /// Anlık görüntü dosyasının biçim sürümü bu sürümle uyuşmuyor.
    ///
    /// Depo **okunmaz ve değiştirilmez**; kullanıcı verisi veya geçmiş kaybı oluşmaz.
    SurumUyusmazligi {
        /// Okunan dosyanın yolu.
        dosya: PathBuf,
        /// Dosyada yazan sürüm.
        bulunan: u32,
        /// Bu sürümün anladığı sürüm.
        beklenen: u32,
    },
    /// JSONL satırı çözümlenemedi veya şemaya uymadı.
    BozukSatir {
        /// Hatanın bulunduğu dosya.
        dosya: PathBuf,
        /// Satırın 1 tabanlı numarası.
        satir: u64,
        /// Ayrıntılı açıklama.
        ayrinti: String,
    },
    /// İstenen anlık görüntü kimliği depoda yok.
    AnlikGoruntuYok {
        /// Kullanıcının istediği kimlik.
        istenen: String,
        /// Depoda bulunan en yeni kimlik.
        en_yeni: Option<String>,
    },
    /// Komut satırından gelen eksik veya tutarsız parametre.
    Parametre {
        /// Sorunlu parametrenin adı.
        ad: &'static str,
        /// Açıklama.
        ayrinti: String,
    },
    /// Zaman damgası RFC 3339 biçimine uymuyor.
    GecersizZaman {
        /// Ayrıntılı açıklama.
        ayrinti: String,
    },
    /// Yapılan işlem "bu durumda anlamsız" (ör. yığın boşken undo).
    GecersizDurum {
        /// Açıklama.
        ayrinti: String,
    },
}

impl Hata {
    /// Dosya sistemi hatasını yol ve eylem bağlamıyla sarar.
    pub fn io(eylem: &'static str, yol: &Path, kaynak: io::Error) -> Self {
        Hata::Io {
            eylem,
            yol: yol.to_path_buf(),
            kaynak,
        }
    }

    /// Parametre hatası üretir.
    pub fn parametre(ad: &'static str, ayrinti: impl Into<String>) -> Self {
        Hata::Parametre {
            ad,
            ayrinti: ayrinti.into(),
        }
    }

    /// Durum hatası üretir.
    pub fn gecersiz_durum(ayrinti: impl Into<String>) -> Self {
        Hata::GecersizDurum {
            ayrinti: ayrinti.into(),
        }
    }
}

impl fmt::Display for Hata {
    fn fmt(&self, bicik: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Hata::Io { eylem, yol, kaynak } => {
                write!(bicik, "{} başarısız ({}): {}", eylem, yol.display(), kaynak)
            }
            Hata::TaramaYoluHatali { yol, ayrinti } => {
                write!(
                    bicik,
                    "tarama kökü geçersiz ({}): {}",
                    yol.display(),
                    ayrinti
                )
            }
            Hata::DepoBosOrBozuk { dizin, ayrinti } => {
                write!(bicik, "depo okunamadı ({}): {}", dizin.display(), ayrinti)
            }
            Hata::SurumUyusmazligi {
                dosya,
                bulunan,
                beklenen,
            } => write!(
                bicik,
                "biçim sürümü uyuşmuyor ({}): dosyada {}, beklenen {}",
                dosya.display(),
                bulunan,
                beklenen
            ),
            Hata::BozukSatir {
                dosya,
                satir,
                ayrinti,
            } => write!(
                bicik,
                "bozuk kayıt ({}:{}): {}",
                dosya.display(),
                satir,
                ayrinti
            ),
            Hata::AnlikGoruntuYok { istenen, en_yeni } => match en_yeni {
                Some(y) => write!(
                    bicik,
                    "anlık görüntü yok (istenen: {}; en yeni: {})",
                    istenen, y
                ),
                None => write!(bicik, "anlık görüntü yok (istenen: {}; depo boş)", istenen),
            },
            Hata::Parametre { ad, ayrinti } => {
                write!(bicik, "geçersiz parametre ({}): {}", ad, ayrinti)
            }
            Hata::GecersizZaman { ayrinti } => write!(bicik, "geçersiz zaman damgası: {}", ayrinti),
            Hata::GecersizDurum { ayrinti } => write!(bicik, "geçersiz durum: {}", ayrinti),
        }
    }
}

impl std::error::Error for Hata {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Hata::Io { kaynak, .. } => Some(kaynak),
            _ => None,
        }
    }
}
