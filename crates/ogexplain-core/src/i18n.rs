use std::sync::OnceLock;

static LOCALE: OnceLock<String> = OnceLock::new();

pub fn detect_locale() -> String {
    let lang = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    if lang.starts_with("zh") {
        "zh-CN".to_string()
    } else {
        "en".to_string()
    }
}

/// Auto-detects from system unless `locale` is explicitly provided.
pub fn init(locale: Option<&str>) {
    let loc = locale
        .map(|s| s.to_string())
        .unwrap_or_else(detect_locale);
    LOCALE.set(loc.clone()).ok();
    rust_i18n::set_locale(&loc);
}

pub fn current_locale() -> &'static str {
    LOCALE.get().map(|s| s.as_str()).unwrap_or("en")
}

pub fn is_zh() -> bool {
    current_locale().starts_with("zh")
}
