//! Timefold komut satırı arayüzü.
//!
//! Yalnızca argüman ayrıştırma ve çıktı yönlendirmesi yapar; tüm iş mantığı
//! [`timefold::uygulama`] modülündedir. Böylece çekirdek, ikili derlenmeden
//! test edilebilir.
//!
//! **Veri güvenliği:** hiçbir alt komut tarama köküne yazmaz. `scan`, `undo`,
//! `redo`, `restore` yalnızca `--depo` ile verilen dizine yazar.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use timefold::hata::Hata;
use timefold::rapor::RaporCikti;
use timefold::uygulama::{
    self, DiffIstegi, ForecastIstegi, GeriIstegi, HeatmapIstegi, ListIstegi, RestoreIstegi,
    ScanIstegi, TimelineIstegi,
};
use timefold::yol::Yol;

/// Klasör/dosya boyut zaman çizelgesi ve ısı haritası analiz aracı.
#[derive(Debug, Parser)]
#[command(
    name = "timefold",
    version,
    about = "Klasör/dosya boyut zaman çizelgesi ve ısı haritası analiz aracı",
    long_about = "Timefold her taramayı bir zaman noktası olarak sürümlü bir depoya yazar, \
iki taramayı karşılaştırır ve \"hangi klasör ne zaman büyüdü\" sorusuna büyüme hızı ve \
güven aralığıyla yanıt verir. Hiçbir komut kullanıcı dosyasını silmez veya değiştirmez."
)]
struct Cli {
    #[command(subcommand)]
    komut: Komut,
}

#[derive(Debug, Subcommand)]
enum Komut {
    /// Dizini tarar ve anlık görüntüyü depoya ekler.
    Scan {
        /// Taranacak kök dizin.
        #[arg(value_name = "KOK")]
        kok: PathBuf,
        /// Anlık görüntü deposunun dizini.
        #[arg(long, value_name = "DIZIN", default_value = "timefold-depo")]
        depo: PathBuf,
        /// Dışlanacak joker desen (birden çok kez verilebilir).
        #[arg(long = "haric", value_name = "DESEN")]
        haric: Vec<String>,
        /// Nokta ile başlayan adları (gizli dosyaları) taramadan çıkar.
        #[arg(long = "gizli-haric")]
        gizli_haric: bool,
        /// İzlenecek en derin seviye.
        #[arg(long = "derinlik", value_name = "SEVIYE", default_value_t = 64)]
        derinlik: usize,
        /// Bu sayıdan fazla girişte örnekleme devreye girer.
        #[arg(long = "en-fazla-giris", value_name = "ADET")]
        en_fazla_giris: Option<u64>,
        /// Korunacak en yeni anlık görüntü sayısı.
        #[arg(long = "tut", value_name = "ADET")]
        tut: Option<usize>,
        /// Eski anlık görüntüleri günlük seyrelt.
        #[arg(long = "gunluk")]
        gunluk: bool,
        /// Hiçbir şey yazmadan ne olacağını göster.
        #[arg(long = "kuru-sur")]
        kuru_sur: bool,
        /// Anlık görüntünün zaman damgasını elle ver (UTC unix saniye).
        ///
        /// Varsayılan: sistem saati. Geçmiş veri üretirken veya testlerde
        /// "günlük" büyüme serisi kurarken kullanılır; varsayılan saat 300 saniye
        /// aralığında tarama yapıldığı için günlük eğilim üretmez.
        #[arg(long, value_name = "UNIX_SANIYE")]
        zaman: Option<i64>,
        /// Çıktıyı JSON olarak yaz.
        #[arg(long)]
        json: bool,
    },
    /// İki anlık görüntü arasındaki farkı gösterir.
    Diff {
        /// Anlık görüntü deposunun dizini.
        #[arg(long, value_name = "DIZIN", default_value = "timefold-depo")]
        depo: PathBuf,
        /// Karşılaştırılacak önceki anlık görüntü (varsayılan: en yeni).
        #[arg(long, value_name = "KIMLIK")]
        onceki: Option<String>,
        /// Listede gösterilecek en fazla kayıt sayısı.
        #[arg(long, value_name = "ADET", default_value_t = 20)]
        en_cok: usize,
        /// Çıktıyı JSON olarak yaz.
        #[arg(long)]
        json: bool,
    },
    /// Bir yolun boyut geçmişini zaman çizelgesi olarak gösterir.
    Timeline {
        /// Anlık görüntü deposunun dizini.
        #[arg(long, value_name = "DIZIN", default_value = "timefold-depo")]
        depo: PathBuf,
        /// İncelenecek göreli yol.
        #[arg(long, value_name = "YOL")]
        yol: String,
        /// Çıktıyı JSON olarak yaz.
        #[arg(long)]
        json: bool,
    },
    /// Isı haritası: değişim büyüklüğü × derinlik.
    Heatmap {
        /// Anlık görüntü deposunun dizini.
        #[arg(long, value_name = "DIZIN", default_value = "timefold-depo")]
        depo: PathBuf,
        /// Başlangıç anlık görüntüsü (varsayılan: son iki anlık görüntü).
        #[arg(long, value_name = "KIMLIK")]
        baslangic: Option<String>,
        /// Sonuçları bu üst yolün altına indir.
        #[arg(long = "altina", value_name = "YOL")]
        altina: Option<String>,
        /// Listede gösterilecek en fazla satır.
        #[arg(long, value_name = "ADET", default_value_t = 20)]
        en_cok: usize,
        /// Çıktıyı JSON olarak yaz.
        #[arg(long)]
        json: bool,
    },
    /// Son anlık görüntüyü geri alır (yalnızca arşivden).
    Undo {
        /// Anlık görüntü deposunun dizini.
        #[arg(long, value_name = "DIZIN", default_value = "timefold-depo")]
        depo: PathBuf,
        /// Hiçbir şey yazmadan ne olacağını göster.
        #[arg(long = "kuru-sur")]
        kuru_sur: bool,
        /// Çıktıyı JSON olarak yaz.
        #[arg(long)]
        json: bool,
    },
    /// Geri alınan anlık görüntüyü yeniden uygular.
    Redo {
        /// Anlık görüntü deposunun dizini.
        #[arg(long, value_name = "DIZIN", default_value = "timefold-depo")]
        depo: PathBuf,
        /// Hiçbir şey yazmadan ne olacağını göster.
        #[arg(long = "kuru-sur")]
        kuru_sur: bool,
        /// Çıktıyı JSON olarak yaz.
        #[arg(long)]
        json: bool,
    },
    /// Belirtilen anlık görüntüye geri döner.
    Restore {
        /// Anlık görüntü deposunun dizini.
        #[arg(long, value_name = "DIZIN", default_value = "timefold-depo")]
        depo: PathBuf,
        /// Dönülecek anlık görüntü kimliği.
        #[arg(long, value_name = "KIMLIK")]
        hedef: String,
        /// Hiçbir şey yazmadan ne olacağını göster.
        #[arg(long = "kuru-sur")]
        kuru_sur: bool,
        /// Çıktıyı JSON olarak yaz.
        #[arg(long)]
        json: bool,
    },
    /// Doğrusal eğilimden tahmin ve güven aralığı üretir.
    Forecast {
        /// Anlık görüntü deposunun dizini.
        #[arg(long, value_name = "DIZIN", default_value = "timefold-depo")]
        depo: PathBuf,
        /// İncelenecek göreli yol.
        #[arg(long, value_name = "YOL")]
        yol: String,
        /// Kaç gün sonrası tahmin edilsin.
        #[arg(long, value_name = "GUN", default_value_t = 30)]
        ileri_gun: u32,
        /// Çıktıyı JSON olarak yaz.
        #[arg(long)]
        json: bool,
    },
    /// Depodaki anlık görüntüleri listeler.
    List {
        /// Anlık görüntü deposunun dizini.
        #[arg(long, value_name = "DIZIN", default_value = "timefold-depo")]
        depo: PathBuf,
        /// Çıktıyı JSON olarak yaz.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (cikti, json_modu) = match calistir(cli.komut) {
        Ok(sonuc) => sonuc,
        Err(hata) => {
            eprintln!("hata: {}", hata);
            return ExitCode::FAILURE;
        }
    };
    yazdir(&cikti, json_modu);
    ExitCode::SUCCESS
}

#[allow(clippy::fn_params_excessive_bools)]
fn calistir(komut: Komut) -> timefold::hata::Sonuc<(RaporCikti, bool)> {
    match komut {
        Komut::Scan {
            kok,
            depo,
            haric,
            gizli_haric,
            derinlik,
            en_fazla_giris,
            tut,
            gunluk,
            kuru_sur,
            zaman,
            json,
        } => {
            let istek = ScanIstegi {
                kok,
                depo,
                haric,
                gizli_haric,
                derinlik,
                en_fazla_giris,
                tut,
                gunluk,
                kuru_sur,
                zaman: zaman.unwrap_or_else(timefold::zaman::simdi_unix_saniye),
            };
            Ok((uygulama::scan_yurut(&istek)?, json))
        }
        Komut::Diff {
            depo,
            onceki,
            en_cok,
            json,
        } => {
            let istek = DiffIstegi {
                depo,
                onceki,
                en_cok,
            };
            Ok((uygulama::diff_yurut(&istek)?, json))
        }
        Komut::Timeline { depo, yol, json } => {
            let istek = TimelineIstegi {
                depo,
                yol: Yol::metin_yap(&yol),
            };
            Ok((uygulama::timeline_yurut(&istek)?, json))
        }
        Komut::Heatmap {
            depo,
            baslangic,
            altina,
            en_cok,
            json,
        } => {
            let istek = HeatmapIstegi {
                depo,
                baslangic,
                en_cok,
                altina: altina.as_deref().map(Yol::metin_yap),
            };
            Ok((uygulama::heatmap_yurut(&istek)?, json))
        }
        Komut::Undo {
            depo,
            kuru_sur,
            json,
        } => {
            let istek = GeriIstegi { depo, kuru_sur };
            Ok((uygulama::undo_yurut(&istek)?, json))
        }
        Komut::Redo {
            depo,
            kuru_sur,
            json,
        } => {
            let istek = GeriIstegi { depo, kuru_sur };
            Ok((uygulama::redo_yurut(&istek)?, json))
        }
        Komut::Restore {
            depo,
            hedef,
            kuru_sur,
            json,
        } => {
            let istek = RestoreIstegi {
                depo,
                hedef,
                kuru_sur,
            };
            Ok((uygulama::restore_yurut(&istek)?, json))
        }
        Komut::Forecast {
            depo,
            yol,
            ileri_gun,
            json,
        } => {
            let istek = ForecastIstegi {
                depo,
                yol: Yol::metin_yap(&yol),
                ileri_gun,
            };
            Ok((uygulama::forecast_yurut(&istek)?, json))
        }
        Komut::List { depo, json } => {
            let istek = ListIstegi { depo };
            Ok((uygulama::list_yurut(&istek)?, json))
        }
    }
}

fn yazdir(cikti: &RaporCikti, json_modu: bool) {
    if json_modu {
        match serde_json::to_string_pretty(&cikti.json) {
            Ok(metin) => println!("{}", metin),
            Err(hata) => eprintln!("hata: JSON çıktısı oluşturulamadı: {}", hata),
        }
    } else {
        println!("{}", cikti.metin);
    }
}

// `Hata` yalnızca `main` içinde `Display` ile gösterilir; burada yalnızca
// tipin kullanıldığını doğrulamak için bir referans tutulur.
#[allow(dead_code)]
fn hata_tipi_kullanilabilir(h: &Hata) -> String {
    h.to_string()
}
