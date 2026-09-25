use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "tr")]
    Tr,
    #[serde(rename = "en")]
    En,
}

impl Language {
    pub fn display_name(&self) -> &'static str {
        match self {
            Language::Auto => "Sistem Dili (Otomatik) / System Default",
            Language::Tr => "Türkçe (TR)",
            Language::En => "English (EN)",
        }
    }

    pub fn to_code(&self) -> &'static str {
        match self {
            Language::Auto => "auto",
            Language::Tr => "tr",
            Language::En => "en",
        }
    }

    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Language::Tr,
            2 => Language::En,
            _ => Language::Auto,
        }
    }

    pub fn to_index(&self) -> u32 {
        match self {
            Language::Auto => 0,
            Language::Tr => 1,
            Language::En => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GuiConfig {
    #[serde(default = "default_language")]
    language: String,
}

fn default_language() -> String {
    "auto".to_string()
}

fn get_config_path() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        let mut p = PathBuf::from(home);
        p.push(".config");
        p.push("omenspace");
        p.push("gui_config.json");
        Some(p)
    } else {
        None
    }
}

fn detect_system_language() -> Language {
    for var in &["LC_MESSAGES", "LC_ALL", "LANG", "LANGUAGE"] {
        if let Ok(val) = std::env::var(var) {
            let lower = val.to_lowercase();
            if lower.starts_with("tr") || lower.contains("tr_tr") || lower.contains("turkish") {
                return Language::Tr;
            }
        }
    }
    Language::En
}

struct I18nState {
    selected_language: Language,
    active_language: Language,
}

static I18N_STATE: RwLock<I18nState> = RwLock::new(I18nState {
    selected_language: Language::Auto,
    active_language: Language::En,
});

pub fn init() {
    let mut selected = Language::Auto;
    if let Some(path) = get_config_path() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<GuiConfig>(&content) {
                selected = match cfg.language.as_str() {
                    "tr" => Language::Tr,
                    "en" => Language::En,
                    _ => Language::Auto,
                };
            }
        }
    }

    let active = match selected {
        Language::Auto => detect_system_language(),
        other => other,
    };

    if let Ok(mut state) = I18N_STATE.write() {
        state.selected_language = selected;
        state.active_language = active;
    }
}

pub fn get_selected_language() -> Language {
    I18N_STATE.read().map(|s| s.selected_language).unwrap_or(Language::Auto)
}

pub fn get_active_language() -> Language {
    I18N_STATE.read().map(|s| s.active_language).unwrap_or(Language::En)
}

pub fn set_language(lang: Language) {
    let active = match lang {
        Language::Auto => detect_system_language(),
        other => other,
    };

    if let Ok(mut state) = I18N_STATE.write() {
        state.selected_language = lang;
        state.active_language = active;
    }

    // Save config
    if let Some(path) = get_config_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let cfg = GuiConfig {
            language: lang.to_code().to_string(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&cfg) {
            let _ = fs::write(path, json);
        }
    }
}

/// Translate key using active language
pub fn t(key: &'static str) -> &'static str {
    let active = get_active_language();
    match active {
        Language::Tr => translate_tr(key),
        _ => translate_en(key),
    }
}

fn translate_tr(key: &'static str) -> &'static str {
    match key {
        // App / Navigation
        "app_title" => "OmenSpace",
        "menu" => "Menü",
        "nav_performance" => "Performans",
        "nav_undervolt" => "Gelişmiş Güç",
        "nav_mux" => "Grafik Denetimi",
        "nav_monitoring" => "Sistem İzleyici",
        "nav_lighting" => "RGB Studio",
        "nav_app_profiles" => "Profil Yöneticisi",
        "nav_overlay" => "Hızlı Menü",
        "nav_updater" => "Güncelleme Merkezi",
        "nav_settings" => "Ayarlar",

        // Titles
        "title_performance" => "Performans",
        "title_undervolt" => "Gelişmiş Güç",
        "title_mux" => "Grafik Denetimi",
        "title_monitoring" => "Sistem İzleyici",
        "title_lighting" => "RGB Studio",
        "title_app_profiles" => "Profil Yöneticisi",
        "title_overlay" => "Oyun İçi Hızlı Menü (Shift+F2)",
        "title_updater" => "Güncelleme Merkezi",
        "title_settings" => "Ayarlar",

        // Overlay Page & Settings
        "overlay_desc" => "Kısayol tuşu ile oyun içi hızlı menüyü yapılandırın, konumunu ve kısayolunu özelleştirin.",
        "overlay_group" => "Oyun İçi Hızlı Menü",
        "overlay_enable" => "Hızlı Menüyü Etkinleştir",
        "overlay_enable_sub" => "Kısayol tuşu için hızlı menüyü arka planda hazır tutar",
        "overlay_launch_now" => "Hızlı Menüyü Şimdi Aç / Test Et",
        "overlay_launch_now_sub" => "Ekran üstü hızlı kontrol panelini açar",
        "overlay_launch_btn" => "Hızlı Menüyü Aç/Kapat",
        "overlay_shortcuts_title" => "Kısayollar ve Kontroller",
        "overlay_shortcuts_sub" => "Global kısayol: Aç/Kapat • 1-3: Güç Profili • Q/W/E: Fan Modu • ESC: Kapat",
        "overlay_status_title" => "Hızlı Menü Arka Plan Durumu",
        "overlay_status_active" => "Çalışıyor",
        "overlay_status_inactive" => "Durduruldu",
        "overlay_preview_title" => "Hızlı Menü Önizleme & Özellikler",
        "overlay_preview_desc" => "Kısayol tuşuna bastığınızda ekranda cam efektli koyu tema menü belirir. Oyunlardan çıkmadan anında profil değiştirebilirsiniz.",
        "overlay_position_group" => "KONUM AYARLARI",
        "overlay_position_title" => "Ekran Konumu",
        "overlay_position_sub" => "Hızlı Menünün ekranda nerede açılacağını seçin",
        "overlay_pos_top_left" => "Sol Üst",
        "overlay_pos_top_center" => "Üst Orta",
        "overlay_pos_top_right" => "Sağ Üst",
        "overlay_pos_center_left" => "Sol Orta",
        "overlay_pos_center" => "Tam Orta",
        "overlay_pos_center_right" => "Sağ Orta",
        "overlay_pos_bottom_left" => "Sol Alt",
        "overlay_pos_bottom_center" => "Alt Orta",
        "overlay_pos_bottom_right" => "Sağ Alt",
        "overlay_hotkey_group" => "GLOBAL KISAYOL",
        "overlay_hotkey_title" => "Aç/Kapat Kısayolu",
        "overlay_hotkey_sub" => "Hızlı Menüyü oyun içinde açıp kapatacak tuş kombinasyonu",
        "overlay_hotkey_record_btn" => "Yeni Kısayol Ata",
        "overlay_hotkey_recording" => "Tuşa basın... (ESC ile iptal)",
        "overlay_hotkey_reset" => "Sıfırla (Shift+F2)",
        "overlay_margin_title" => "Ekran Kenar Boşluğu",
        "overlay_margin_sub" => "Hızlı Menü ile ekran kenarı arasındaki boşluk (piksel)",

        // Performance Page
        "system_profiles" => "Sistem Profilleri",
        "system_profiles_desc" => "BIOS profillerini yöneterek donanımınızdan en iyi performansı elde edin.",
        "perf_modes_cat" => "PERFORMANS MODLARI",
        "fan_modes_cat" => "FAN MODLARI",
        "mode_eco" => "Eco",
        "mode_eco_sub" => "Pil tasarrufu",
        "mode_balanced" => "Balanced",
        "mode_balanced_sub" => "Önerilen denge",
        "mode_performance" => "Performance",
        "mode_performance_sub" => "Maksimum güç",
        "fan_auto" => "Auto",
        "fan_auto_sub" => "Akıllı soğutma",
        "fan_max" => "Max",
        "fan_max_sub" => "Maks. soğutma",
        "fan_custom" => "Custom",
        "fan_custom_sub" => "Kullanıcı tanımlı",
        "ec_delegate" => "EC'ye Devret",
        "ec_delegate_tooltip" => "120 saniye boyunca heartbeat sinyali alınamazsa fan kontrolü otomatik olarak gömülü kontrolcüye (EC) devredilir. Bu süre içinde kontrol tamamen EC'dedir.",
        "custom_curve_title" => "Özel Fan Eğrisi",
        "custom_curve_hint" => "Noktaları yukarı/aşağı sürükleyerek fan hızını ayarlayın",
        "cpu" => "CPU",
        "gpu" => "GPU",
        "apply" => "Uygula",
        "daemon_label" => "Daemon",
        "save_preset" => "Kaydet",
        "delete_preset" => "Sil",
        "preset_name" => "Ön Ayar Adı",
        "preset_sub" => "Ön Ayar",
        "editing_preset" => "Düzenleniyor: {}",
        "save" => "Kaydet",
        "delete" => "Sil",

        // Monitoring Page
        "hardware_specs" => "Donanım Özellikleri",
        "device_status" => "Cihaz Durumu",
        "total_system_power" => "TOPLAM SİSTEM GÜCÜ",
        "system_modes" => "SİSTEM MODLARI",
        "realtime_power_desc" => "CPU & GPU anlık güç tüketimi",
        "monitoring_desc" => "Gerçek zamanlı CPU, GPU, Bellek ve Disk kullanım verileri ile sistem durumunu izleyin.",
        "mon_temp" => "SICAKLIK",
        "mon_load" => "YÜK",
        "mon_sys_pwr" => "GÜÇ",
        "mon_fan" => "FAN",
        "mon_ram" => "RAM",
        "mon_disk" => "SSD",
        "mon_rpm" => "RPM",
        "mon_mode" => "MOD",
        "cpu_wattage" => "CPU GÜÇ TÜKETİMİ",
        "gpu_wattage" => "GPU GÜÇ TÜKETİMİ",
        "hybrid_mode_short" => "Hibrit",
        "throttle_warning" => "TERMAL KISITLAMA",

        // Undervolt Page
        "uv_desc" => "Gelişmiş termal yönetim için çekirdek voltajlarını ve güç sınırlarını ayarlayın. Lütfen dikkatli kullanın.",
        "uv_lock_title" => "Intel Undervolt Koruması (UV_LOCK)",
        "uv_lock_desc" => "Sisteminiz yetkisiz MSR yazmalarını engelleyen Intel Undervolt koruması altında olabilir. \
                           Undervolt ofsetlerinin geçerli olabilmesi için BIOS üzerinden (gelişmiş menü varsa) veya OMEN Gaming Hub yüklüyse OGH üzerinden UV korumasının kaldırılması/undervolt izninin açık olması gerekebilir.",
        "msr_protected" => "MSR KORUMALI / MSR PROTECTED",
        "uv_unsupported_title" => "Desteklenmeyen İşlemci",
        "uv_unsupported_desc" => "İşlemciniz ({}) donanımsal olarak kilitlidir ve voltaj kontrolünü desteklememektedir.",
        "voltage_offsets_group" => "Voltaj Ofsetleri (Core / Cache / Uncore)",
        "voltage_offsets_desc" => "Negatif voltaj ofseti sıcaklıkları düşürür ve termal kısıtlamayı önler.",
        "core_offset_title" => "CPU Core Voltage Offset",
        "core_offset_sub" => "İşlemci çekirdeklerine giden voltaj farkı",
        "cache_offset_title" => "CPU Cache (Ring) Offset",
        "cache_offset_sub" => "L3 Cache ve Ring Bus voltaj farkı",
        "sa_offset_title" => "System Agent (Uncore) Offset",
        "sa_offset_sub" => "Bellek kontrolcüsü ve PCIe veri yolu",
        "power_limits_title" => "Güç Limitleri (Intel RAPL / MSR)",
        "power_limits_desc" => "PL1 sürekli yük gücünü, PL2 kısa süreli turbo tavanını belirler.",
        "pl1_label" => "PL1 — Sürekli Güç Limiti (Long Duration)",
        "pl1_sub" => "Standart TDP: 45W | Ayarlanabilir: 25W - 75W",
        "pl2_label" => "PL2 — Turbo Güç Limiti (Short Duration)",
        "pl2_sub" => "Standart Turbo: 95W | Ayarlanabilir: 45W - 115W",
        "tau_label" => "Tau — Turbo Süresi Penceresi",
        "tau_sub" => "PL2 turbo modunda kalınacak maksimum süre",
        "tau_seconds" => "saniye",
        "tau_default" => "(Varsayılan)",
        "tcc_label" => "TCC Offset (Maksimum CPU Sıcaklığı)",
        "tcc_sub" => "Hedef tavan sıcaklık: 100°C - TCC Ofseti (Örn: 3°C ofset = 97°C Max)",
        "tcc_offset_str" => "Ofset",
        "reset_defaults" => "Varsayılanlara Sıfırla",
        "test_safe_mode" => "⚡ Test Et (30sn Güvenli Mod)",
        "apply_save" => "Uygula & Kaydet",
        "applied_successfully" => "Ayarlar başarıyla uygulandı",

        // MUX Page
        "mux_desc" => "Ekran kartı çalışma modunu belirleyin. Değişiklikler yeniden başlatma gerektirebilir.",
        "hybrid_title" => "Hybrid",
        "hybrid_mode" => "Otomatik Geçiş (Önerilen)",
        "hybrid_desc" => "Günlük kullanımda dahili grafik birimi, oyunlarda harici grafik kartı kullanılır.",
        "hybrid_sub" => "Dahili ve Harici GPU",
        "discrete_title" => "Discrete",
        "discrete_mode" => "Discrete Mod",
        "discrete_desc" => "Dahili GPU kapatılır. Tüm ekran çıkışları doğrudan harici grafik kartına (dGPU) bağlanır.",
        "advanced_title" => "Advanced",
        "advanced_mode" => "Gelişmiş / Optimus",
        "advanced_desc" => "Dinamik geçiş sağlayan gelişmiş MUX modu (Sadece desteklenen cihazlarda).",
        "restart_required" => "Yeniden Başlatma Gerekli",
        "restart_desc" => "GPU modu değişikliğinin geçerli olması için sistemi yeniden başlatın.",
        "restart_now" => "Yeniden Başlat",
        "gpu_details_group" => "Grafik &amp; Ekran Bilgileri",
        "disp_out_row" => "Ekran Çıkışı &amp; GPU Eşleşmesi",
        "disp_out_sub" => "Dahili Panel (eDP-1) 1920x1080 @ 144Hz",
        "active_gpu" => "Ekran Kartı Modeli",
        "dgpu_sub" => "Harici / Ayrık Grafik Birimi (dGPU)",
        "driver_ver" => "Sürücü Sürümü",
        "driver_sub" => "NVIDIA Linux Grafik Sürücüsü",

        // RGB Lighting Page
        "lighting_desc" => "Klavye ve lightbar RGB renklerini ve efektlerini özelleştirin.",
        "kb_lighting_group" => "Klavye Aydınlatması",
        "omen_4zone_desc" => "OMEN 4 Bölge RGB dinamik renk ve animasyon yapılandırması.",
        "desktop_rgb_desc" => "OMEN Desktop donanımları için kasa içi RGB aydınlatma yönetimi.",
        "victus_1zone_desc" => "Victus Tek Bölge aydınlatma ve parlaklık yapılandırması.",
        "hw_arch" => "Algılanan Donanım Mimarisi",
        "hw_arch_omen" => "OMEN Serisi — 4 Bölge RGB Klavye Desteği",
        "hw_arch_victus" => "Victus Serisi — Tek Bölge Aydınlatma Desteği",
        "badge_4zone" => "4-Bölge RGB",
        "badge_1zone" => "Tek Bölge",
        "kb_effect" => "Klavye Efekti",
        "kb_effect_speed" => "Efekt Hızı",
        "effect_static" => "Statik (Sabit Renk)",
        "effect_breathing" => "Nefes Alma (Breathing)",
        "effect_blinking" => "Yanıp Sönme (Blinking)",
        "effect_cycle" => "Renk Döngüsü (Cycle)",
        "effect_wave" => "Dalga (Wave)",
        "effect_audio" => "Ses Görselleştirici",
        "zone_override" => "Bölge Modu Geçersiz Kılma",
        "zone_override_sub" => "Donanım bölge kontrol tipini manuel belirleyin",
        "kb_brightness" => "Klavye Parlaklığı",
        "kb_color_map" => "Klavye Renk Haritası",
        "kb_color_map_desc" => "Bir renk seçin ve renklendirmek istediğiniz tuşa veya bölgeye tıklayın.",
        "per_key_hint" => "Tekli tuş atamalarını klavye üstüne tıklayarak ayarlayabilirsiniz.",
        "active_color" => "Aktif Renk:",
        "apply_to_kb" => "Klavyeye Uygula",
        "lightbar_group" => "Lightbar (Ön / Kasa Işık Şeridi)",
        "lightbar_desc" => "OMEN kasaları ve laptoplarındaki ön RGB ışık çubuğu kontrolü.",
        "lightbar_unsupported" => "Bu cihazda lightbar desteklenmemektedir.",
        "lightbar_enable" => "Arayüzde Göster",
        "lightbar_enable_sub" => "Lightbar ayarlarını arayüzde göster / gizle",
        "lightbar_effect" => "Lightbar Efekti",
        "lightbar_brightness" => "Lightbar Parlaklığı",
        "lightbar_segments" => "Lightbar Segment Renkleri",

        // App Profiles Page
        "app_profiles_desc" => "Aktif uygulamaya göre performans profili ve fan modunu otomatik değiştir",
        "enable_profiles" => "Uygulama Profillerini Etkinleştir",
        "enable_profiles_sub" => "Odak penceresi değiştiğinde profil otomatik uygulanır",
        "defined_profiles" => "Tanımlı Profiller",
        "defined_profiles_desc" => "Uygulama açıldığında otomatik geçiş yapılacak profiller",
        "add_profile" => "Profil Ekle",
        "add_profile_sub" => "Yeni bir uygulama profili oluştur",
        "detect_method" => "Algılama Yöntemi",
        "window_detect" => "Pencere Algılama",
        "window_detect_sub" => "Aktif uygulamayı nasıl belirleyeceği",
        "proc_name" => "Süreç Adı (process name)",
        "wm_class" => "WM_CLASS (X11)",
        "app_id_wayland" => "app-id (Wayland)",
        "switch_delay" => "Geçiş Gecikmesi",
        "switch_delay_sub" => "Uygulama odaklanınca kaç saniye beklenecek",
        "profile_fmt" => "Profil",
        "fan_fmt" => "Fan",
        "browse_apps" => "Uygulamalara Gözat...",
        "select_app" => "Uygulama Seçin",
        "search_apps" => "Uygulamalarda ara...",

        // Updater Page
        "updater_desc" => "OmenSpace ve cihaz firmware güncellemelerini yönet",
        "current_version" => "Mevcut Versiyon",
        "last_checked" => "Son kontrol: Bugün",
        "update_status" => "Güncelleme Durumu",
        "up_to_date" => "Güncel",
        "check_updates" => "Güncellemeleri Kontrol Et",
        "firmware_group" => "Donanım Yazılımı",
        "firmware_desc" => "HP BIOS ve EC firmware güncellemeleri — fwupd üzerinden",
        "scan_fwupd" => "fwupd ile Tara",
        "scan_fwupd_sub" => "Tüm LVFS güncellemelerini kontrol et",
        "update_channel_group" => "Güncelleme Kanalı",
        "channel_row" => "Kanal",
        "channel_sub" => "Hangi yayın kanalından güncelleme alınacak",
        "channel_stable" => "Kararlı (Stable)",
        "channel_beta" => "Beta",
        "channel_dev" => "Geliştirici (Dev)",
        "auto_update_check" => "Otomatik Güncelleme Kontrolü",
        "auto_update_sub" => "Haftada bir otomatik kontrol yap",
        "checking_updates_body" => "Güncellemeler denetleniyor, lütfen bekleyin...",
        "checking_updates_title" => "Güncelleştirmeler Denetleniyor...",
        "update_available" => "Yeni Güncelleme Mevcut",
        "release_notes" => "Sürüm Notları (Değişiklikler):",
        "no_release_notes" => "Sürüm notu bulunamadı.",
        "ignore" => "Yoksay",
        "update" => "Güncelle",
        "version_fetch_err" => "Sürüm bilgisi alınamadı.",
        "invalid_json_err" => "Geçersiz JSON yanıtı.",
        "connection_err" => "Bağlantı hatası.",
        "updating" => "Güncelleniyor...",
        "update_completed" => "Güncelleme Tamamlandı!",
        "close" => "Kapat",
        "cancel" => "İptal",
        "no_updates" => "Sisteminiz güncel. Yeni bir güncelleme bulunamadı.",
        "update_failed" => "Güncelleme denetimi başarısız",
        "fwupdmgr_missing" => "fwupdmgr yüklü değil veya erişilemiyor.",
        "ok_btn" => "Tamam",

        // Settings Page
        "settings_desc" => "OmenSpace daemon & uygulama yapılandırması",
        "hw_config_group" => "Donanım Yapılandırması",
        "appearance_and_lang" => "Görünüm &amp; Dil",
        "appearance_mode" => "Tema Görünümü",
        "appearance_mode_sub" => "Uygulama temasını belirleyin (Aydınlık / Karanlık)",
        "language_row_title" => "Uygulama Dili (Language)",
        "language_row_sub" => "Arayüz dilini anında değiştirin",
        "daemon_group" => "Daemon",
        "daemon_status" => "Daemon Durumu",
        "daemon_status_sub" => "OmenSpace arka plan servisine bağlantı",
        "connected" => "Bağlı",
        "ready" => "Hazır",
        "disconnected" => "Bağlantı Yok",
        "heartbeat_interval" => "Heartbeat Aralığı",
        "heartbeat_sub" => "Daemon iletişim frekansı (saniye)",
        "autostart" => "Otomatik Başlat",
        "autostart_sub" => "Sistem açılışında daemon'ı başlat",
        "perf_behavior" => "Performans Davranışı",
        "startup_profile" => "Başlangıç Profili",
        "startup_profile_sub" => "Uygulama açıldığında etkinleştirilecek profil",
        "last_used" => "Son Kullanılan",
        "battery_care" => "Pil Koruma Modu",
        "battery_care_sub" => "Pil dolum sınırını %80 ile sınırla (uzun ömür)",
        "thermal_alerts" => "Termal Uyarılar",
        "thermal_alerts_sub" => "CPU/GPU 90°C üzerine çıktığında bildirim gönder",
        "fan_control_group" => "Fan Kontrolü",
        "ec_timeout" => "EC Devir Zaman Aşımı",
        "ec_timeout_sub" => "Bu süre içinde bağlantı kesilirse EC kontrolü devralır",
        "manual_fan_override" => "Manuel Fan Override'a İzin Ver",
        "manual_fan_override_sub" => "Kullanıcı tanımlı eğrileri donanıma uygula",
        "rgb_backend_group" => "Klavye Aydınlatma Denetleyicisi",
        "rgb_backend_desc" => "RGB ve aydınlatma donanım iletişim sürücüsü",
        "comm_driver" => "İletişim Sürücüsü (Backend)",
        "comm_driver_sub" => "Klavye ışıklandırma komutlarının donanıma iletim yöntemi",
        "auto_detect_recommended" => "Otomatik Algıla (Önerilen)",
        "active_hw_interface" => "Aktif Donanım Arayüzü",
        "active_hw_interface_sub" => "Sistemde tespit edilen kontrol mekanizması",
        "about_group" => "OmenSpace Hakkında",
        "version" => "Versiyon",
        "device" => "Cihaz",
        "kernel" => "Kernel",
        "daemon_socket" => "Daemon Soketi",
        "seconds_abbr" => "sn",
        "troubleshooting_group" => "Hata Ayıklama &amp; Tanılama",
        "troubleshooting_desc" => "Desteklenmeyen cihazlar için donanım raporu ve GitHub Issue oluşturucu.",

        "fan_cleaning_title" => "Fan Temizliği (Dust Cleaning)",
        "fan_cleaning_sub" => "Tozları temizlemek için fanları kısa süreliğine tam güçte çalıştırır",
        "fan_cleaning_msg" => "Fan temizleme rutini çalışıyor. Fanlar birkaç saniye tam devirde dönecek...",
        "lightbar_wmi_toggle" => "Lightbar Arayüzü",
        "lightbar_wmi_toggle_sub" => "Arayüzde Lightbar seçeneklerini gösterir / gizler",
        "btn_close" => "Kapat",
        "btn_cancel" => "İptal",
        "btn_next" => "İleri",
        "btn_start_next" => "Başla & İleri",
        "rgb_issue_title" => "Per-Key RGB Hata Raporu Oluştur",
        "rgb_issue_sub" => "Klavye aydınlatmanız çalışmıyorsa tanılama raporu oluşturur",
        "diag_report_title" => "Sistem Tanı Raporu Oluştur",
        "diag_report_sub" => "Donanım problarını ve EC kayıtlarını dışa aktarır",
        "per_key_wiz_title" => "RGB Per-Key Kalibrasyon Sihirbazı",
        "gen_rgb_issue" => "RGB Hata Raporu Oluşturuluyor",
        "wait_hw_footprint" => "Donanım ayak izi toplanırken lütfen bekleyin...",
        "diag_scan_running" => "Tanılama Taraması Çalışıyor",
        "scan_wmi_endpoints" => "WMI uç noktaları ve EC bellek kayıtları taranıyor. Bu işlem birkaç saniye sürebilir...",
        "wiz_key_name_ph" => "Tuş adı...",
        "theme_auto" => "Otomatik (Sistem Teması)",
        "theme_light" => "Açık",
        "theme_dark" => "Koyu",
        "zone_4zone" => "OMEN (4-Bölgeli RGB)",
        "zone_single" => "Victus (Tek Bölge)",
        "zone_perkey" => "OMEN (Per-Key RGB)",
        "zone_desktop" => "OMEN Desktop (Kasa RGB)",

        "per_key_wiz_sub" => "Tüm 104 tuşu desteklemek için klavyenizi interaktif olarak haritalayın",
        "rgb_issue_ready" => "RGB Hata Raporu Hazır",
        "copy_report_gh" => "Bu raporu kopyalayıp GitHub issue tracker'a gönderin.",
        "diag_report_ready" => "Tanılama Raporu",
        "diag_report_body" => "Sistem tanılama raporunuz oluşturuldu.",
        "btn_create_gh_issue" => "GitHub Issue Oluştur",
        "error_generic" => "Hata",
        "wiz_key_lit" => "Tuş 1 / 104 işik veriyor. Hangi tuş?",
        "wiz_key_lit_n" => "Tuş {} / 104 işik veriyor. Hangi tuş?",
        "wiz_complete" => "Sihirbaz Tamamlandı",
        "wiz_complete_body" => "Kalibrasyon tamamlandı. Lütfen bu raporu gönderin.",

        // RGB & Lightbar additions
        "kb_global_color" => "Tüm Klavyenin Rengi (Global Color):",
        "lightbar_title" => "4-Segment Lightbar (Ön / Kasa Işık Şeridi)",
        "lightbar_sync_btn" => "Tüm Segmentleri Boya",
        "lb_seg_1" => "Bölge 1 (Sol)",
        "lb_seg_2" => "Bölge 2 (Orta-Sol)",
        "lb_seg_3" => "Bölge 3 (Orta-Sağ)",
        "lb_seg_4" => "Bölge 4 (Sağ)",
        "omen_per_key_desc" => "Per-Key RGB Klavye",
        "effect_wave_custom" => "Dalga (Özel Renkler)",
        "effect_wave_rainbow" => "Dalga (Gökkuşağı)",
        
        "effect_starlight" => "Starlight / Yıldız Işığı",
        "effect_marquee" => "Marquee / Spiral",
        "effect_reactive" => "Reactive (Tepkisel)",
        "effect_ripple" => "Ripple (Dalgalanma)",
        "effect_raindrop" => "Raindrop (Yağmur)",

        _ => translate_en(key),
    }
}

fn translate_en(key: &'static str) -> &'static str {
    match key {
        // App / Navigation
        "app_title" => "OmenSpace",
        "menu" => "Menu",
        "nav_performance" => "Performance",
        "nav_undervolt" => "Advanced Power",
        "nav_mux" => "Graphics Switcher",
        "nav_monitoring" => "System Monitor",
        "nav_lighting" => "RGB Studio",
        "nav_app_profiles" => "Profile Manager",
        "nav_overlay" => "Quick Overlay",
        "nav_updater" => "Update Center",
        "nav_settings" => "Settings",

        // Titles
        "title_performance" => "Performance",
        "title_undervolt" => "Advanced Power",
        "title_mux" => "Graphics Switcher",
        "title_monitoring" => "System Monitor",
        "title_lighting" => "RGB Studio",
        "title_app_profiles" => "Profile Manager",
        "title_overlay" => "Quick HUD Overlay (Shift+F2)",
        "title_updater" => "Update Center",
        "title_settings" => "Settings",

        // Overlay Page & Settings
        "overlay_desc" => "Configure position, hotkey, and test the in-game floating HUD overlay.",
        "overlay_group" => "Quick HUD Overlay",
        "overlay_enable" => "Enable Quick Overlay",
        "overlay_enable_sub" => "Keep overlay resident in background for instant hotkey access",
        "overlay_launch_now" => "Launch Overlay Now / Test",
        "overlay_launch_now_sub" => "Open on-screen floating HUD overlay",
        "overlay_launch_btn" => "Toggle Overlay",
        "overlay_shortcuts_title" => "Shortcuts & Keybindings",
        "overlay_shortcuts_sub" => "Global hotkey: Toggle • 1-3: Power Profiles • Q/W/E: Fan Modes • ESC: Close",
        "overlay_status_title" => "Overlay Background Daemon Status",
        "overlay_status_active" => "Active",
        "overlay_status_inactive" => "Inactive",
        "overlay_preview_title" => "Overlay Preview & Capabilities",
        "overlay_preview_desc" => "Your configured hotkey invokes a glassmorphic floating HUD overlay over games and apps for zero-distraction power and fan switching.",
        "overlay_position_group" => "POSITION SETTINGS",
        "overlay_position_title" => "Screen Position",
        "overlay_position_sub" => "Where the overlay appears on screen",
        "overlay_pos_top_left" => "Top Left",
        "overlay_pos_top_center" => "Top Center",
        "overlay_pos_top_right" => "Top Right",
        "overlay_pos_center_left" => "Center Left",
        "overlay_pos_center" => "Center",
        "overlay_pos_center_right" => "Center Right",
        "overlay_pos_bottom_left" => "Bottom Left",
        "overlay_pos_bottom_center" => "Bottom Center",
        "overlay_pos_bottom_right" => "Bottom Right",
        "overlay_hotkey_group" => "GLOBAL HOTKEY",
        "overlay_hotkey_title" => "Toggle Hotkey",
        "overlay_hotkey_sub" => "Key combination to open/close the overlay anywhere, including in games",
        "overlay_hotkey_record_btn" => "Set New Hotkey",
        "overlay_hotkey_recording" => "Press keys now... (ESC to cancel)",
        "overlay_hotkey_reset" => "Reset to Shift+F2",
        "overlay_margin_title" => "Screen Edge Margin",
        "overlay_margin_sub" => "Distance between overlay and screen edge (pixels)",

        // Performance Page
        "system_profiles" => "System Profiles",
        "system_profiles_desc" => "Manage BIOS profiles to get the best performance from your hardware.",
        "perf_modes_cat" => "PERFORMANCE MODES",
        "fan_modes_cat" => "FAN MODES",
        "mode_eco" => "Eco",
        "mode_eco_sub" => "Battery saver",
        "mode_balanced" => "Balanced",
        "mode_balanced_sub" => "Recommended balance",
        "mode_performance" => "Performance",
        "mode_performance_sub" => "Maximum power",
        "fan_auto" => "Auto",
        "fan_auto_sub" => "Smart cooling",
        "fan_max" => "Max",
        "fan_max_sub" => "Max cooling",
        "fan_custom" => "Custom",
        "fan_custom_sub" => "User defined",
        "ec_delegate" => "Delegate to EC",
        "ec_delegate_tooltip" => "If no heartbeat signal is received for 120 seconds, fan control is automatically delegated to the Embedded Controller (EC). Control remains entirely with EC during this period.",
        "custom_curve_title" => "Custom Fan Curve",
        "custom_curve_hint" => "Drag points up/down to adjust fan speed",
        "cpu" => "CPU",
        "gpu" => "GPU",
        "apply" => "Apply",
        "daemon_label" => "Daemon",
        "save_preset" => "Save Preset",
        "delete_preset" => "Delete Preset",
        "preset_name" => "Preset Name",
        "preset_sub" => "Preset",
        "editing_preset" => "Editing Preset: {}",
        "save" => "Save",
        "delete" => "Delete",

        // Monitoring Page
        "hardware_specs" => "Hardware Specifications",
        "device_status" => "Device Status",
        "total_system_power" => "TOTAL POWER",
        "system_modes" => "SYSTEM MODES",
        "realtime_power_desc" => "CPU & GPU real-time power consumption",
        "monitoring_desc" => "Monitor real-time CPU, GPU, Memory and Disk usage along with system status.",
        "mon_temp" => "TEMP",
        "mon_load" => "LOAD",
        "mon_sys_pwr" => "PWR",
        "mon_fan" => "FAN",
        "mon_ram" => "RAM",
        "mon_disk" => "SSD",
        "mon_rpm" => "RPM",
        "mon_mode" => "MODE",
        "cpu_wattage" => "CPU WATTAGE",
        "gpu_wattage" => "GPU WATTAGE",
        "hybrid_mode_short" => "Hybrid",
        "throttle_warning" => "THROTTLE",

        // Undervolt Page
        "uv_desc" => "Adjust core voltages and power limits for advanced thermal management. Please use with caution.",
        "uv_lock_title" => "Intel Undervolt Protection (UV_LOCK)",
        "uv_lock_desc" => "Your system may be under Intel Undervolt Protection blocking unauthorized MSR writes. \
                           For undervolt offsets to apply, UV protection must be disabled or undervolting must be enabled via BIOS (if advanced menus are available) or OMEN Gaming Hub.",
        "msr_protected" => "MSR PROTECTED",
        "uv_unsupported_title" => "Unsupported CPU",
        "uv_unsupported_desc" => "Your processor ({}) is hardware locked and does not support voltage control.",
        "voltage_offsets_group" => "Voltage Offsets (Core / Cache / Uncore)",
        "voltage_offsets_desc" => "Negative voltage offsets reduce temperatures and prevent thermal throttling.",
        "core_offset_title" => "CPU Core Voltage Offset",
        "core_offset_sub" => "Voltage offset applied to CPU cores (mV)",
        "cache_offset_title" => "CPU Cache (Ring) Offset",
        "cache_offset_sub" => "Voltage offset for L3 cache and ring bus (mV)",
        "sa_offset_title" => "System Agent (Uncore) Offset",
        "sa_offset_sub" => "Memory controller and PCIe interconnect offset (mV)",
        "power_limits_title" => "Power Limits (Intel RAPL / MSR)",
        "power_limits_desc" => "PL1 determines sustained power draw; PL2 sets short-duration turbo ceiling.",
        "pl1_label" => "PL1 — Long Duration Power Limit",
        "pl1_sub" => "Base TDP: 45W | Configurable: 25W - 75W",
        "pl2_label" => "PL2 — Short Duration Turbo Limit",
        "pl2_sub" => "Base Turbo: 95W | Configurable: 45W - 115W",
        "tau_label" => "Tau — Turbo Time Window",
        "tau_sub" => "Maximum duration allowed at PL2 turbo wattage",
        "tau_seconds" => "seconds",
        "tau_default" => "(Default)",
        "tcc_label" => "TCC Offset (Max CPU Temperature)",
        "tcc_sub" => "Target peak temperature: 100°C - TCC Offset (e.g. 3°C offset = 97°C Max)",
        "tcc_offset_str" => "Offset",
        "reset_defaults" => "Reset to Defaults",
        "test_safe_mode" => "⚡ Test (30s Safe Mode)",
        "apply_save" => "Apply & Save",
        "applied_successfully" => "Settings applied successfully",

        // MUX Page
        "mux_desc" => "Select GPU operation mode. Changes may require a system restart.",
        "hybrid_title" => "Hybrid",
        "hybrid_mode" => "Auto Switch (Recommended)",
        "hybrid_desc" => "Uses integrated GPU for daily tasks and discrete graphics for heavy games.",
        "hybrid_sub" => "Integrated & Discrete",
        "discrete_title" => "Discrete",
        "discrete_mode" => "Discrete Mode",
        "discrete_desc" => "Integrated GPU is disabled. All displays connect directly to the discrete GPU.",
        "advanced_title" => "Advanced",
        "advanced_mode" => "Advanced / Optimus",
        "advanced_desc" => "Advanced MUX mode with dynamic switching (Only on supported devices).",
        "restart_required" => "Restart Required",
        "restart_desc" => "Please restart the system for GPU mode changes to take effect.",
        "restart_now" => "Restart Now",
        "gpu_details_group" => "Graphics & Display Details",
        "disp_out_row" => "Display Output & Active GPU",
        "disp_out_sub" => "Internal Panel (eDP-1) 1920x1080 @ 144Hz",
        "active_gpu" => "Graphics Card Model",
        "dgpu_sub" => "Discrete Graphics Unit (dGPU)",
        "driver_ver" => "Driver Version",
        "driver_sub" => "NVIDIA Linux Graphics Driver",

        // RGB Lighting Page
        "lighting_desc" => "Customize keyboard and lightbar RGB colors and effects.",
        "kb_lighting_group" => "Keyboard Lighting",
        "omen_4zone_desc" => "OMEN 4-Zone RGB dynamic color and animation configuration.",
        "desktop_rgb_desc" => "In-case RGB lighting management for OMEN Desktop hardware.",
        "victus_1zone_desc" => "Victus Single-Zone lighting and brightness configuration.",
        "hw_arch" => "Detected Hardware Architecture",
        "hw_arch_omen" => "OMEN Series — 4-Zone RGB Keyboard Support",
        "hw_arch_victus" => "Victus Series — Single-Zone Lighting Support",
        "badge_4zone" => "4-Zone RGB",
        "badge_1zone" => "Single Zone",
        "kb_effect" => "Keyboard Effect",
        "kb_effect_speed" => "Effect Speed",
        "effect_static" => "Static (Solid Color)",
        "effect_breathing" => "Breathing",
        "effect_blinking" => "Blinking",
        "effect_cycle" => "Color Cycle",
        "effect_wave_custom" => "Wave (Özel Renkler)",
        "effect_wave_rainbow" => "Wave (Gökkuşağı)",
        "effect_wave" => "Wave",
        "zone_override" => "Zone Mode Override",
        "zone_override_sub" => "Manually specify hardware zone control type",
        "kb_brightness" => "Keyboard Brightness",
        "kb_sync_lightbar" => "Sync All Segments",
        "kb_color_1" => "Zone 1",
        "kb_color_2" => "Zone 2",
        "kb_color_3" => "Zone 3",
        "kb_color_4" => "Zone 4",
        "kb_color_map" => "Keyboard Color Map",
        "kb_color_map_desc" => "Select a color and click any key or zone to apply it.",
        "per_key_hint" => "Select a color and click any key to apply it.",
        "active_color" => "Active Color:",
        "apply_to_kb" => "Apply to Keyboard",
        "lightbar_group" => "Lightbar (Front / Chassis Lightstrip)",
        "lightbar_desc" => "Front RGB lightbar control for OMEN desktops and laptops.",
        "lightbar_unsupported" => "Lightbar is not supported on this device.",
        "lightbar_enable" => "Show in UI",
        "lightbar_enable_sub" => "Show / hide lightbar settings in the UI",
        "lightbar_effect" => "Lightbar Effect",
        "lightbar_brightness" => "Lightbar Brightness",
        "lightbar_segments" => "Lightbar Segment Colors",

        // App Profiles Page
        "app_profiles_desc" => "Automatically switch performance profile and fan mode based on active application",
        "enable_profiles" => "Enable App Profiles",
        "enable_profiles_sub" => "Profile applies automatically when window focus changes",
        "defined_profiles" => "Configured Profiles",
        "defined_profiles_desc" => "Profiles activated when the corresponding application is focused",
        "add_profile" => "Add Profile",
        "add_profile_sub" => "Create a new application profile",
        "detect_method" => "Detection Method",
        "window_detect" => "Window Detection Backend",
        "window_detect_sub" => "How active application is identified",
        "proc_name" => "Process Name (process name)",
        "wm_class" => "WM_CLASS (X11)",
        "app_id_wayland" => "app-id (Wayland)",
        "switch_delay" => "Transition Delay",
        "switch_delay_sub" => "Seconds to wait after window focus before switching",
        "profile_fmt" => "Profile",
        "fan_fmt" => "Fan",
        "browse_apps" => "Browse Apps...",
        "select_app" => "Select Application",
        "search_apps" => "Search apps...",

        // Updater Page
        "updater_desc" => "Manage OmenSpace and device firmware updates",
        "current_version" => "Current Version",
        "last_checked" => "Last checked: Today",
        "update_status" => "Update Status",
        "up_to_date" => "Up to date",
        "check_updates" => "Check for Updates",
        "firmware_group" => "Firmware & Drivers",
        "firmware_desc" => "HP BIOS and EC firmware updates via fwupd",
        "scan_fwupd" => "Scan with fwupd (LVFS)",
        "scan_fwupd_sub" => "Check Linux Vendor Firmware Service updates",
        "update_channel_group" => "Update Channel",
        "channel_row" => "Channel",
        "channel_sub" => "Release channel to receive updates from",
        "channel_stable" => "Stable",
        "channel_beta" => "Beta",
        "channel_dev" => "Developer (Dev)",
        "auto_update_check" => "Automatic Update Check",
        "auto_update_sub" => "Check for updates automatically once a week",
        "checking_updates_body" => "Checking for updates, please wait...",
        "checking_updates_title" => "Checking for updates...",
        "update_available" => "New Update Available",
        "release_notes" => "Release Notes (Changelog):",
        "no_release_notes" => "No release notes found.",
        "ignore" => "Ignore",
        "update" => "Update",
        "version_fetch_err" => "Failed to fetch version info.",
        "invalid_json_err" => "Invalid JSON response.",
        "connection_err" => "Connection error.",
        "updating" => "Updating...",
        "update_completed" => "Update Completed!",
        "close" => "Close",
        "cancel" => "Cancel",
        "no_updates" => "Your system is up to date. No new updates found.",
        "update_failed" => "Update check failed",
        "fwupdmgr_missing" => "fwupdmgr is not installed or accessible.",
        "ok_btn" => "OK",

        // Settings Page
        "settings_desc" => "OmenSpace daemon & application configuration",
        "hw_config_group" => "Hardware Configuration",
        "appearance_and_lang" => "Appearance & Language",
        "appearance_mode" => "Theme Appearance",
        "appearance_mode_sub" => "Set the application theme (Light / Dark)",
        "language_row_title" => "Application Language / Dil",
        "language_row_sub" => "Select interface language instantly",
        "daemon_group" => "Daemon",
        "daemon_status" => "Daemon Status",
        "daemon_status_sub" => "Connection to OmenSpace background service",
        "connected" => "Connected",
        "ready" => "Ready",
        "disconnected" => "Disconnected",
        "heartbeat_interval" => "Heartbeat Interval",
        "heartbeat_sub" => "Daemon communication frequency (seconds)",
        "autostart" => "Autostart on Boot",
        "autostart_sub" => "Start daemon automatically when system boots",
        "perf_behavior" => "Performance Behavior",
        "startup_profile" => "Startup Profile",
        "startup_profile_sub" => "Profile activated when application starts",
        "last_used" => "Last Used",
        "battery_care" => "Battery Care",
        "battery_care_desc" => "Limits charging to 80% to prolong battery life.",
        "thermal_alerts" => "Thermal Alerts",
        "thermal_alerts_sub" => "Send notification when CPU/GPU exceeds 90°C",
        "fan_control_group" => "Fan Control",
        "ec_timeout" => "EC Handover Timeout",
        "ec_timeout_sub" => "If connection is lost for this duration, EC takes over",
        "manual_fan_override" => "Allow Manual Fan Override",
        "manual_fan_override_sub" => "Apply user custom curves to hardware",
        "rgb_backend_group" => "Keyboard Lighting Controller",
        "rgb_backend_desc" => "RGB and lighting hardware communication driver",
        "comm_driver" => "Communication Driver (Backend)",
        "comm_driver_sub" => "Method used to transmit RGB lighting commands to hardware",
        "auto_detect_recommended" => "Auto Detect (Recommended)",
        "active_hw_interface" => "Active Hardware Interface",
        "active_hw_interface_sub" => "Detected hardware control mechanism on this system",
        "about_group" => "About OmenSpace",
        "version" => "Version",
        "device" => "Device",
        "kernel" => "Kernel",
        "daemon_socket" => "Daemon Socket",
        "seconds_abbr" => "s",
        "troubleshooting_group" => "Troubleshooting & Diagnostics",
        "troubleshooting_desc" => "Hardware reporter and GitHub Issue generator for unsupported devices.",

        "fan_cleaning_title" => "Fan Dust Cleaning",
        "fan_cleaning_sub" => "Run fans at maximum speed for a few seconds to clear out dust",
        "fan_cleaning_msg" => "Running fan dust cleaning routine. Fans will max out for a few seconds...",
        "lightbar_wmi_toggle" => "Lightbar UI",
        "lightbar_wmi_toggle_sub" => "Show or hide lightbar settings in the interface",
        "btn_close" => "Close",
        "btn_cancel" => "Cancel",
        "btn_next" => "Next",
        "btn_start_next" => "Start & Next",
        "rgb_issue_title" => "Create RGB Per-Key Issue",
        "rgb_issue_sub" => "Generates a diagnostic report if your keyboard RGB is unsupported",
        "diag_report_title" => "Generate System Diagnostic Report",
        "diag_report_sub" => "Extracts EC registers and hardware probe data",
        "per_key_wiz_title" => "RGB Per-Key Calibration Wizard",
        "gen_rgb_issue" => "Generating RGB Issue Report",
        "wait_hw_footprint" => "Please wait while we gather your hardware footprint...",
        "diag_scan_running" => "Diagnostic Scan Running",
        "scan_wmi_endpoints" => "Scanning WMI endpoints and Embedded Controller (EC) memory registers. This might take a few seconds...",
        "wiz_key_name_ph" => "Key name...",
        "theme_auto" => "Auto (System Theme)",
        "theme_light" => "Light",
        "theme_dark" => "Dark",
        "zone_4zone" => "OMEN (4-Zone RGB)",
        "zone_single" => "Victus (Single Zone)",
        "zone_perkey" => "OMEN (Per-Key RGB)",
        "zone_desktop" => "OMEN Desktop (Case RGB)",

        "per_key_wiz_sub" => "Interactively map all 104 keys to support your keyboard",
        "rgb_issue_ready" => "RGB Issue Report Ready",
        "copy_report_gh" => "Copy this report and submit it to our GitHub issue tracker.",
        "diag_report_ready" => "Diagnostic Report",
        "diag_report_body" => "Your system diagnostic report has been generated.",
        "btn_create_gh_issue" => "Create GitHub Issue",
        "error_generic" => "Error",
        "wiz_key_lit" => "Key 1 / 104 is lit. What is it?",
        "wiz_complete" => "Wizard Complete",
        "wiz_complete_body" => "Calibration is complete. Please submit this report.",

        // RGB & Lightbar additions
        "kb_global_color" => "Global Keyboard Color:",
        "lightbar_title" => "4-Segment Lightbar (Front / Chassis Strip)",
        "lb_seg_1" => "Zone 1 (Left)",
        "lb_seg_2" => "Zone 2 (Mid-Left)",
        "lb_seg_3" => "Zone 3 (Mid-Right)",
        "lb_seg_4" => "Zone 4 (Right)",
        "omen_per_key_desc" => "Per-Key RGB Keyboard",
        "effect_starlight" => "Starlight",
        "effect_marquee" => "Marquee",
        "effect_reactive" => "Reactive",
        "effect_ripple" => "Ripple",
        "effect_raindrop" => "Raindrop",

        // Default fallback
        _ => key,
    }
}
