//! Uçtan uca akış testleri: tarama → fark → zaman çizelgesi → ısı haritası →
//! tahmin → undo/redo → restore.
//!
//! Bu dosya "gerçek bir kullanıcı ne görür" sorusuna yanıt verir: metin çıktısı
//! ve JSON çıktısının ikisinin de üretilebildiğini ve aralarında tutarlı olduğunu
//! doğrular.

mod yardimci;

use std::path::{Path, PathBuf};

use timefold::depo::Depo;
use timefold::hata::Hata;
use timefold::kayit::KayitTuru;
use timefold::uygulama::{
    self, DiffIstegi, ForecastIstegi, GeriIstegi, HeatmapIstegi, ListIstegi, RestoreIstegi,
    ScanIstegi, TimelineIstegi,
};
use timefold::yol::Yol;
use yardimci::GeciciDizin;

/// Testlerde kullanılan sabit temel zaman (2026-09-29T00:00:00Z).
const T0: i64 = 1_791_302_400;

/// Bir günün saniye cinsinden uzunluğu.
const GUN: i64 = 86_400;

fn scan_istegi(kok: &Path, depo: &Path, zaman: i64) -> ScanIstegi {
    ScanIstegi {
        kok: kok.to_path_buf(),
        depo: depo.to_path_buf(),
        haric: Vec::new(),
        gizli_haric: false,
        derinlik: 64,
        en_fazla_giris: None,
        tut: None,
        gunluk: false,
        kuru_sur: false,
        zaman,
    }
}

fn depo_yolu(dizin: &GeciciDizin) -> PathBuf {
    dizin.yol().join("depo")
}

/// Örnek ağaç: `a/x.txt` (5 bayt), `a/alt/y.txt` (3 bayt), `b.txt` (2 bayt).
fn ornek_agac(dizin: &GeciciDizin) -> PathBuf {
    let kok = dizin.alt("kok");
    dizin.yaz("kok/a/x.txt", b"12345");
    dizin.yaz("kok/a/alt/y.txt", b"123");
    dizin.yaz("kok/b.txt", b"12");
    kok
}

#[test]
fn tam_akis_scan_diff_timeline_heatmap_forecast() {
    let dizin = GeciciDizin::yeni("tam-akis").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);

    // --- 1. İlk tarama: her şey "eklendi" ---------------------------------
    let cikti = uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("ilk tarama");
    assert!(cikti.metin.contains("T0001"));
    assert_eq!(cikti.json["fark"]["eklendi"], 6, "5 yol + 1 klasör");
    assert_eq!(cikti.json["toplam_bayt"], 10);

    // --- 2. Ağaç büyür, bir dal küçülür -----------------------------------
    dizin.yaz("kok/a/x.txt", b"1234567890"); // 5 -> 10 bayt
    dizin.yaz("kok/c/yeni.txt", b"abcde"); // yeni 5 bayt
    let cikti = uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + GUN)).expect("ikinci tarama");
    assert_eq!(cikti.json["fark"]["eklendi"], 2, "c ve c/klasör");
    assert_eq!(cikti.json["fark"]["silindi"], 0);
    assert!(cikti.json["fark"]["degisti"].as_u64().expect("sayı") >= 3);

    // --- 3. Fark raporu ---------------------------------------------------
    let diff = uygulama::diff_yurut(&DiffIstegi {
        depo: depo.clone(),
        onceki: None,
        en_cok: 50,
    })
    .expect("diff");
    assert!(diff.metin.contains("fark özeti"));
    assert!(diff.json["kayitlar"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false));
    // Aynı veri iki kez sorgulanırsa çıktı birebir aynı olmalı.
    let diff2 = uygulama::diff_yurut(&DiffIstegi {
        depo: depo.clone(),
        onceki: None,
        en_cok: 50,
    })
    .expect("diff 2");
    assert_eq!(diff.metin, diff2.metin, "diff kararlı olmalı");

    // --- 4. Zaman çizelgesi ------------------------------------------------
    let cizelge = uygulama::timeline_yurut(&TimelineIstegi {
        depo: depo.clone(),
        yol: Yol::metin_yap("a/x.txt"),
    })
    .expect("timeline");
    assert_eq!(cikti_adet(&cizelge), 2);
    let noktalar = cizelge.json["noktalar"].as_array().expect("nokta dizisi");
    assert_eq!(noktalar[0]["bayt"], 5);
    assert_eq!(noktalar[1]["bayt"], 10);
    assert_eq!(noktalar[1]["fark"], 5);

    // --- 5. Isı haritası: derinlik ağırlığı --------------------------------
    let isi = uygulama::heatmap_yurut(&HeatmapIstegi {
        depo: depo.clone(),
        baslangic: Some("T0001".to_owned()),
        en_cok: 20,
        altina: None,
    })
    .expect("heatmap");
    let satirlar = isi.json["satirlar"].as_array().expect("ısı satırları");
    // `a/alt/y.txt` değişmedi; yalnızca büyüyenler listelenir.
    assert!(
        !satirlar.iter().any(|s| s["yol"] == "a/alt/y.txt"),
        "değişmeyen yol ısıda olmamalı"
    );
    let derin = satirlar
        .iter()
        .find(|s| s["yol"] == "a")
        .expect("a klasörü ısıda");
    // "a" 5 bayt büyüdü (10-5), derinlik 1 → 5 × 2 = 10
    assert_eq!(derin["skor"], 10);
    assert_eq!(derin["derinlik"], 1);

    // --- 6. Tahmin: doğrusal büyüme + güven aralığı -----------------------
    let kok2 = ornek_agac(&dizin);
    let _ = kok2;
    for gun in 2_i64..=5 {
        dizin.yaz("kok/a/x.txt", &vec![b'x'; (5 + gun) as usize]);
        uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + gun * GUN)).expect("tarama");
    }
    let tahmin = uygulama::forecast_yurut(&ForecastIstegi {
        depo: depo.clone(),
        yol: Yol::metin_yap("a/x.txt"),
        ileri_gun: 30,
    })
    .expect("forecast");
    assert_eq!(tahmin.json["hesaplandi"], true);
    let alt = tahmin.json["alt"].as_f64().expect("alt sınır");
    let orta = tahmin.json["tahmin"].as_f64().expect("tahmin");
    let ust = tahmin.json["ust"].as_f64().expect("üst sınır");
    assert!(
        alt <= orta && orta <= ust,
        "güven aralığı tahmini kapsamalı"
    );
    let varsayimlar = tahmin.json["varsayimlar"]
        .as_array()
        .expect("varsayım listesi");
    assert!(!varsayimlar.is_empty(), "varsayım listesi boş olamaz");
}

fn cikti_adet(cikti: &timefold::rapor::RaporCikti) -> usize {
    cikti.json["gozlem"].as_u64().expect("gözlem sayısı") as usize
}

#[test]
fn scan_diff_timeline_heatmap_forecast_kuru_surma_hicbir_sey_yazmaz() {
    let dizin = GeciciDizin::yeni("kuru").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    let agac_oncesi = yardimci::agac_dokumu(&kok);

    let mut istek = scan_istegi(&kok, &depo, T0);
    istek.kuru_sur = true;
    let cikti = uygulama::scan_yurut(&istek).expect("kuru tarama");
    assert!(cikti.metin.contains("yazılmadı"));
    assert_eq!(cikti.json["uygulandi"], false);

    // Depo dizini bile oluşmamalı; okunamayan depo hata vermelidir.
    assert!(!depo.join("index.json").exists());
    assert!(Depo::ac(&depo).is_err());
    assert_eq!(yardimci::agac_dokumu(&kok), agac_oncesi, "ağaç değişmemeli");
}

#[test]
fn iki_kez_ayni_tarama_bos_fark_uretir() {
    let dizin = GeciciDizin::yeni("ayni-tarama").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("1");
    let cikti = uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + 10)).expect("2");
    assert_eq!(cikti.json["fark"]["eklendi"], 0);
    assert_eq!(cikti.json["fark"]["silindi"], 0);
    assert_eq!(cikti.json["fark"]["degisti"], 0);
    assert_eq!(cikti.json["fark"]["net_bayt"], 0);
}

#[test]
fn ayni_boyut_farkli_icerik_yalnizca_zaman_damgasi_ile_ayirt_edilir() {
    let dizin = GeciciDizin::yeni("ayni-boyut").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("1");

    // Aynı uzunlukta, farklı içerik.
    dizin.yaz("kok/a/x.txt", b"54321");
    let cikti = uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + 60)).expect("2");
    // Boyut aynı kaldığı için fark boştur: içerik değişimi kasıtlı olarak
    // raporlanmaz (bkz. README → Bilinen Sınırlamalar).
    assert_eq!(
        cikti.json["fark"]["degisti"], 0,
        "aynı boyutta içerik değişimi boyut farkı üretmez"
    );
    assert_eq!(cikti.json["fark"]["net_bayt"], 0);
}

#[test]
fn silinen_dosya_ve_klasor_fark_uretiyor() {
    let dizin = GeciciDizin::yeni("silinen").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("1");
    std::fs::remove_file(kok.join("b.txt")).expect("b silinir");
    std::fs::remove_dir_all(kok.join("a/alt")).expect("alt klasör silinir");
    let cikti = uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + GUN)).expect("2");
    assert_eq!(
        cikti.json["fark"]["silindi"], 3,
        "b.txt, a/alt ve a/alt/y.txt"
    );
    assert_eq!(cikti.json["toplam_bayt"], 5, "yalnızca a/x.txt kaldı");
}

#[test]
fn surum_uyusmazligi_hata_dondurur_ve_dosyaya_dokunmaz() {
    let dizin = GeciciDizin::yeni("surum").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("1");

    let indeks = depo.join("index.json");
    let onceki_indeks = std::fs::read_to_string(&indeks).expect("okuma");
    std::fs::write(
        &indeks,
        onceki_indeks.replace("\"surum\": 1", "\"surum\": 7"),
    )
    .expect("yazma");

    let sonuc = uygulama::list_yurut(&ListIstegi { depo: depo.clone() });
    assert!(matches!(
        sonuc,
        Err(Hata::SurumUyusmazligi {
            bulunan: 7,
            beklenen: 1,
            ..
        })
    ));
    // Dosya bozulmamış olmalı.
    assert_eq!(
        std::fs::read_to_string(&indeks).expect("okuma"),
        onceki_indeks.replace("\"surum\": 1", "\"surum\": 7")
    );
}

#[test]
fn bozuk_jsonl_satiri_atlanir_ve_sayilir() {
    let dizin = GeciciDizin::yeni("bozuk").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("1");
    dizin.yaz("kok/yeni.txt", b"abc");
    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + GUN)).expect("2");

    // Üçüncü anlık görüntünün fark dosyasına bozuk satır enjekte edilir.
    let depo_ac = Depo::ac(&depo).expect("depo");
    let fark_yolu = depo_ac.fark_yol("T0002");
    let icerik = std::fs::read_to_string(&fark_yolu).expect("okuma");
    let bozuk = format!("{}\n{{bu bir JSON değil}}\n", icerik);
    std::fs::write(&fark_yolu, bozuk).expect("yazma");

    let cizelge = uygulama::timeline_yurut(&TimelineIstegi {
        depo: depo.clone(),
        yol: Yol::metin_yap("a/x.txt"),
    })
    .expect("bozuk satır yüzünden çökmemeli");
    assert!(cikti_adet(&cizelge) >= 2, "okunabilir kayıtlar korunmalı");
}

#[test]
fn eksik_anlik_goruntu_dosyasi_bosluk_olusum_tesbit_ediliyor() {
    let dizin = GeciciDizin::yeni("eksik").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    for gun in 0_i64..4 {
        let boyut = (5 + gun) as usize;
        dizin.yaz("kok/a/x.txt", &vec![b'x'; boyut]);
        uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + gun * GUN)).expect("tarama");
    }
    let depo_ac = Depo::ac(&depo).expect("depo");
    std::fs::remove_file(depo_ac.tam_yol("T0002")).expect("tam silinir");
    std::fs::remove_file(depo_ac.fark_yol("T0002")).expect("fark silinir");

    let cizelge = uygulama::timeline_yurut(&TimelineIstegi {
        depo: depo.clone(),
        yol: Yol::metin_yap("a/x.txt"),
    })
    .expect("timeline");
    let bosluk = cizelge.json["bosluk"].as_array().expect("bosluk listesi");
    assert_eq!(bosluk.len(), 1);
    assert_eq!(bosluk[0], "T0002");
    // Boşluk varsa tahmin **üretilmez**.
    let tahmin = uygulama::forecast_yurut(&ForecastIstegi {
        depo: depo.clone(),
        yol: Yol::metin_yap("a/x.txt"),
        ileri_gun: 30,
    })
    .expect("forecast");
    assert_eq!(tahmin.json["hesaplandi"], false);
    assert!(tahmin.json["gerekce"]
        .as_str()
        .expect("gerekce metni")
        .contains("uydurulmadı"));
}

#[test]
fn undo_redo_ve_restore_akisi_calisiyor() {
    let dizin = GeciciDizin::yeni("undo-undo").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    for gun in 0_i64..5 {
        dizin.yaz("kok/a/x.txt", &vec![b'x'; (5 + gun) as usize]);
        uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + gun * GUN)).expect("tarama");
    }
    assert_eq!(Depo::ac(&depo).expect("depo").adet(), 5);

    let undo = uygulama::undo_yurut(&GeriIstegi {
        depo: depo.clone(),
        kuru_sur: false,
    })
    .expect("undo");
    assert!(undo.metin.contains("T0005"));
    assert_eq!(Depo::ac(&depo).expect("depo").adet(), 4);

    let redo = uygulama::redo_yurut(&GeriIstegi {
        depo: depo.clone(),
        kuru_sur: false,
    })
    .expect("redo");
    assert!(redo.metin.contains("T0005"));
    assert_eq!(Depo::ac(&depo).expect("depo").adet(), 5);

    let restore = uygulama::restore_yurut(&RestoreIstegi {
        depo: depo.clone(),
        hedef: "T0002".to_owned(),
        kuru_sur: false,
    })
    .expect("restore");
    assert!(restore.metin.contains("T0002"));
    assert_eq!(restore.json["kalan_adet"], 2);
    assert!(!Depo::ac(&depo).expect("depo").tam_yol("T0005").exists());
}

#[test]
fn kimlikler_saklama_sonrasi_yeniden_kullanilmaz() {
    let dizin = GeciciDizin::yeni("kimlik").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    for gun in 0_i64..5 {
        dizin.yaz("kok/farkli.txt", &vec![b'x'; (gun + 1) as usize]);
        uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + gun * GUN)).expect("tarama");
    }
    let mut istek = scan_istegi(&kok, &depo, T0 + 10 * GUN);
    istek.tut = Some(2);
    let cikti = uygulama::scan_yurut(&istek).expect("saklamalı tarama");
    // Beşinci tarama T0006 olmalı: T0005'e çakışmamak zorunda.
    assert_eq!(cikti.json["anlik_goruntu"], "T0006");
    let depo_ac = Depo::ac(&depo).expect("depo");
    assert_eq!(depo_ac.adet(), 2, "son 2 anlık görüntü korunmalı");
    assert_eq!(depo_ac.kimlikler(), ["T0005", "T0006"]);
}

#[test]
fn gunluk_seyreltme_her_gun_bir_tane_koruyor() {
    let dizin = GeciciDizin::yeni("gunluk").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    // 6 gün, her gün 3 tarama.
    for gun in 0_i64..6 {
        for saat in 0..3 {
            dizin.yaz("kok/f.txt", &vec![b'x'; (gun * 3 + saat + 1) as usize]);
            uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + gun * GUN + saat * 3600))
                .expect("tarama");
        }
    }
    let mut istek = scan_istegi(&kok, &depo, T0 + 10 * GUN);
    istek.tut = Some(2);
    istek.gunluk = true;
    uygulama::scan_yurut(&istek).expect("saklamalı tarama");

    let depo_ac = Depo::ac(&depo).expect("depo");
    // Son 2 + 6 günün en yenileri + en eski (zincirin başı) korunur.
    assert!(
        depo_ac.adet() >= 7,
        "günlük kapsama korunmalı: {}",
        depo_ac.adet()
    );
    assert!(
        depo_ac.adet() <= 9,
        "seyreltme işe yaramalı: {}",
        depo_ac.adet()
    );
}

#[test]
fn ornekleme_klasor_toplamlarini_alt_sinir_yapiyor() {
    let dizin = GeciciDizin::yeni("ornekleme").expect("geçici dizin");
    let kok = dizin.alt("kok");
    for i in 0..60 {
        dizin.yaz(&format!("kok/d{:02}.txt", i), b"x");
    }
    let depo = depo_yolu(&dizin);

    let tam = uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("tam tarama");
    assert_eq!(tam.json["orneklendi"], false);
    assert_eq!(tam.json["toplam_bayt"], 60);

    // Aynı ağacı örnekleyerek tara: ikinci depo, aynı kök.
    let depo2 = dizin.yol().join("depo2");
    let mut istek = scan_istegi(&kok, &depo2, T0);
    istek.en_fazla_giris = Some(10);
    let ornek = uygulama::scan_yurut(&istek).expect("örnekli tarama");
    assert_eq!(ornek.json["orneklendi"], true);
    let saklanan = ornek.json["yazilan"].as_u64().expect("saklanan");
    assert!(saklanan < 60, "örnekleme kayıt azaltmalı: {}", saklanan);
    let toplam = ornek.json["toplam_bayt"].as_u64().expect("toplam");
    assert!(toplam < 60, "örneklenmiş toplam alt sınırdır: {}", toplam);

    // Aynı komut iki kez çalıştırılınca **aynı** dosyalar seçilir.
    let depo3 = dizin.yol().join("depo3");
    let mut istek2 = scan_istegi(&kok, &depo3, T0);
    istek2.en_fazla_giris = Some(10);
    let ornek2 = uygulama::scan_yurut(&istek2).expect("örnekli tarama 2");
    assert_eq!(
        ornek.json["yazilan"], ornek2.json["yazilan"],
        "örnekleme tekrarlanabilir olmalı"
    );
    assert_eq!(ornek.json["toplam_bayt"], ornek2.json["toplam_bayt"]);
}

#[test]
fn dislama_deseni_taramadan_cikariliyor() {
    let dizin = GeciciDizin::yeni("dislama").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    dizin.yaz("kok/gecici/x.tmp", b"abc");
    dizin.yaz("kok/node_modules/paket.js", b"1234");
    let depo = depo_yolu(&dizin);

    let mut istek = scan_istegi(&kok, &depo, T0);
    istek.haric = vec!["*.tmp".to_owned(), "**/node_modules/**".to_owned()];
    let cikti = uygulama::scan_yurut(&istek).expect("tarama");
    assert!(cikti.json["atlanan_dislama"].as_u64().expect("sayı") >= 2);
    let depo_ac = Depo::ac(&depo).expect("depo");
    let kayitlar = depo_ac.tam_kayitlari("T0001").expect("kayıtlar");
    assert!(!kayitlar.iter().any(|k| k.yol.metin().ends_with(".tmp")));
    assert!(!kayitlar
        .iter()
        .any(|k| k.yol.metin().contains("node_modules")));
}

#[test]
fn gizli_dosyalar_varsayilan_taranir_ve_istege_bagli_cikarilir() {
    let dizin = GeciciDizin::yeni("gizli").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    dizin.yaz("kok/.gizli", b"1234");
    let depo = depo_yolu(&dizin);
    let cikti = uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("tarama");
    assert_eq!(
        cikti.json["toplam_bayt"], 14,
        "gizli dosya varsayılan taranır"
    );

    let depo2 = dizin.yol().join("depo2");
    let mut istek = scan_istegi(&kok, &depo2, T0);
    istek.gizli_haric = true;
    let cikti2 = uygulama::scan_yurut(&istek).expect("tarama 2");
    assert_eq!(cikti2.json["toplam_bayt"], 10);
}

#[test]
fn kismi_tarama_basinlikta_isaretlenir() {
    let dizin = GeciciDizin::yeni("kismi").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    let mut istek = scan_istegi(&kok, &depo, T0);
    istek.derinlik = 2; // a/alt taranmaz
    let cikti = uygulama::scan_yurut(&istek).expect("tarama");
    assert_eq!(cikti.json["kismi"], true);
    assert!(cikti.metin.contains("kısmi tarama"));
    assert!(cikti.metin.contains("derinlik sınırı"));
}

#[test]
fn depo_dizini_taramaya_dahil_edilmez() {
    let dizin = GeciciDizin::yeni("depo-harici").expect("geçici dizin");
    // Depo dizini tarama kökünün **içinde**: kendi çıktısını saymamalı.
    let kok = dizin.alt("kok");
    dizin.yaz("kok/a.txt", b"12");
    let depo = kok.join("timefold-depo");

    let cikti = uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("tarama");
    assert_eq!(cikti.json["toplam_bayt"], 2, "depo içeriği sayılmamalı");
    let cikti2 = uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + GUN)).expect("tarama 2");
    assert_eq!(
        cikti2.json["fark"]["eklendi"], 0,
        "ikinci taramada depo büyümüş görünmemeli"
    );
}

#[test]
fn json_ve_metin_ciktisi_ayni_sayi_lari_tasir() {
    let dizin = GeciciDizin::yeni("cikti").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    let cikti = uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("tarama");
    let metindeki = cikti.metin.clone();
    // Metin çıktısındaki "10 B" ifadesi JSON'daki 10 baytla örtüşmeli.
    assert!(metindeki.contains("10 B"));
    assert_eq!(cikti.json["toplam_bayt"], 10);
}

#[test]
fn klasor_kaydi_alt_agac_toplamini_tasir() {
    let dizin = GeciciDizin::yeni("klasor-toplam").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("tarama");
    let depo_ac = Depo::ac(&depo).expect("depo");
    let kayitlar = depo_ac.tam_kayitlari("T0001").expect("kayıtlar");

    let kok_kaydi = kayitlar.iter().find(|k| k.yol.kok_mu()).expect("kök kaydı");
    assert_eq!(kok_kaydi.tur, KayitTuru::Klasor);
    assert_eq!(kok_kaydi.bayt, 10);

    let alt = kayitlar
        .iter()
        .find(|k| k.yol.metin() == "a/alt")
        .expect("a/alt kaydı");
    assert_eq!(alt.bayt, 3, "a/alt yalnızca kendi dosyasını taşır");
    assert_eq!(alt.tur, KayitTuru::Klasor);
}

#[test]
fn depo_bos_veya_eksikken_hata_dondurur() {
    let dizin = GeciciDizin::yeni("bos-depo").expect("geçici dizin");
    let depo = dizin.yol().join("olmayan-depo");
    assert!(uygulama::list_yurut(&ListIstegi { depo: depo.clone() }).is_err());
    assert!(uygulama::diff_yurut(&DiffIstegi {
        depo,
        onceki: None,
        en_cok: 5
    })
    .is_err());

    let kok = ornek_agac(&dizin);
    let bos = depo_yolu(&dizin);
    Depo::olustur_veya_ac(&bos, &kok).expect("depo");
    // Depo boşken liste çalışır, diff çalışmaz.
    let liste = uygulama::list_yurut(&ListIstegi { depo: bos.clone() }).expect("list");
    assert!(liste.metin.contains("depo boş"));
    assert!(uygulama::diff_yurut(&DiffIstegi {
        depo: bos,
        onceki: None,
        en_cok: 5
    })
    .is_err());
}

#[test]
fn olmayan_kimlik_hata_dondurur() {
    let dizin = GeciciDizin::yeni("olmayan").expect("geçici dizin");
    let kok = ornek_agac(&dizin);
    let depo = depo_yolu(&dizin);
    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("tarama");

    assert!(uygulama::restore_yurut(&RestoreIstegi {
        depo: depo.clone(),
        hedef: "T9999".to_owned(),
        kuru_sur: true,
    })
    .is_err());
    assert!(uygulama::diff_yurut(&DiffIstegi {
        depo,
        onceki: Some("T9999".to_owned()),
        en_cok: 5,
    })
    .is_err());
}

#[test]
fn gecersiz_kok_hata_dondurur() {
    let dizin = GeciciDizin::yeni("gecersiz-kok").expect("geçici dizin");
    let depo = depo_yolu(&dizin);
    let istek = scan_istegi(&dizin.yol().join("yok"), &depo, T0);
    assert!(uygulama::scan_yurut(&istek).is_err());

    let dosya = dizin.yaz("kok/dosya.txt", b"x");
    let istek2 = scan_istegi(&dosya, &depo, T0);
    assert!(
        uygulama::scan_yurut(&istek2).is_err(),
        "dosya kök kabul edilmemeli"
    );
}

#[test]
fn isim_tutarli_ve_hata_mesaji_tasir() {
    let dizin = GeciciDizin::yeni("hata-metni").expect("geçici dizin");
    let depo = dizin.yol().join("yok");
    let hata = Depo::ac(&depo).expect_err("olmayan depo hata vermeli");
    let metin = hata.to_string();
    assert!(
        metin.contains("depo okunamadı"),
        "anlamlı hata metni: {}",
        metin
    );
}
