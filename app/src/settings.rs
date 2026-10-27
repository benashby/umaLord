//! Per-user settings in %APPDATA%\umaLord\settings.json (theme, saved API key), in
//! `~/Library/Application Support/umaLord` on macOS, and `$XDG_CONFIG_HOME/umaLord`
//! (`~/.config/umaLord`) elsewhere.

use std::path::PathBuf;

use serde_json::Value;

/// The per-user folder for settings, the live-data cache and the assistant's bridge.
pub fn app_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.map(|d| d.join("umaLord"))
}

/// A folder in the user's home, like Downloads or Pictures (`%USERPROFILE%\<name>` on Windows).
pub fn user_dir(name: &str) -> Option<PathBuf> {
    let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(home).map(|h| PathBuf::from(h).join(name))
}

pub fn path() -> Option<PathBuf> {
    app_dir().map(|d| d.join("settings.json"))
}

fn read_all() -> Value {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Default::default()))
}

pub fn get(key: &str) -> Option<Value> {
    read_all().get(key).cloned().filter(|v| !v.is_null())
}

/// Set one key, keeping the others. `Value::Null` removes it.
pub fn set(key: &str, value: Value) -> bool {
    let Some(p) = path() else { return false };
    let mut all = read_all();
    if let Some(map) = all.as_object_mut() {
        if value.is_null() {
            map.remove(key);
        } else {
            map.insert(key.to_string(), value);
        }
    }
    p.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(&p, all.to_string()).is_ok()
}
