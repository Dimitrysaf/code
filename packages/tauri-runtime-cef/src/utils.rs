use crate::backend::Theme;

pub fn to_cef_theme(theme: tauri_utils::Theme) -> Theme {
    match theme {
        tauri_utils::Theme::Dark => Theme::Dark,
        _ => Theme::Light,
    }
}

pub fn from_cef_theme(theme: Theme) -> tauri_utils::Theme {
    match theme {
        Theme::Dark => tauri_utils::Theme::Dark,
        Theme::Light => tauri_utils::Theme::Light,
    }
}

pub fn to_tao_theme(theme: tauri_utils::Theme) -> tao::window::Theme {
    match theme {
        tauri_utils::Theme::Dark => tao::window::Theme::Dark,
        _ => tao::window::Theme::Light,
    }
}
