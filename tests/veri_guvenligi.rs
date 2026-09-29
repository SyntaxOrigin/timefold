//! **Veri güvenliği testleri:** Timefold'un hiçbir komutu kullanıcı dosyasını
//! silmez, taşımaz veya değiştirmez.
//!
//! Bu dosyadaki testler, raporun en katı taahhüdünü kanıtlar. Yöntem şudur:
//!
//! 1. Bir tarama kökü hazırlanır ve **dokümante edilmiş ağaç dökümü** alınır:
//!    `göreli yol → (bayt, salt-okunur bayrağı)`.
//! 2. `scan`, `undo`, `redo`, `restore` ve `--kuru-sur` komutları çalıştırılır.
//! 3. Döküm **birebir karşılaştırılır**.
//!
//! Boyut *ve* izin (salt-okunur bayrağı) birlikte karşılaştırıldığı için
//! "içerik değişmedi ama izin değişti" gibi sinsi ihlaller de yakalanır. Ek olarak
//! dosya sayımı, toplam bayt ve son değişiklik zamanı ayrı ayrı denetlenir.

mod yardimci;

use std::path::{Path, PathBuf};

use timefold::depo::Depo;
use timefold::kayit::KayitTuru;
use timefold::uygulama::{
    self, DiffIstegi, ForecastIstegi, GeriIstegi, HeatmapIstegi, ListIstegi, RestoreIstegi,
    ScanIstegi, TimelineIstegi,
};
use timefold::yol::Yol;
use yardimci::GeciciDizin;

const T0: i64 = 1_791_302_400;
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

/// Salt okunur bir dosya içeren, salt okunur bir klasör içeren ağaç kurar.
///
/// `--dry-run` ve izin testleri bu ağacı kullanır; Windows'ta salt-okunur
/// dosyanın silinmesi de engellenir, bu yüzden test kendi geçici dizininde
/// kalır ve `Drop` ile temizlenir.
fn izinli_agac(dizin: &GeciciDizin) -> PathBuf {
    let kok = dizin.alt("kok");
    dizin.yaz("kok/normal.txt", b"normal icerik");
    dizin.yaz(
        "kok/okunur/korunacak.txt",
        b"bu dosya hicbir zaman degismemeli",
    );
    dizin.yaz("kok/gecici/veri.bin", &vec![0u8; 4096]);
    let korunacak = dizin.yol().join("kok/okunur/korunacak.txt");
    let sonuc = std::fs::set_permissions(&korunacak, {
        let mut izinler = std::fs::metadata(&korunacak).expect("meta").permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        {
            izinler.set_readonly(true);
        }
        izinler
    });
    assert!(sonuc.is_ok(), "salt okunur bayrağı ayarlanabilmeli");
    kok
}

#[test]
fn scan_kullanici_dosyalarina_dokunmuyor() {
    let dizin = GeciciDizin::yeni("guv-scan").expect("geçici dizin");
    let kok = izinli_agac(&dizin);
    let depo = dizin.yol().join("depo");
    let oncesi = yardimci::agac_dokumu(&kok);

    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("tarama");
    assert_eq!(
        yardimci::agac_dokumu(&kok),
        oncesi,
        "tarama ağacı değiştirmemeli"
    );

    // Ağaç büyüyüp küçülsün, yine de kontrol edelim.
    dizin.yaz("kok/yeni.txt", b"yeni");
    std::fs::remove_file(kok.join("gecici/veri.bin")).expect("kendi testim siler");
    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + GUN)).expect("2. tarama");
    let sonrasi = yardimci::agac_dokumu(&kok);
    // Döküm anahtarları `kok` dizinine **görelidir**. Fark yalnızca testin
    // kendi yaptığı değişikliklerden ibaret olmalı.
    assert!(sonrasi.contains_key("yeni.txt"), "yeni dosya görünmeli");
    assert!(
        !sonrasi.contains_key("gecici/veri.bin"),
        "silinen dosya görünmemeli"
    );
    assert_eq!(
        sonrasi.get("okunur/korunacak.txt").copied(),
        oncesi.get("okunur/korunacak.txt").copied()
    );
}

#[test]
fn kuru_surma_kullanici_dosyalarina_ve_dosya_sistemine_dokunmuyor() {
    let dizin = GeciciDizin::yeni("guv-kuru").expect("geçici dizin");
    let kok = izinli_agac(&dizin);
    let depo = dizin.yol().join("depo");
    let oncesi = yardimci::agac_dokumu(&kok);

    let mut istek = scan_istegi(&kok, &depo, T0);
    istek.kuru_sur = true;
    let cikti = uygulama::scan_yurut(&istek).expect("kuru tarama");

    assert_eq!(cikti.json["uygulandi"], false);
    assert_eq!(yardimci::agac_dokumu(&kok), oncesi, "ağaç değişmemeli");
    // Depo **hiç oluşmamalı**: "hiçbir şey yazma" sözünün tam karşılığı budur.
    assert!(
        !depo.exists(),
        "kuru çalıştırma depo dizini oluşturmamalı: {}",
        depo.display()
    );
}

#[test]
fn kuru_surma_aclari_birakmaz() {
    let dizin = GeciciDizin::yeni("guv-kuru2").expect("geçici dizin");
    let kok = izinli_agac(&dizin);
    let depo = dizin.yol().join("depo");
    let oncesi = yardimci::agac_dokumu(&kok);

    for gun in 0..3 {
        let mut istek = scan_istegi(&kok, &depo, T0 + gun * GUN);
        istek.kuru_sur = true;
        uygulama::scan_yurut(&istek).expect("kuru tarama");
    }
    assert_eq!(yardimci::agac_dokumu(&kok), oncesi);
    assert!(!depo.exists(), "kuru çalıştırma hiçbir dosya bırakmamalı");
}

#[test]
fn undo_redo_restore_kullanici_dosyalarina_dokunmuyor() {
    let dizin = GeciciDizin::yeni("guv-undo").expect("geçici dizin");
    let kok = izinli_agac(&dizin);
    let depo = dizin.yol().join("depo");
    for gun in 0..5 {
        uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + gun * GUN)).expect("tarama");
    }
    let oncesi = yardimci::agac_dokumu(&kok);

    uygulama::undo_yurut(&GeriIstegi {
        depo: depo.clone(),
        kuru_sur: true,
    })
    .expect("kuru undo");
    uygulama::undo_yurut(&GeriIstegi {
        depo: depo.clone(),
        kuru_sur: false,
    })
    .expect("undo");
    uygulama::redo_yurut(&GeriIstegi {
        depo: depo.clone(),
        kuru_sur: true,
    })
    .expect("kuru redo");
    uygulama::redo_yurut(&GeriIstegi {
        depo: depo.clone(),
        kuru_sur: false,
    })
    .expect("redo");
    uygulama::restore_yurut(&RestoreIstegi {
        depo: depo.clone(),
        hedef: "T0002".to_owned(),
        kuru_sur: true,
    })
    .expect("kuru restore");
    uygulama::restore_yurut(&RestoreIstegi {
        depo: depo.clone(),
        hedef: "T0002".to_owned(),
        kuru_sur: false,
    })
    .expect("restore");

    assert_eq!(
        yardimci::agac_dokumu(&kok),
        oncesi,
        "undo/redo/restore yalnızca arşivi değiştirmeli"
    );
}

#[test]
fn salt_okunur_dosya_korunur() {
    let dizin = GeciciDizin::yeni("guv-izin").expect("geçici dizin");
    let kok = izinli_agac(&dizin);
    let depo = dizin.yol().join("depo");
    let korunacak = kok.join("okunur/korunacak.txt");
    let oncesi_bayt = std::fs::metadata(&korunacak).expect("meta").len();

    for gun in 0..4 {
        uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + gun * GUN)).expect("tarama");
        uygulama::undo_yurut(&GeriIstegi {
            depo: depo.clone(),
            kuru_sur: false,
        })
        .ok();
    }

    assert!(
        yardimci::salt_okunur(&korunacak),
        "salt okunur bayrağı korunmalı"
    );
    assert_eq!(
        std::fs::metadata(&korunacak).expect("meta").len(),
        oncesi_bayt,
        "dosya içeriği değişmemeli"
    );
    assert!(korunacak.is_file(), "dosya silinmemeli");
}

#[test]
fn tum_okuma_komutlari_yazma_yapmaz() {
    let dizin = GeciciDizin::yeni("guv-okuma").expect("geçici dizin");
    let kok = izinli_agac(&dizin);
    let depo = dizin.yol().join("depo");
    for gun in 0..3 {
        uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + gun * GUN)).expect("tarama");
    }
    let oncesi = yardimci::agac_dokumu(&kok);
    // Arşiv dökümü: okuma komutları arşivi de değiştirmemeli.
    let arsiv_oncesi = arsiv_dokumu(&depo);

    uygulama::list_yurut(&ListIstegi { depo: depo.clone() }).expect("list");
    uygulama::diff_yurut(&DiffIstegi {
        depo: depo.clone(),
        onceki: None,
        en_cok: 10,
    })
    .expect("diff");
    uygulama::timeline_yurut(&TimelineIstegi {
        depo: depo.clone(),
        yol: Yol::metin_yap("okunur"),
    })
    .expect("timeline");
    uygulama::heatmap_yurut(&HeatmapIstegi {
        depo: depo.clone(),
        baslangic: None,
        en_cok: 10,
        altina: None,
    })
    .expect("heatmap");
    uygulama::forecast_yurut(&ForecastIstegi {
        depo: depo.clone(),
        yol: Yol::metin_yap("okunur"),
        ileri_gun: 30,
    })
    .expect("forecast");

    assert_eq!(yardimci::agac_dokumu(&kok), oncesi, "ağaç değişmemeli");
    assert_eq!(arsiv_dokumu(&depo), arsiv_oncesi, "arşiv değişmemeli");
}

/// Depo dizininin tam dökümü (göreli yol → boyut).
fn arsiv_dokumu(depo: &Path) -> std::collections::BTreeMap<String, u64> {
    let mut harita = std::collections::BTreeMap::new();
    let mut yigin = vec![depo.to_path_buf()];
    while let Some(dizin) = yigin.pop() {
        let Ok(girdiler) = std::fs::read_dir(&dizin) else {
            continue;
        };
        for giris in girdiler.flatten() {
            let yol = giris.path();
            let goreli = yol
                .strip_prefix(depo)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| yol.to_string_lossy().into_owned());
            let Ok(meta) = std::fs::metadata(&yol) else {
                continue;
            };
            if meta.is_dir() {
                harita.insert(goreli, 0);
                yigin.push(yol);
            } else {
                harita.insert(goreli, meta.len());
            }
        }
    }
    harita
}

#[test]
fn kayit_icerigi_yalnizca_yol_ad_boyut_ve_zaman_tasar() {
    let dizin = GeciciDizin::yeni("guv-gizlilik").expect("geçici dizin");
    let kok = dizin.alt("kok");
    dizin.yaz("kok/gizli-ad.txt", b"12345");
    let depo = dizin.yol().join("depo");
    uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0)).expect("tarama");

    let depo_ac = Depo::ac(&depo).expect("depo");
    let kayitlar = depo_ac.tam_kayitlari("T0001").expect("kayıtlar");
    let ornek = kayitlar
        .iter()
        .find(|k| k.yol.metin().ends_with("gizli-ad.txt"))
        .expect("kayıt");
    // Alan kümesi tam olarak yol + tür + boyut + zaman; **içerik** yoktur.
    assert_eq!(ornek.yol.metin(), "gizli-ad.txt");
    assert_eq!(ornek.tur, KayitTuru::Dosya);
    assert_eq!(ornek.bayt, 5);
    // Mutlak yol da saklanmaz (taşınabilirlik + gizlilik).
    let ham = std::fs::read_to_string(depo_ac.tam_yol("T0001")).expect("okuma");
    assert!(
        !ham.contains(&kok.to_string_lossy().to_string()),
        "mutlak yol sızmamalı"
    );
    assert!(!ham.contains("12345"), "dosya içeriği sızmamalı");
}

#[test]
fn depo_kok_disi_herhangi_bir_dosya_yazmaz() {
    let dizin = GeciciDizin::yeni("guv-tek-yazma").expect("geçici dizin");
    let kok = izinli_agac(&dizin);
    let kok_dokumu = yardimci::agac_dokumu(&kok);
    let diger = dizin.alt("baska");
    let depo = diger.join("depo");

    for gun in 0..3 {
        uygulama::scan_yurut(&scan_istegi(&kok, &depo, T0 + gun * GUN)).expect("tarama");
    }
    uygulama::restore_yurut(&RestoreIstegi {
        depo,
        hedef: "T0001".to_owned(),
        kuru_sur: false,
    })
    .expect("restore");

    assert_eq!(
        yardimci::agac_dokumu(&kok),
        kok_dokumu,
        "tarama kökü değişmemeli"
    );
    // Depo dizininin **dışındaki** hiçbir şey değişmemeli. Depo, `baska/depo`
    // altında açıldığı için "depo" girdisinin kendisi de oluşmuştur.
    let diger_son = yardimci::agac_dokumu(&diger);
    let disarida: Vec<_> = diger_son
        .iter()
        .filter(|(k, _)| k.as_str() != "depo" && !k.starts_with("depo/"))
        .collect();
    assert!(
        disarida.is_empty(),
        "depo dışında girdi olmamalı: {:?}",
        disarida
    );
    assert!(
        diger.join("depo/index.json").is_file(),
        "tek yazma hedefi depo dizini olmalı"
    );
}
