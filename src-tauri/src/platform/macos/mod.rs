//! macOS-specific platform code.

/// Set Dock icon visibility via activation policy.
pub fn set_dock_icon_visible(app: &tauri::AppHandle, visible: bool) -> Result<(), String> {
    use tauri::ActivationPolicy;
    let policy = if visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };
    app.set_activation_policy(policy)
        .map_err(|e| format!("Failed to set activation policy: {e}"))
}
