use crate::logging::LoggingState;
use tauri::State;

#[tauri::command]
pub async fn log_frontend_message(
    state: State<'_, LoggingState>,
    level: String,
    category: String,
    message: String,
) -> Result<(), String> {
    state.write_frontend_log(&level, &category, &message);
    Ok(())
}

#[tauri::command]
pub async fn set_log_level(state: State<'_, LoggingState>, level: String) -> Result<(), String> {
    tracing::info!(new_level = %level, "Log level changed via Settings");
    state.set_level(&level)
}

#[tauri::command]
pub async fn get_log_dir(state: State<'_, LoggingState>) -> Result<String, String> {
    Ok(state.log_dir.to_string_lossy().to_string())
}
