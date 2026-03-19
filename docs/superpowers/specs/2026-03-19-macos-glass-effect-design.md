# macOS Glass Effect & Icon Adaptation

## Overview

Add `NSVisualEffectView` glass background to the popover window across all supported macOS versions, and update app/tray icons for macOS 26 compatibility. On macOS 26+, the system automatically renders these as Liquid Glass; on older versions, the standard vibrancy material applies.

## Goals

- Popover window uses native system blur material instead of opaque CALayer fill
- Glass effect works on all supported macOS versions (13.0+)
- Users can toggle the effect off in Settings
- App icon updated for macOS 26 Liquid Glass icon rendering
- Tray icon verified/adapted for macOS 26 menu bar changes

## Non-Goals

- Custom CSS `backdrop-filter` fallback (we use native `NSVisualEffectView` only)
- Separate builds for different macOS versions

---

## 1. NSVisualEffectView Integration

### 1.1 Rust Changes — `commands.rs`

**Current behavior**: `apply_window_surface()` creates a CALayer on the content view with an opaque background color and rounded corners.

**New behavior**: Insert an `NSVisualEffectView` as a subview of the content view, behind the webview. The CALayer fill becomes semi-transparent to let the blur show through.

```
┌─ NSWindow (transparent, no opaque) ──────────┐
│  ┌─ contentView ───────────────────────────┐  │
│  │  ┌─ NSVisualEffectView (full-frame) ──┐ │  │
│  │  │  material: .popover                 │ │  │
│  │  │  blendingMode: .behindWindow        │ │  │
│  │  │  state: .active                     │ │  │
│  │  │  cornerRadius: 14                   │ │  │
│  │  └────────────────────────────────────┘ │  │
│  │  ┌─ WKWebView (semi-transparent bg) ──┐ │  │
│  │  │  CSS --surface with alpha ~0.65     │ │  │
│  │  └────────────────────────────────────┘ │  │
│  └─────────────────────────────────────────┘  │
└───────────────────────────────────────────────┘
```

**Key implementation details**:

- `NSVisualEffectView` is created once during setup and retained
- Material: `NSVisualEffectMaterial::Popover` — semantically correct for menu bar popovers, and the system maps it to the appropriate glass material per OS version
- Blending mode: `NSVisualEffectBlendingMode::BehindWindow` — blurs desktop content behind the window
- State: `NSVisualEffectState::Active` — keeps the blur active even when the window isn't focused (important since the popover briefly loses focus during certain interactions)
- The `NSVisualEffectView`'s layer owns corner clipping when glass is active: set `masksToBounds = true` and `cornerRadius = 14` on its layer. The content view's CALayer should have its `cornerRadius` cleared (set to 0) to avoid double-clipping artifacts. When glass is disabled, restore the content view CALayer's corner radius.
- Auto-resizing mask set so it tracks the content view's frame

**New function**: `apply_glass_effect(window, enabled)`:
- When `enabled = true`: adds `NSVisualEffectView` as subview at index 0 (behind webview), sets CALayer background to transparent
- When `enabled = false`: removes the `NSVisualEffectView`, restores opaque CALayer background

**Modified function**: `apply_window_surface()`:
- Accepts an additional `glass_enabled: bool` parameter (passed by callers who read from `AppState`)
- If glass is active: sets CALayer background color with the alpha from CSS (semi-transparent)
- If glass is inactive: forces alpha to 0xFF regardless of input (current behavior)

### 1.2 Cargo.toml

Add `NSVisualEffectView` feature to `objc2-app-kit`:

```toml
objc2-app-kit = { version = "0.3.2", features = [
    "NSAppearance", "NSApplication", "NSColor", "NSResponder",
    "NSStatusItem", "NSView", "NSWindow", "NSVisualEffectView",
    "objc2-quartz-core"
] }
```

Note: `NSResponder` is listed explicitly because `NSVisualEffectView` depends on it via the `NSView` hierarchy.

### 1.3 New Tauri Command — `set_glass_effect`

```rust
#[tauri::command]
fn set_glass_effect(app: AppHandle, enabled: bool) -> Result<(), String>
```

Called from frontend when the user toggles the glass setting. Adds/removes the `NSVisualEffectView` and adjusts surface opacity accordingly.

### 1.4 AppState Addition

Add `glass_enabled: Arc<RwLock<bool>>` to `AppState`, defaulting to `true`. The `apply_window_surface()` function reads this to decide whether to use transparent or opaque fill.

---

## 2. CSS Theme Variable Changes — `app.css`

When glass effect is active, `--surface` and `--bg` must be semi-transparent so the native blur shows through.

**Approach**: Add a `data-glass` attribute to the root element. When present, override surface colors with semi-transparent versions.

```css
/* Glass-active overrides — dark (explicit or default) */
[data-glass="true"][data-theme="dark"] {
  --bg: rgba(12, 12, 14, 0.55);
  --surface: rgba(20, 20, 22, 0.60);
}

/* Glass-active overrides — light (explicit) */
[data-glass="true"][data-theme="light"] {
  --bg: rgba(245, 245, 247, 0.55);
  --surface: rgba(255, 255, 255, 0.55);
}

/* Glass-active overrides — system theme (no data-theme attribute) */
/* Dark system preference (default) */
[data-glass="true"]:root:not([data-theme]) {
  --bg: rgba(12, 12, 14, 0.55);
  --surface: rgba(20, 20, 22, 0.60);
}

/* Light system preference */
@media (prefers-color-scheme: light) {
  [data-glass="true"]:root:not([data-theme]) {
    --bg: rgba(245, 245, 247, 0.55);
    --surface: rgba(255, 255, 255, 0.55);
  }
}
```

Alpha values (~0.55–0.65) are tuned so:
- Text remains readable over the blurred background
- The blur is clearly visible, not just a subtle tint
- Provider accent backgrounds (`--provider-bg`) remain distinguishable

**No changes** to `--surface-2`, `--surface-hover`, `--border`, or text colors — these are already relative/translucent.

---

## 3. Frontend Integration — `windowAppearance.ts`

### 3.1 syncNativeWindowSurface

When glass is active, the webview's own `setBackgroundColor` must use alpha=0 (fully transparent) so the `NSVisualEffectView` blur shows through. The semi-transparent tint comes from CSS `--surface` only — the native webview background must not paint over the blur.

When glass is inactive, `setBackgroundColor` uses the opaque `--surface` value (current behavior).

```typescript
// When glass is active:
await getCurrentWebviewWindow().setBackgroundColor({ red: 0, green: 0, blue: 0, alpha: 0 });
// surface color (semi-transparent) is sent only to set_window_surface for the CALayer

// When glass is inactive:
await getCurrentWebviewWindow().setBackgroundColor(surface); // opaque, current behavior
```

`syncNativeWindowSurface()` needs a `glassEnabled` parameter or reads from a shared state to branch on this.

### 3.2 Glass State Management

In `App.svelte` or a new utility:

```typescript
// On mount, read glass setting from store and apply
document.documentElement.setAttribute('data-glass', glassEnabled ? 'true' : 'false');

// When toggled in Settings, update attribute + call set_glass_effect command
```

### 3.3 Settings Store

Add `glassEffect: boolean` to the settings store (default: `true`). Persisted via `tauri-plugin-store`.

---

## 4. Settings UI — `Settings.svelte`

Add a toggle in the Appearance section:

```
Glass Effect    [toggle]
```

Label: "Glass Effect" (or "玻璃效果" if i18n)
Description: Blurs the desktop behind the window

When toggled:
1. Update store
2. Set `data-glass` attribute on root
3. Call `set_glass_effect` command
4. Call `syncNativeWindowSurface()` to update surface alpha

---

## 5. App Icon — `src-tauri/icons/`

### macOS 26 Liquid Glass Compatibility

macOS 26 applies Liquid Glass rendering to app icons automatically. For best results:

- **Simplify the icon design**: remove internal gradients and shadows — the system adds its own depth and light refraction
- **Use a single foreground layer** with clear silhouette on transparent background
- **Maintain high contrast** between the icon shape and the background
- **Update all icon variants**: `icon.icns`, `icon.png`, `128x128.png`, `128x128@2x.png`, `32x32.png`

The icon design itself is a creative/asset task; the spec defines the technical requirements:
- Format: ICNS with 16×16 through 1024×1024 representations
- Background: transparent (let macOS apply its own shape mask and glass)
- Single-color or minimal-color foreground for clean glass rendering

---

## 6. Tray Icon — `tray_render.rs`

### macOS 26 Menu Bar

- Verify that template mode (`icon_as_template(true)`) continues to work correctly — macOS 26 may apply additional visual treatments to template icons
- Test the dynamically-rendered progress bar overlays render correctly against the new menu bar style
- The current 44×44 @2x size should remain correct (standard `NSStatusItem` icon size)
- If macOS 26 changes the menu bar chrome, the `is_menu_bar_dark()` detection may need updating to handle new appearance names

No proactive code changes for the tray icon — verify and fix if needed during implementation.

---

## 7. Initialization Flow

```
app.setup()
  → apply_default_window_surface(window)  // opaque CALayer (safe default)
  → frontend loads
    → reads glass setting from store
    → sets data-glass attribute (CSS becomes semi-transparent if glass=true)
    → calls set_glass_effect(enabled)     // adds/removes NSVisualEffectView
    → syncNativeWindowSurface()           // syncs CSS alpha to native layer
```

This avoids the flash-of-glass-then-opaque race: the window starts opaque, and the frontend applies the stored setting once loaded. The popover is hidden by default (`visible: false`), so the user never sees the opaque-to-glass transition.

On theme change:
```
CSS variables update (--surface becomes semi-transparent)
  → syncNativeWindowSurface()
    → set_window_surface command with new alpha
    → CALayer updates to new semi-transparent color
    → NSVisualEffectView remains, blur shows through
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `src-tauri/Cargo.toml` | Add `NSVisualEffectView` feature |
| `src-tauri/src/commands.rs` | Add `apply_glass_effect()`, `set_glass_effect` command, modify `apply_window_surface()` |
| `src-tauri/src/lib.rs` | Register `set_glass_effect` command, call `apply_glass_effect` in setup |
| `src/app.css` | Add `[data-glass]` semi-transparent surface overrides |
| `src/lib/windowAppearance.ts` | Add glass-aware branching: transparent webview bg when glass active, opaque when not |
| `src/lib/stores/settings.ts` | Add `glassEffect` setting |
| `src/lib/components/Settings.svelte` | Add glass effect toggle |
| `src/App.svelte` | Read glass setting, set `data-glass` attribute |
| `src-tauri/icons/*` | Update app icon for Liquid Glass compatibility |
| `src-tauri/src/tray_render.rs` | Verify/fix tray icon for macOS 26 (if needed) |

## 9. Testing

- Toggle glass effect on/off — window should switch between translucent blur and opaque background
- Theme switching (dark/light/auto) with glass active — surface alpha should update correctly
- "Reduce Transparency" accessibility setting — `NSVisualEffectView` automatically falls back to opaque
- Window resize — glass effect should track window size changes
- Provider switching — accent backgrounds should remain visible over glass
- Tray icon rendering — progress bars should display correctly
