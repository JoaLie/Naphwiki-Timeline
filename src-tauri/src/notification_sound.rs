use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tauri::{ipc::Response, AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

const SOUND_DATA_FILE: &str = "custom-notification-sound.bin";
const SOUND_METADATA_FILE: &str = "custom-notification-sound.json";
const MAX_SOUND_BYTES: u64 = 10 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 4 * 1024;
const MAIN_WINDOW: &str = "main";
const SETTINGS_WINDOW: &str = "timeline-settings";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomNotificationSoundInfo {
    name: String,
    mime_type: String,
    size_bytes: u64,
}

fn sound_format(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp3") => Some("audio/mpeg"),
        Some("wav") => Some("audio/wav"),
        Some("ogg") | Some("opus") => Some("audio/ogg"),
        Some("m4a") => Some("audio/mp4"),
        Some("aac") => Some("audio/aac"),
        Some("flac") => Some("audio/flac"),
        _ => None,
    }
}

fn ensure_read_access(window: &WebviewWindow) -> Result<(), String> {
    match window.label() {
        MAIN_WINDOW | SETTINGS_WINDOW => Ok(()),
        _ => Err("This window cannot access notification sounds.".to_string()),
    }
}

fn ensure_write_access(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == SETTINGS_WINDOW {
        Ok(())
    } else {
        Err("Notification sounds can only be changed from Timeline settings.".to_string())
    }
}

fn storage_paths(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let directory = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "The application data folder is unavailable.".to_string())?;
    Ok((
        directory.join(SOUND_DATA_FILE),
        directory.join(SOUND_METADATA_FILE),
    ))
}

fn load_sound_info(app: &AppHandle) -> Option<CustomNotificationSoundInfo> {
    let (data_path, metadata_path) = storage_paths(app).ok()?;
    let data_size = fs::metadata(data_path).ok()?.len();
    if data_size == 0 || data_size > MAX_SOUND_BYTES {
        return None;
    }

    let metadata_size = fs::metadata(&metadata_path).ok()?.len();
    if metadata_size == 0 || metadata_size > MAX_METADATA_BYTES {
        return None;
    }

    let metadata = fs::read(metadata_path).ok()?;
    let mut info = serde_json::from_slice::<CustomNotificationSoundInfo>(&metadata).ok()?;
    if info.name.trim().is_empty()
        || info.name.len() > 255
        || !matches!(
            info.mime_type.as_str(),
            "audio/mpeg" | "audio/wav" | "audio/ogg" | "audio/mp4" | "audio/aac" | "audio/flac"
        )
    {
        return None;
    }
    info.size_bytes = data_size;
    Some(info)
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err("The stored notification sound could not be removed.".to_string()),
    }
}

#[tauri::command]
pub fn get_custom_notification_sound_info(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Option<CustomNotificationSoundInfo>, String> {
    ensure_read_access(&window)?;
    Ok(load_sound_info(&app))
}

#[tauri::command]
pub fn read_custom_notification_sound(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Response, String> {
    ensure_read_access(&window)?;
    if load_sound_info(&app).is_none() {
        return Err("No valid custom notification sound is stored.".to_string());
    }

    let (data_path, _) = storage_paths(&app)?;
    let data = fs::read(data_path)
        .map_err(|_| "The custom notification sound could not be read.".to_string())?;
    Ok(Response::new(data))
}

#[tauri::command]
pub fn select_custom_notification_sound(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Option<CustomNotificationSoundInfo>, String> {
    ensure_write_access(&window)?;
    let selected = app
        .dialog()
        .file()
        .add_filter(
            "Audio files",
            &["mp3", "wav", "ogg", "opus", "m4a", "aac", "flac"],
        )
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected
        .into_path()
        .map_err(|_| "Only local audio files can be selected.".to_string())?;
    let mime_type = sound_format(&source)
        .ok_or_else(|| "Choose an MP3, WAV, OGG, OPUS, M4A, AAC, or FLAC file.".to_string())?;
    let source_metadata =
        fs::metadata(&source).map_err(|_| "The selected sound could not be read.".to_string())?;
    if !source_metadata.is_file() || source_metadata.len() == 0 {
        return Err("The selected sound is empty or invalid.".to_string());
    }
    if source_metadata.len() > MAX_SOUND_BYTES {
        return Err("The selected sound is larger than 10 MB.".to_string());
    }

    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty() && name.len() <= 255)
        .ok_or_else(|| "The selected sound has an invalid file name.".to_string())?
        .to_string();
    let data =
        fs::read(&source).map_err(|_| "The selected sound could not be read.".to_string())?;
    let info = CustomNotificationSoundInfo {
        name,
        mime_type: mime_type.to_string(),
        size_bytes: data.len() as u64,
    };
    let metadata = serde_json::to_vec_pretty(&info)
        .map_err(|_| "The custom sound details could not be saved.".to_string())?;
    let (data_path, metadata_path) = storage_paths(&app)?;
    let directory = data_path
        .parent()
        .ok_or_else(|| "The application data folder is unavailable.".to_string())?;
    fs::create_dir_all(directory)
        .map_err(|_| "The application data folder could not be created.".to_string())?;
    fs::write(data_path, data)
        .map_err(|_| "The custom notification sound could not be saved.".to_string())?;
    fs::write(metadata_path, metadata)
        .map_err(|_| "The custom sound details could not be saved.".to_string())?;
    Ok(Some(info))
}

#[tauri::command]
pub fn clear_custom_notification_sound(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<(), String> {
    ensure_write_access(&window)?;
    let (data_path, metadata_path) = storage_paths(&app)?;
    remove_if_present(&data_path)?;
    remove_if_present(&metadata_path)
}

#[cfg(test)]
mod tests {
    use super::sound_format;
    use std::path::Path;

    #[test]
    fn supported_sound_extensions_are_case_insensitive() {
        assert_eq!(sound_format(Path::new("alert.MP3")), Some("audio/mpeg"));
        assert_eq!(sound_format(Path::new("alert.WaV")), Some("audio/wav"));
        assert_eq!(sound_format(Path::new("alert.opus")), Some("audio/ogg"));
    }

    #[test]
    fn unsupported_sound_extensions_are_rejected() {
        assert_eq!(sound_format(Path::new("alert.exe")), None);
        assert_eq!(sound_format(Path::new("alert")), None);
    }
}
