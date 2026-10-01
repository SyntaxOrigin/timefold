# Timefold — KlasörZamanı

> "Disk dolmadan önceki altı ayı gösteren, tahminle değil **farkla** konuşan yerel analiz aracı."

Timefold, bir dizin ağacını periyodik olarak tarar; her taramayı bir **zaman noktası**
olarak sürümlü bir depoya yazar. Her anlık görüntü bir bütün kayıt değil, bir önceki
anlık görüntüye göre **fark (delta) kaydıdır**; "o tarihte ne vardı" sorusu zincir
okunurken *tersine uygulanarak* çözülür. Araç "hangi klasör ne zaman büyüdü"
sorusuna **büyüme hızı + güven aralığı + varsayım listesi** ile yanıt verir.

**Güvenlik modeli:** Timefold hiçbir komutta kullanıcı dosyasını silmez, taşımaz veya
değiştirmez. Tek yazma hedefi aracın **kendi deposudur**; `--kuru-sur` bayrağıyla
hiçbir dosya ya da dizin bile oluşturulmaz. Bu davranış `tests/veri_guvenligi.rs`
içindeki 8 testle kanıtlanır (ağaç dökümü, boyut + salt-okunur bayrağı karşılaştırması).

---

## Özellikler

- **Anlık görüntü (snapshot) deposu** — özyinelemeli, sembolik bağ korumalı klasör
  gezintisi; her yol için (dosyalar için kendi boyutu, klasörler için alt ağacın
  **toplam** baytı) kayıt. İzin hataları, kırık sembolik bağlar ve dışlanan adlar
  atlanır ve **sayılarıyla birlikte raporlanır**.
- **Fark tabanlı (delta) sürümlü depo** — ilk tarama tam liste, sonrakiler yalnızca
  değişen yollar. JSONL biçimi, yoluna göre **sıralı**; fark hesabı iki kaydı bellekte
  tutarak tek geçişte yapılır.
- **Tersine uygulama** — `undo` son anlık görüntüyü arşivden geri alır, `redo`
  yeniden uygular, `restore` istenen anlık görüntüye döner. Üçü de **yalnızca aracın
  kendi arşivinde** çalışır ve `--kuru-sur` ile önceden gösterilebilir.
- **Zaman çizelgesi** — yol başına boyut geçmişi; gözlemler **kronolojik** sıralanır
  ve "ne zaman büyüdü" sorusu nokta nokta yanıtlanır.
- **Isı haritası** — `ısı skoru = |değişim baytı| × (derinlik + 1)`, beş banda
  (`soguk`…`alasin`) ayrılır. Sıralama kararlıdır (eşit skorda yol adı).
- **Tahmin** — en küçük kareler doğrusal eğilimi, **%95 güven aralığı** (Student t
  kritik değeri, `n−2` serbestlik derecesi) ve **varsayım listesi**. Veri yetersizse
  tahmin **üretilmez**: "hesaplanamadı — en az 3 gözlem gerekir, mevcut 2" yazılır.
- **Örnekli saklama** — `--tut N` son N anlık görüntüyü korur, `--gunluk` eski
  anlık görüntülerden her UTC günü için en yeni tane bırakır. Silinen bayt miktarı
  her seferinde raporlanır.
- **Örneklemeli tarama** — `--en-fazla-giris N`: dev ağaçlarda bellek tavanını korumak
  için dosyalar **içerik tabanlı** (yolun FNV-1a toplamına göre) seyreltilir. Sonuç
  tekrarlanabilirdir; klasör toplamları bir **alt sınırdır** ve ölçekleme yapılmaz.
- **Boşluk kuralı** — okunamayan anlık görüntü "veri yok" olarak işaretlenir ve araya
  **değer uydurulmaz**; bu seriden tahmin üretilmez.
- **Çıktı biçimleri** — her komut insanın okuduğu metin (varsayılan) ve makine okuyan
  JSON (`--json`) üretir; ikisi aynı veriden türetilir.
- **Sürüm uyuşmazlığı koruması** — bilinmeyen depo biçiminde araç **okumaz ve
  değiştirmez**, hata verir.

---

## Kurulum

Gereksinim: Rust **1.74** veya üstü (geliştirme ortamında `rustc 1.98.1` ile
derlendi ve test edildi). Başka hiçbir bağımlılık, araca veya çalışma zamanı
kitaplığına ihtiyaç yoktur; araç tek bir çalıştırılabilir dosyadır.

```bash
cargo build --release
```

Çıktı: `target/release/timefold.exe` (Windows) veya `target/release/timefold`
(Linux/macOS).

Yerel kurulum (ikiliyi `PATH` içine alır):

```bash
cargo install --path .
```

### Rust araç zinciri notu

Windows'ta GNU (MinGW) hedefi ile derlenirken `dlltool.exe` PATH üzerinde olmalıdır
(MinGW-w64 araç zincirinin `bin` dizini). Aksi hâlde `windows-sys` derlenirken
`error calling dlltool` hatası alınır. MSVC hedefi kullanılıyorsa bu gerekli değildir.

---

## Kullanım

Aşağıdaki **her komut gerçekten çalıştırıldı**; çıktılar birebir kopyadır.
Demo için `%USERPROFILE%\AppData\Local\Temp\timefold-demo\veri` altında şu ağaç
kuruldu (PowerShell):

```powershell
$t = "%USERPROFILE%\AppData\Local\Temp\timefold-demo"
New-Item -ItemType Directory -Path "$t\veri\belgeler","$t\veri\karsilastirmalar","$t\veri\arac" -Force | Out-Null
Set-Content "$t\veri\belgeler\rapor-ocak.txt"  -Value ("x" * 48000)  -NoNewline
Set-Content "$t\veri\belgeler\notlar.txt"      -Value ("y" * 12000)  -NoNewline
Set-Content "$t\veri\karsilastirmalar\eski.txt" -Value ("z" * 200000) -NoNewline
Set-Content "$t\veri\arac\onbellek.tmp"        -Value ("q" * 300000) -NoNewline
```

### 1) İlk tarama

```console
$ timefold scan "$t\veri" --depo "$t\depo" --haric "*.tmp"
anlık görüntü T0001 kaydedildi
  taranan: 7 giriş (3 dosya, 4 klasör)
  toplam: 253.9 KB
  fark: +7 eklendi, -0 silindi, ~0 değişti, net +253.9 KB
  atlanan: 0 izin, 0 sembolik bağ, 1 dışlama
```

`*.tmp` dosyası dışlandığı için hem dışlama sayacı artar hem de toplam düşer.
**Not:** `net` değeri yalnızca **dosya** kayıtlarının bayt değişimidir; klasör
kayıtlarının `bayt` alanı alt ağacın toplamı olduğu için katlanmaz (aksi hâlde ağaç
derinliği kadar katla sayım yapılırdı).

### 2) Ağaç değiştiğinde ikinci tarama

```powershell
Add-Content "$t\veri\belgeler\rapor-ocak.txt" -Value ("w" * 90000) -NoNewline
New-Item -ItemType Directory -Path "$t\veri\karsilastirmalar\2026" -Force | Out-Null
Set-Content "$t\veri\karsilastirmalar\2026\rapor.pdf" -Value ("p" * 450000) -NoNewline
Remove-Item "$t\veri\belgeler\notlar.txt" -Force
```

```console
$ timefold scan "$t\veri" --depo "$t\depo" --haric "*.tmp"
anlık görüntü T0002 kaydedildi
  taranan: 8 giriş (3 dosya, 5 klasör)
  toplam: 769.5 KB
  fark: +2 eklendi, -1 silindi, ~4 değişti, net +515.6 KB
  atlanan: 0 izin, 0 sembolik bağ, 1 dışlama
```

### 3) İki anlık görüntüyü karşılaştırma

```console
$ timefold diff --depo "$t\depo" --en-cok 8
fark özeti: 7 değişim kaydı (2 eklendi, 1 silindi, 4 değişti)
  net: +515.6 KB
  ~ (kök) (253.9 KB -> 769.5 KB)
  ~ belgeler (58.6 KB -> 134.8 KB)
  - belgeler/notlar.txt (son boyut 11.7 KB)
  ~ belgeler/rapor-ocak.txt (46.9 KB -> 134.8 KB)
  ~ karsilastirmalar (195.3 KB -> 634.8 KB)
  + karsilastirmalar/2026 (439.5 KB)
  + karsilastirmalar/2026/rapor.pdf (439.5 KB)
```

`+` eklendi, `-` silindi, `~` boyutu değişti. Klasör satırları da listelenir; onların
`bayt` değeri alt ağacın toplamıdır.

### 4) Yolun zaman çizelgesi

```console
$ timefold timeline --depo "$t\depo" --yol "belgeler/rapor-ocak.txt"
belgeler/rapor-ocak.txt — 2 gözlem, 46.9 KB ile 134.8 KB arası
  2026-09-29T09:25:48Z     46.9 KB          +0 B  T0001
  2026-09-29T09:25:48Z    134.8 KB      +87.9 KB  T0002
```

Zaman damgaları **RFC 3339 UTC**'dir. İki tarama aynı saniyede yapıldığı için
damgalar aynıdır; gözlemler yine de kronolojik sırada listelenir.

### 5) Isı haritası

```console
$ timefold heatmap --depo "$t\depo" --en-cok 6
ısı haritası — 7 değişen yol
  bant         derinlik     önceki       yeni      fark     skor
  alasin             3        0 B   439.5 KB +439.5 KB  1800000
      karsilastirmalar/2026/rapor.pdf
  cok                2        0 B   439.5 KB +439.5 KB  1350000
      karsilastirmalar/2026
  sicak              1   195.3 KB   634.8 KB +439.5 KB   900000
      karsilastirmalar
  sicak              0   253.9 KB   769.5 KB +515.6 KB   528000
      (kök)
  ilik               2    46.9 KB   134.8 KB  +87.9 KB   270000
      belgeler/rapor-ocak.txt
  ilik               1    58.6 KB   134.8 KB  +76.2 KB   156000
      belgeler
  ... ve 1 yol daha
```

Aynı büyüme (439.5 KB) üç seviyede de görünüyor; derin olanın skoru 2 kat fazla çünkü
`skor = |fark| × (derinlik + 1)`. Bu, "alt klasörün büyümesi yeni bilgidir" sezgisini
sayısallaştırır.

### 6) Depodaki anlık görüntüler

```console
$ timefold list --depo "$t\depo"
2 anlık görüntü:
  T0001  2026-09-29T09:25:48Z           7 kayıt  tam
  T0002  2026-09-29T09:25:48Z           8 kayıt  tam
2 anlık görüntü, 4 dosya, 3.6 KB (undo: var)
```

### 7) Tahmin ve güven aralığı

Varsayılan saatle tarama yapılırsa iki anlık görüntü saniyeler arayla düşer ve
eğilim anlamsızlaşır. Bu yüzden `--zaman` ile **UTC unix saniye** verilerek günlük
bir seri kurulur. Demo: `arsiv.dat` her gün 500 KB büyüyen bir dosya.

```powershell
$t = "%USERPROFILE%\AppData\Local\Temp\timefold-tahmin"
New-Item -ItemType Directory -Path "$t\veri" -Force | Out-Null
Set-Content "$t\veri\arsiv.dat" -Value ("x" * 1000000) -NoNewline
$b = 1791302400; $g = 86400
foreach ($i in 0..7) {
  if ($i -gt 0) { Add-Content "$t\veri\arsiv.dat" -Value ("y" * (500000 * $i)) -NoNewline }
  timefold scan "$t\veri" --depo "$t\depo" --zaman ($b + $i * $g)
}
```

`1791302400` = `2026-10-06T16:00:00Z`; `86400` saniye = bir gün.

```console
$ timefold timeline --depo "$t\depo2" --yol "arsiv.dat"
arsiv.dat — 8 gözlem, 976.6 KB ile 14.3 MB arası
  2026-10-06T16:00:00Z    976.6 KB          +0 B  T0001
  2026-10-07T16:00:00Z      1.4 MB     +488.3 KB  T0002
  2026-10-08T16:00:00Z      2.4 MB     +976.6 KB  T0003
  2026-10-09T16:00:00Z      3.8 MB       +1.4 MB  T0004
  2026-10-10T16:00:00Z      5.7 MB       +1.9 MB  T0005
  2026-10-11T16:00:00Z      8.1 MB       +2.4 MB  T0006
  2026-10-12T16:00:00Z     11.0 MB       +2.9 MB  T0007
  2026-10-13T16:00:00Z     14.3 MB       +3.3 MB  T0008
```

```console
$ timefold forecast --depo "$t\depo2" --yol "arsiv.dat" --ileri-gun 30
arsiv.dat — 8 gözlem, 6 serbestlik derecesi
  eğilim: +1.9 MB / gün
  tahmin (2026-11-12T16:00:00Z tarihi, 30 gün sonra): 69.9 MB
  %95 güven aralığı: 53.6 MB .. 86.1 MB
  varsayımlar:
    - doğrusal eğilim varsayıldı (bayt/gün = 2000000.00)
    - 8 gözlem, 6 serbestlik derecesi
    - 95% güven aralığı, Student t kritik değeri ≈ 2.447
    - artıklar normal dağılım varsayıldı; ani artışlar (twinning spike) aralığı genişletmez
```

Görüldüğü gibi eğilim tam olarak günlük 2.000.000 baytı yakalıyor ve tahmin
`14,3 MB + 30 × 2 MB = 74 MB` mertebesinde. Güven aralığı **asla doldurulmaz**;
hesaplanamıyorsa "hesaplanamadı" yazılır:

```console
$ timefold forecast --depo "$t\depo" --yol "belgeler/notlar.txt" --ileri-gun 30
belgeler/notlar.txt — hesaplanamadı — en az 3 gözlem gerekir, mevcut 1
```

### 8) Undo / redo / restore

```console
$ timefold undo --depo "$t\depo"
undo T0008 — 876 B uygulandı
$ timefold redo --depo "$t\depo"
redo T0008 — 876 B uygulandı
$ timefold restore --depo "$t\depo" --hedef T0002 --kuru-sur
T0002 noktasına dönüldü; silinen: T0003, T0004, T0005, T0006, T0007, T0008 (5.1 KB; kuru çalıştırma, uygulanmadı)
```

`restore --kuru-sur` **hiçbir şey silmez**; yalnızca ne silineceğini söyler.

### 9) JSON çıktısı

```console
$ timefold scan "$t\veri" --depo "$t\depo2" --haric "*.tmp" --kuru-sur --json
{
  "anlik_goruntu": "T0000",
  "atlanan_dislama": 1,
  "atlanan_izin": 0,
  "atlanan_sembolik": 0,
  "dosya": 3,
  "fark": {
    "bozuk_satir": 0,
    "degisti": 0,
    "eklendi": 8,
    "net_bayt": 788000,
    "silindi": 0
  },
  "gerekce": "",
  "kesilen": 0,
  "kismi": false,
  "klasor": 5,
  "kok": "C:\\Users\\xXx\\AppData\\Local\\Temp\\timefold-demo\\veri",
  "komut": "scan",
  "orneklendi": false,
  "saklama": null,
  "taranan": 8,
  "toplam_bayt": 788000,
  "uygulandi": false,
  "yazilan": 8,
  "zaman": "2026-09-29T09:25:48Z"
}
```

`depo2` dizini bu komuttan sonra **hâlâ yoktur** — `--kuru-sur` sözünün tam karşılığı:

```console
$ Test-Path "$t\depo2"
False
```

JSON nesneleri `serde_json::Value` üretir ve **anahtarlar alfabetik** sıralanır.

---

## Test

```bash
cargo test
```

Gerçek çıktı (Rust 1.98.1, Windows 11, x86_64-unknown-windows-gnu):

```text
running 154 tests
test result: ok. 154 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.99s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 22 tests
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.71s

running 8 tests
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

Doc-tests timefold
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Toplam: 184 test, 0 başarısız** (154 birim + 22 entegrasyon + 8 veri güvenliği).

Diğer kalite kapıları:

```bash
cargo build --release                                  # hatasız
cargo clippy --all-targets -- -D warnings             # uyarısız
cargo fmt --check                                      # fark yok
```

### Kapsanan senaryolar (edge case'ler)

| Senaryo | Nerede |
|---|---|
| Boş klasör (tek kayıt, 0 bayt) | `tarama::tests::bos_klasor_tek_kayit_uretir` |
| Tek dosya | `tarama::tests::tek_dosya_boyutu_dogru` |
| İç içe derin ağaç, klasör toplamları | `tarama::tests::ic_ice_derin_agac_toplamlari_toplaniyor` |
| Gizli (nokta) dosyalar | `tarama::tests::gizli_dosyalar_varsayilan_taranir`, `entegrasyon::gizli_dosyalar_...` |
| Sembolik bağ izlenmiyor | `tarama::tests::sembolik_bag_izlenmiyor` |
| Sembolik bağ döngüsü / derinlik sınırı | `tarama::tests::derinlik_siniri_yarim_tarama_olusum_tesbit_ediliyor` |
| İzin hatası atlanıyor | `tarama::tests::gecersiz_kok_hata_donduruyor` (okunamayan kök) + `atlanan_izin` sayacı; sınıflandırma `delta::karsilastir` içinde |
| Kısmi tarama | `entegrasyon::kismi_tarama_basinlikta_isaretlenir` |
| Silinen dosya ve klasör | `delta::tests::silinen_dosya_negatif_etki_uretir`, `entegrasyon::silinen_dosya_ve_klasor_fark_uretiyor` |
| Değişen dosya | `delta::tests::degisen_dosya_fark_uretir` |
| Eklenen dosya | `delta::tests::ilk_taramada_her_sey_eklenir` |
| Aynı boyut, farklı içerik | `entegrasyon::ayni_boyut_farkli_icerik_yalnizca_zaman_damgasi_ile_ayirt_edilir` |
| Anlık görüntü farkı boş | `delta::tests::anlik_goruntu_farki_bos_kalabilir`, `entegrasyon::iki_kez_ayni_tarama_bos_fark_uretir` |
| Sürüm uyuşmazlığı (indeks + anlık görüntü) | `depo::tests::surum_uyusmazligi_hata_donduruyor`, `..._ile_olmayan_depo_dosyalari_corrupt_olmaz`, `entegrasyon::surum_uyusmazligi_...` |
| Bozuk JSONL satırı | `delta::tests::bozuk_satir_atlanip_sayiliyor`, `entegrasyon::bozuk_jsonl_satiri_atlanir_ve_sayilir`, `depo::tests::depodaki_baslik_satiri_bozuk_kayit_sayilmaz` |
| Eksik anlık görüntü → boşluk | `zamancizelgesi::tests::eksik_dosya_bosluk_olusum_tesbit_ediliyor`, `entegrasyon::eksik_anlik_goruntu_dosyasi_bosluk_olusum_tesbit_ediliyor` |
| Tersine uygulama (içerik + izin) | `veri_guvenligi::undo_redo_restore_kullanici_dosyalarina_dokunmuyor`, `veri_guvenligi::salt_okunur_dosya_korunur` |
| `--kuru-sur` | `entegrasyon::..._kuru_surma_hicbir_sey_yazmaz`, `veri_guvenligi::kuru_surma_kullanici_dosyalarina_ve_dosya_sistemine_dokunmuyor`, `veri_guvenligi::kuru_surma_aclari_birakmaz` |
| İki kez aynı tarama | `tarama::tests::iki_kez_tarama_ayni_sonucu_uretir` |
| Zaman çizelgesi sıralaması | `zamancizelgesi::tests::zaman_cizelgesi_kronolojik_sirali` |
| Tahmin güven aralığı | `tahmin::tests::guven_araligi_tahmini_kapsiyor`, `..._hesaplanamaz`, `..._bosluklu_seride_tahmin_uretilmez` |
| Örnekleme | `tarama::tests::ornekleme_giris_sinirini_uzuyor`, `entegrasyon::ornekleme_klasor_toplamlarini_alt_sinir_yapiyor` |
| Arşiv dışlama | `tarama::tests::arsiv_dislama_deseni_uygulaniyor`, `entegrasyon::dislama_deseni_taramadan_cikariliyor` |
| JSON şema gidiş-dönüşü | `kayit::tests::tam_kayit_json_gidis_donusu_korunuyor`, `..._degisim_turleri_...`, `entegrasyon::json_ve_metin_ciktisi_ayni_sayi_lari_tasir` |
| Depo dizini taramaya dahil edilmez | `entegrasyon::depo_dizini_taramaya_dahil_edilmez` |
| Kimlik çakışmaması (saklama sonrası) | `entegrasyon::kimlikler_saklama_sonrasi_yeniden_kullanilmaz` |
| Gündelik seyreltme | `saklama::tests::*`, `entegrasyon::gunluk_seyreltme_her_gun_bir_tane_koruyor` |
| Gizlilik: içerik/mutlak yol sızmaz | `veri_guvenligi::kayit_icerigi_yalnizca_yol_ad_boyut_ve_zaman_tasar` |

### "Veri silinmez" kuralı nasıl test ediliyor?

`tests/veri_guvenligi.rs` bir **ağaç dökümü** yardımcısı kullanır: tarama kökü özyinelemeli
 gezilir ve `göreli yol → (bayt, salt-okunur bayrağı)` haritası üretilir. Komutlar
çalıştırıldıktan sonra aynı döküm alınır ve `assert_eq!` ile **birebir** karşılaştırılır.

- **Boyut *ve* izin birlikte** karşılaştırıldığı için "içerik aynı ama izin değişti"
  gibi sinsi ihlaller de yakalanır.
- Salt okunur bir dosya fixture'ı (`okunur/korunacak.txt`) oluşturulur; `scan`,
  `undo`, `redo`, `restore` çalıştırıldıktan sonra dosyanın hâlâ salt okunur ve
  baytının değişmediği ayrıca doğrulanır.
- `--kuru-sur` testleri, depo dizininin **hiç oluşmadığını** (`assert!(!depo.exists())`)
  doğrular; "hiçbir şey yazma" sözünün tam karşılığı budur.
- Okuma komutlarının **arşivi de** değiştirmediği ayrı bir dökümle kanıtlanır.
- `kayit_icerigi_yalnizca_yol_ad_boyut_ve_zaman_tasar` testi, arşiv dosyasının ham
  metninde dosya içeriğinin ve mutlak yolun **sızmadığını** doğrular.

---

## Proje Yapısı

```text
09-timefold/
├── Cargo.toml
├── Cargo.lock                 (üretilir, commit edilir)
├── LICENSE.txt
├── README.md
├── .gitignore
├── src/
│   ├── main.rs                CLI kabuğu (yalnızca argüman ayrıştırma + çıktı)
│   ├── lib.rs                 modül dizini, #![forbid(unsafe_code)], #![deny(missing_docs)]
│   ├── hata.rs                Hata enum'u + Display/Error (elle yazıldı)
│   ├── yol.rs                 Yol (bileşen sıralı), joker dışlama, gizli dosya kuralı
│   ├── zaman.rs               RFC 3339 UTC üretimi/ayrıştırma, ay/gün anahtarları
│   ├── kayit.rs               AnlikGoruntuBasi, TamKayit, Degisim, FNV-1a Saglama
│   ├── akis.rs                SatirOkuyucu, AkisYazici, harici Sirala, Birlestir
│   ├── tarama.rs              özyinelemeli gezgin, sembolik bağ koruması, örnekleme
│   ├── depo.rs                Depo indeksi, anlık görüntü yazma/okuma, undo/redo/restore
│   ├── delta.rs               akış tabanlı karşılaştırma (merge-join), delta::Kaynak
│   ├── zamancizelgesi.rs      yol başına geçmiş, kronoloji, boşluk işaretleme
│   ├── isiharitasi.rs         ısı skoru, bantlama, kararlı sıralama
│   ├── tahmin.rs              en küçük kareler, güven aralığı, varsayım listesi
│   ├── saklama.rs             SaklamaPolitikasi (--tut / --gunluk)
│   ├── rapor.rs               metin + JSON biçimlendirme
│   └── uygulama.rs            komut yürütücü (tüm yazma burada toplanır)
└── tests/
    ├── yardimci.rs            geçici dizin + ağaç dökümü yardımcıları
    ├── entegrasyon.rs         22 uçtan uca test
    └── veri_guvenligi.rs      8 veri güvenliği testi
```

Modül bağımlılık yönü tek yönlüdür: `hata ← yol ← zaman ← kayit ← akis ← tarama ←
delta ← depo ← {zamancizelgesi, isiharitasi} ← {tahmin, saklama} ← rapor ← uygulama ← main`.
Döngüsel bağımlılık yoktur.

---

## Yapılandırma

Timefold'in **yapılandırma dosyası yoktur**; tüm ayarlar komut satırı bayraklarıdır.
Bu, "yapılandırma program dizininde, salt okunur ortamda çalışabilir" taşınabilirlik
gereksinimini en güçlü biçimde karşılar: ayar dosyası aranmaz, dolayısıyla bulunamaz.

| Bayraç | Komut | Varsayılan | Etkisi |
|---|---|---|---|
| `<KOK>` | `scan` | — | Taranacak kök dizin (konumsal). |
| `--depo <DIZIN>` | tümü | `timefold-depo` | Anlık görüntü deposunun dizini. **Tek yazma hedefidir.** |
| `--haric <DESEN>` | `scan` | — | Dışlanacak joker desen; birden çok kez verilebilir. `*` ve `?` tek ad bileşeninde, `**` sınır tanımaz. Büyük/küçük harf duyarsız. |
| `--gizli-haric` | `scan` | kapalı | Nokta ile başlayan adları (`.git`, `.cache`, …) taramadan çıkarır. |
| `--derinlik <SEVIYE>` | `scan` | `64` | İzlenecek en derin seviye (kök = 1). Aşan ağaç kısmi olarak işaretlenir. Sembolik bağ döngüsü koruması. |
| `--en-fazla-giris <ADET>` | `scan` | yok | Bu sayıdan fazla **dosya** varsa örnekleme devreye girer (içerik tabanlı seyreltme). Klasörler her zaman saklanır; toplamlar alt sınırdır. |
| `--tut <ADET>` | `scan` | yok | Korunacak en yeni anlık görüntü sayısı. |
| `--gunluk` | `scan` | kapalı | `--tut` penceresinin dışındaki anlık görüntülerden her UTC günü için en yeni tane bırakır. |
| `--kuru-sur` | `scan`, `undo`, `redo`, `restore` | kapalı | Hiçbir şey yazmadan ne olacağını gösterir. `scan`de depo henüz yoksa **hiçbir dosya/dizin oluşmaz**. |
| `--zaman <UNIX_SANIYE>` | `scan` | sistem saati | Anlık görüntünün zaman damgasını elle verir. Geriye dönük seri kurmak ve testler için. |
| `--onceki <KIMLIK>` | `diff` | son iki | Karşılaştırılacak önceki anlık görüntü. |
| `--en-cok <ADET>` | `diff`, `heatmap` | `20` | Listede gösterilecek en fazla kayıt/satır. |
| `--yol <YOL>` | `timeline`, `forecast` | — | İncelenecek **göreli** yol (`/` veya `\` ayraçlı). |
| `--baslangic <KIMLIK>` | `heatmap` | son iki | Isı haritasının başlangıç anlık görüntüsü. |
| `--altina <YOL>` | `heatmap` | yok | Sonuçları bu üst yolün altına indirir ve skorları **yeniden** bantlar. |
| `--ileri-gun <GUN>` | `forecast` | `30` | Kaç gün sonrası tahmin edileceği. |
| `--json` | tümü | kapalı | Metin yerine makine okunabilir JSON yazar. |

### Depo biçimi

```text
<depo>/
  index.json                              sürüm, sayaç, kimlik listesi
  anlik_goruntuler/
    T0001.tam.jsonl                       1. satır başlık + tüm kayıtlar
    T0001.fark.jsonl                      1. satır başlık + tüm değişimler
    T0002.tam.jsonl
    T0002.fark.jsonl
  geri/                                   undo edilen anlık görüntüler (redo yığını)
```

Anlık görüntü başlığı: şema ayracı, biçim sürümü, kimlik, temel kimlik, zaman damgası,
taranan kök yolu, kayıt sayısı, FNV-1a sağlama toplamı, tür (`tam`/`fark`), kısmi
tarama bayrağı ve gerekçesi, örnekleme bayrağı, görülen toplam giriş sayısı.

Kayıt satırı: `{"yol":"a/b.txt","tur":"dosya","bayt":123,"degisiklik":1759080000}`.
Değişim satırı: `{"cesit":"degisti","yol":"a/b.txt","tur":"dosya","onceki_bayt":10,"yeni_bayt":25}`.

Depo **mutlak yol saklamaz**; yalnızca kök dizinin o anki mutlak yolu `index.json`
içinde tutulur (taşınabilirlik uyarısı için). Başka makineye kopyalanan depo
istatistik sorgulanabilir; tarama yapmak için `--depo` ile yeni bir kök verilmelidir.

---

## Bilinen Sınırlamalar

**Zorunlu ve dürüst liste. Bu bölüm eksiksiz okunmalıdır.**

1. **İçerik değişimi, aynı boyutta görünmez.** Değişim tespiti yalnızca **boyut**
   ve **mtime** üzerinden yapılır. Aynı uzunlukta farklı içerikle değiştirilen bir
   dosya `diff` çıktısında görünmez (`entegrasyon::ayni_boyut_farkli_icerik_...`
   testi bunu bilinçli olarak sabitler). Karma (hash) kullanmak yerine
   `WORKER_CONTRACT.md` § 3.2 gereği 09 için kripto/karma crate'i verilmemiştir.
2. **Yeniden adlandırma sınıflaması üretilmez.** `Degisim::AdDegisti` şemada durur
   ama `delta` modülü onu **üretmez**: dosya sistemi "taşındı" bilgisini vermez ve
   aynı boyutlu silme+ekleme çiftini taşınmış saymak uydurma cevap olurdu. Bu, raporun
   "boşluğu doldurma yasağı" ile aynı ilkedir. Gerçek bir kanıt kaynağı (USN/inotify)
   eklendiğinde devreye alınabilir.
3. **Bildirim yüzeyi yoktur.** USN Journal / inotify / FSEvents kullanılmaz; araç
   periyodik tam tarama yapar. Saniyeler içinde değişen dosyalar iki tarama arasında
   kaçırılır. `MANIFEST.md` kartında da ertelenmiştir. Varsayılan tarama aralığı
   **raporun önerdiği 300 saniyedir**; ancak aracın kendisi bir zamanlayıcı
   çalıştırmaz — aralığı dışarıdan (Görev Zamanlayıcı, cron, CI) sağlamak gerekir.
   Bu bir bilinçli sadeleştirmedir: raporun kendi ifadesiyle "tümü olmadan da çalışır".
4. **Sıkıştırma yoktur.** Sözlük sıkıştırma yerine düz JSONL kullanılır; depo boyutu
   raporun önerisine göre daha büyüktür. Azaltma `--tut` / `--gunluk` ile yapılır ve
   kazanılan bayt her seferinde raporlanır. `MANIFEST.md` kartında da ertelenmiştir.
5. **Örneklemede tahmin üretilmez.** `--en-fazla-giris` aşıldığında klasör toplamları
   bir **alt sınırdır**; ölçekleme yapılmaz, çünkü ölçeklemek gözlem olmayan bir
   tahmin üretmek anlamına gelirdi. Sonuç `orneklendi: true` ve açıklayıcı gerekçe ile
   işaretlenir. Örnekleme **iki geçişte** yapılır (önce sayım, sonra seyreltme), bu
   yüzden `read_dir` maliyeti ikiye katlanır.
6. **Örnekleme yalnızca dosyalara uygulanır.** Klasörler her zaman saklanır; aksi
   hâlde ağaç yapısı bozulurdu.
7. **Tahmin doğrusaldır.** Ani artışları (tweaking/twinning spike) yakalamaz; güven
   aralığı da model yanlışlığını ölçmez, yalnızca **artıkların** dağılımından gelen
   belirsizliği ölçer. Student t kritik değerleri `n−2` serbestlik derecesi için
   **elle tutulan bir tablodur**; `n > 32` için normal yaklaşımı (1.959964) kullanılır.
   Alt sınır negatif çıkabilir; bu durum varsayım listesinde açıkça bildirilir.
8. **En az 3 gözlem gerekir.** Raporın kabul kriteri 6 anlık görüntü ister; MVP'de
   eğilim + güven aralığı için matematiksel alt sınır 3'tür. 3–5 gözlem aralığında
   tahmin üretilir ama "güven aralığı geniştir, tahmin kararlı değildir" uyarısı
   varsayım listesine otomatik eklenir. 3'ten az gözlemde **hiç tahmin üretilmez**.
9. **Delta tek yönlüdür.** `T5`'i okumak için `T1..T4` okunmalıdır. Bunu güvenli
   kılmak için **her** anlık görüntünün tam kopyası da saklanır; arşivleme bir ara
   anlık görüntüyü kaldırdığında zincir kırılmasın diye. Bu, raporun "zaman-mekân
   takası" kararının **bilinçli maliyetidir** ve depo boyutunu ikiye yaklaşık iki katına
   çıkarır. Fark dosyası okuma hızını, tam kopya sürekliliği sağlar.
10. **Bozuk kayıtlar atlanır, sayılır.** Çözümlenemeyen JSONL satırı hata değildir;
    atlanır ve `bozuk_satir` / `uyarı:` ile bildirilir. Bu, güç kesintisiyle yarım
    kalan bir kaydın tüm zinciri okunamaz yapmasını önler; bedeli, bozuk kaydın
    içeriğinin **hiç bilinmemesidir**.
11. **Eksik anlık görüntü boşluk üretir.** Dosya silinmişse o tarih "veri yok" olarak
    işaretlenir ve o seriden tahmin üretilmez. Ara **araya değer uydurmaz** — bu,
    projedeki en güçlü ve en pahalı ilkedir.
12. **FNV-1a bir sağlama toplamıdır, kimlik doğrulama (MAC) değildir.** Hızlı ve
    kriptografik değildir; yalnızca kayıt sırasının/icerik değişip değişmediğini
    yakalamak içindir. 09 için kripto crate'i verilmediği için bu bilinçli bir
    seçimdir ve **güvenlik sınırı olarak kullanılmaz**.
13. **Biçim sürümü uyuşmazlığında veri okunmaz.** Araç hata verir ve dosyaya dokunmaz.
    Bu, eski bir depoyu yanlışlıkla bozmaktır ama ileri sürümle açılan depo için
    geçmiş okunamaz hale gelir.
14. **UTF-8 dışı dosya adları kaybolur.** Windows'ta geçersiz UTF-16 dizileri
    `U+FFFD` ile değiştirilir ve `yol_kaybi` sayacı artar; bu yollar arasında çakışma
    teorik olarak mümkündür.
15. **Yol bağımsızlığı tam değildir.** Depo göreli yollar saklar, ancak `index.json`
    içindeki `kok` alanı mutlaktır. Depo başka makineye taşındığında tarama yapmak için
    kök yeniden verilmelidir; istatistik sorgulanabilir.
16. **Soket / FIFO / cihaz dosyaları atlanır** ve dışlama sayacına yazılır; boyut
    bilgileri anlamlı değildir.
17. **`scan` sıralı çıktıyı dış diskte sıralar** (koşu dosyaları + k-yollu
    birleştirme). Çok büyük ağaçlarda geçici disk kullanımı bir taramanın toplam
    veri miktarına yaklaşabilir. `AKIS::SIRALAMA_TAMPONU` (65.536 satır) bellek
    tavanını belirler.
18. **Tek iş parçacığı, eşzamanlılık yok.** Raporun "tavan 8 tarama iş parçacığı"
    önerisi uygulanmadı; I/O darboğazı tek akışla sınırlıdır. Delta hesabının
    sıralı olması bir zorunluluk olarak korunmuştur.
19. **Aynı depoya eşzamanlı yazma güvenli değildir.** Depoda kilit dosyası yoktur;
    iki `scan` aynı anda aynı `--depo` üzerinde çalışırsa geçici dosyalar
    (`<depo>/gecici/`) çakışabilir. Arşiv okuma komutları (`diff`, `timeline`,
    `heatmap`, `forecast`, `list`) yazma yapmadığı için güvenle eşzamanlı
    çalıştırılabilir.
19. **`clippy::expect_used` / `clippy::unwrap_used` yalnızca test kodunda** kullanılır
    (`#[cfg(test)] mod tests` üzerinde açık `#[allow]`). Üretim kodunda hiç
    `unwrap`/`expect`/`panic!` yoktur; tüm hatalar `Sonuc<T, Hata>` ile döner.
20. **Sembolik bağ oluşturma ortam yetkisine bağlıdır.** Windows'ta sembolik bağ
    kurmak yönetici veya Geliştirici Modu ister. Test, bağ oluşturulamazsa "bağ yok"
    durumuyla da geçerli olacak şekilde yazılmıştır (her iki durumda da dosya
    sayısının 1 olması beklenir). Bağ döngüsü korumasının **belirlenimci** kanıtı
    `--derinlik` sınırı testidir.

### `#[allow]` kullanımı

| Konum | Gerekçe |
|---|---|
| `#[allow(clippy::expect_used, clippy::unwrap_used)]` — her `mod tests` | `WORKER_CONTRACT.md` § 4.2: `unwrap`/`expect` yalnızca testlerde serbest. Bir testin `expect` çağrısının başarısız olması, iddianın yanlış olduğunun en net sinyalidir. |
| `#[allow(dead_code)]` — `tests/yardimci.rs` | Yardımcı modül iki test ikilisine eklenir; her ikisi de yalnızca bir kısmını kullanır. |
| `#[allow(dead_code)]` — `Degisim::AdDegisti::bayt` alanı | Varyant şemada durur ama üretilmez (bkz. sınır 2); alan okunmadığı için uyarı üretir. |
| `#[allow(clippy::fn_params_excessive_bools)]` — `src/main.rs` | `clap` tarafından üretilen alt komut çeşidi; bayrak sayısı aracın doğal arayüzüdür. |

### `Drop` içinde hata yutma

`tests/yardimci.rs` ve `src/akis.rs` içindeki `Drop` uygulamaları temizlik hatasını
`let _ = ...` ile **sessizce yutar**. Bu, sözleşmenin "sessiz yutma yasağına" yegdir:
`Drop` içinden hata döndürülemez ve testin sonucu, geçici klasörün silinip
silinmediğine bağlı olmamalıdır. Kalıcı kod yolunda (`is.tarama_ekle`,
`in.undo`, `in.geri_al_hedefe`, `in.saklama_uygula`) hiçbir hata yutulmaz; hepsi
`Sonuc` ile döner.

---

## Gelecek Geliştirmeler

`MANIFEST.md` kartında ertelenenler ve doğal sonraki adımlar:

1. **Bildirim tabanlı hızlandırma** — USN Journal (Windows) / inotify (Linux) /
   FSEvents (macOS) ile yalnızca değişen alt ağaçların yeniden taranması. Bu, v1'in
   ana başlığı; sıfır kayıp garantisi sağlanır.
2. **Gerçek yeniden adlandırma tespiti** — bildirim yüzeyi geldiğinde
   `Degisim::AdDegisti` güvenilir biçimde doldurulabilir.
3. **İçerik karması ile değişim tespiti** — aynı boyutlu içerik değişimlerini
   yakalamak için. Bağımlılık politikası izin verirse (bkz. sınır 1).
4. **Delta zincirinin sıkıştırılması** — fark dosyaları yüksek oranda tekrar
   içerdiğinden (yol havuzu) sözlük sıkıştırma depo boyutunu belirgin azaltır.
5. **Tahmin modeli seçimi** — ani artışları yakalayan dayanıklı (robust) model ve
   kullanıcı seçimi. Raporın açık sorularından biri.
6. **Aylık kümeleme** — "2026-03 ayı ne kadardı?" sorusunu tek sorguyla yanıtlamak
   için (raporun `Series.points[]` kavramı). Şu an günlük zaman çizelgesi var.
7. **HTML/SVG dışa aktarım** — raporun v2 aşamasındaki tek dosya HTML raporu. Terminal
   + statik HTML/SVG çıktısı bu yönelim çerçevesinde kabul edilen biçimdir.
8. **Denetim izi** — her işlemin zamanı, kapsamı ve serbest kalan baytı kalıcı kayıt.
9. **Yapılandırma dosyası** — saklama politikası ve izlenen kökler için JSON dosyası
   (rapor b07 kararı). Şu an yalnızca bayraklar var; bu, kasıtlı bir sadeleştirmedir.
10. **İlerleme çubuğu ve iptal** — çok büyük ağaçlarda `--iptal` bayrağı.

---

## Troubleshooting

### 1) `error: could not compile 'windows-sys' ... error calling dlltool 'dlltool.exe': program not found`

**Belirti:** `cargo build --release` MinGW/GNU hedefiyle derlenirken başarısız olur;
`cargo build` (debug) çalışırken `--release` çalışmaz.

**Neden:** GNU hedefi, bağımlılıkları bağlamak için `dlltool.exe` gerektirir ve bu
araç MinGW-w64 kurulumunun `bin` dizinindedir; bu dizi PATH'te değildir.

**Çözüm:** MinGW-w64 `bin` dizinini PATH'e ekleyin, ya da MSVC hedefine geçin:

```powershell
$env:PATH = "%USERPROFILE%\.cargo\bin;C:\...\mingw64\bin;" + $env:PATH
```

### 2) `hata: depo okunamadı (...): index.json bulunamadı`

**Belirti:** `diff`, `timeline`, `heatmap`, `forecast`, `list`, `undo`, `redo` veya
`restore` "depo okunamadı" hatası verir.

**Neden:** `--depo` ile verilen dizinde henüz `index.json` yoktur; yani o dizin için
hiç tarama yapılmamıştır. Depo dizini **ilk `scan` ile** oluşur.

**Çözüm:** Önce bir tarama yapın ve **aynı** `--depo` yolunu kullanın:

```bash
timefold scan "/veri" --depo "C:/timefold-depo"
timefold list --depo "C:/timefold-depo"
```

`scan` varsayılan depo yolu `timefold-depo` (çalışma dizininde) olduğu için, farklı
dizinlerde çalışırken `--depo` bayrağını her komutta **aynı** değerle vermek gerekir.

### 3) `hata: biçim sürümü uyuşmuyor (...): dosyada 7, beklenen 1`

**Belirti:** Depo başlığındaki `surum` alanı aracın anladığı sürümden farklıdır.

**Neden:** Depo ya daha yeni bir sürümle, ya da el ile düzenlenerek yazılmıştır.
Araç bilerek **okumaz ve değiştirmez**; bu, kullanıcı verisinin ve geçmişin
korunması içindir.

**Çözüm:** Aracın doğru sürümüyle açın, ya da arşivin yedekli olduğundan emin
olup depoyu silip yeni bir tarama başlatın. Silme işlemi **yalnızca sizin
kontrolünüzde** yapılmalıdır — Timefold bunu kendiliğinden yapmaz.

### 4) `diff` boş görünüyor, oysa dosyalar değişti

**Belirti:** `timefold diff` "fark yok" der, ama içerik değişmiştir.

**Neden:** İki olasılık. (a) Değişiklik aynı **boyutu** koruyor — Timefold boyut ve
mtime'e bakar, içeriğe bakmaz (bkz. Bilinen Sınırlamalar 1). (b) İki tarama aynı
saniyede yapıldı ve mtime çözünürlüğü değişimi yakalayamadı.

**Çözüm:** Önce `list --depo` çıktısında iki anlık görüntünün **farklı zaman
damgalarına** sahip olduğunu doğrulayın. Zaman damgaları aynıysa
`--zaman <UNIX_SANIYE>` ile ayrı damgalar verin. Boyut korunmuşsa bu bir araç
sınırıdır ve yalnızca içerik karmasıyla çözülür.

### 5) `forecast` her zaman "hesaplanamadı" diyor

**Belirti:** `timefold forecast --yol X` "en az 3 gözlem gerekir, mevcut 1" yazar.

**Neden:** Zaman çizelgesinde yol için **en az 3** anlık görüntüde ölçüm olmalıdır.
Yeni eklenen bir dosya yalnızca eklendiği andan itibaren geçmişi vardır. Ayrıca bir
anlık görüntünün dosyası **eksikse** (elle silinmişse) o tarih boşluktur ve seri
tümüyle tahmin dışı bırakılır.

**Çözüm:** En az 3 `scan` çalıştırın. `list` çıktısında hiç `kısmi`/bozuk satır
olmadığını doğrulayın. Boşluk varsa `timeline` çıktısındaki `BOŞLUK` işaretlerini
takip edin; eksik dosya `undo`/`restore` ile arşivden geri getirilebilir.

### 6) Tarama çok yavaş / depo çok büyük

**Belirti:** `scan` uzun sürüyor, depo diski dolduruyor.

**Neden:** Her anlık görüntünün hem fark hem tam kopyası saklanır (zincir sürekliliği
için) ve varsayılan olarak geçmiş sınırsız büyür. Ayrıca sıralama geçici diskte
koşu dosyaları kullanır.

**Çözüm:**

```bash
# Gürültüyü dışla (caches, geçici dosyalar)
timefold scan "/veri" --depo "D" --haric "*.tmp" --haric "**/node_modules/**"

# Geçmişi kısalt: son 30 anlık görüntüyü tut
timefold scan "/veri" --depo "D" --tut 30

# Günlük kapsamayı da koru
timefold scan "/veri" --depo "D" --tut 30 --gunluk

# Çok büyük ağaçta bellek tavanını koru
timefold scan "/veri" --depo "D" --en-fazla-giris 200000
```

Her `scan` çıktısındaki `kazanılan bayt` değeri, arşivlemede ne kadar yer açıldığını
gösterir. **Saklama yalnızca aracın kendi deposunda** uygulanır.

---

## Atıflar

- **RFC 3339 — Date and Time on the Internet: Timestamps**, <https://www.rfc-editor.org/rfc/rfc3339>
  — Depodaki zaman damgalarının metinsel biçimi (`2026-09-29T13:05:07Z`).
- **RFC 8259 — The JavaScript Object Notation (JSON) Data Interchange Format**,
  <https://www.rfc-editor.org/rfc/rfc8259> — Anlık görüntü kayıtlarının ve `--json`
  çıktısının veri biçimi.
- **Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms"**,
  <https://howardhinnant.github.io/date_algorithms.html> — `src/zaman.rs` içindeki
  proleptik Gregoryen gün sayısı ↔ takvim dönüşümünün (public domain) referansı.
- **GNU Coreutils — `du`**, <https://www.gnu.org/software/coreutils/manual/html_node/du-invocation.html>
  — Özyinelemeli disk kullanımı hesabının klasik referansı; "hangi klasör ne kadar
  büyük" sorusunun atası.
- **libgit2**, <https://libgit2.org/> — Fark tabanlı, sürümlü depolama modelinin
  referans uygulaması. Timefold'un "zaman-mekân takası" kararı bu modelden esinlenir,
  ancak karma/indeksleme katmanları bilinçli olarak yoktur.
- **rsync**, <https://www.rsync.samba.org/> — İki sürüm arasındaki farkın akış hâlinde
  hesaplanması (delta transfer) fikri; `src/delta.rs` içindeki merge-join yapısının
  kaynağı.
- **NCdu**, <https://dev.yorhel.nl/ncdu> — Terminal disk kullanımı gezgini; okunabilir
  listeleme yaklaşımı.
- **Rust standart kütüphane** — <https://doc.rust-lang.org/std/> · `ReadDir`,
  `Metadata`, `SystemTime` ve `BufRead::read_line` kullanımı için.
- **serde / serde_json** — <https://serde.rs/> · <https://github.com/serde-rs/json> ·
  <https://docs.rs/serde_json/> — Serileştirme ve JSON çıktısı.
- **clap** — <https://docs.rs/clap/> — Komut satırı ayrıştırma.
- **cargo** — <https://doc.rust-lang.org/cargo/> — Yerel derleme ve test komutları.
- **IANA Time Zone Database** — <https://www.iana.org/time-zones> — Zaman dilimi
  verisinin kaynağı. Timefold **kullanmaz** (tüm damgalar UTC'dir); raporun R11
  riskine karşı bilinçli tercihtir.
- **Rapor dosyasının kendisi:** `%USERPROFILE%\Desktop\Fikirler\09-klasor-zaman-cizelgesi.html`
  (yerel yol, URL değil) — iç tasarımın kaynağı. Bu README'deki kararların tamamı
  (fark tabanlı depo, boşluk kuralı, ısı skoru, güven aralığı, saklama politikası,
  veri güvenliği taahhüdü) bu belgeden türetilmiştir.
- **Karar belgesi:** `%USERPROFILE%\Desktop\Projeler\MANIFEST.md`, "Kart 09" (yerel yol).

Doğrudan kopyalanan kod: **yoktur**. Yukarıdaki kaynaklar referans ve model olarak
kullanılmıştır.

---

## Üretim Atfı

Bu depo **OpenCode** ajanı tarafından, **`space-bunny-free`** modeli
(`opencode/space-bunny-free`) kullanılarak üretilmiştir.

- **Arac:** OpenCode
- **Model:** `opencode/space-bunny-free` (Space Bunny Free)
- **Tür:** Rust, `cargo build` / `cargo test` ile üretilmiş ve doğrulanmıştır.

Kaynak kod, testler ve dokümantasyon bu model tarafından yazılmıştır. İnsan
katkısı: gereksinim tanımı, kabul ölçütleri ve son kontroller.

## Lisans

MIT — bkz. [`LICENSE.txt`](LICENSE.txt). `Copyright (c) 2026 Timefold contributors`.
