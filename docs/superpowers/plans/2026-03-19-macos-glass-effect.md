# macOS Glass Effect & Icon Adaptation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add NSVisualEffectView glass background to the popover window with a Settings toggle, and update icons for macOS 26 Liquid Glass compatibility.

**Architecture:** Native `NSVisualEffectView` inserted behind the WKWebView via Objective-C interop (`objc2-app-kit`). CSS surfaces become semi-transparent when glass is active so the system blur shows through. Frontend controls glass state via a Tauri command and a `data-glass` attribute on the root element.

**Tech Stack:** Rust + objc2-app-kit (NSVisualEffectView), Tauri v2, Svelte 5, CSS custom properties

**Spec:** `docs/superpowers/specs/2026-03-19-macos-glass-effect-design.md`

---

### Task 1: Add NSVisualEffectView Cargo dependency

**Files:**
- Modify: `src-tauri/Cargo.toml:25`

- [ ] **Step 1: Add NSVisualEffectView and NSResponder features**

In `src-tauri/Cargo.toml`, update the `objc2-app-kit` line:

```toml
objc2-app-kit = { version = "0.3.2", features = ["NSAppearance", "NSApplication", "NSColor", "NSGraphics", "NSResponder", "NSStatusItem", "NSView", "NSWindow", "NSVisualEffectView", "objc2-quartz-core"] }
```

Note: `NSGraphics` is required because `addSubview_positioned_relativeTo` is gated behind it.

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: compiles without errors

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml
git commit -m "deps: add NSVisualEffectView and NSResponder features to objc2-app-kit"
```

---

### Task 2: Implement `apply_glass_effect` and `set_glass_effect` in Rust

**Files:**
- Modify: `src-tauri/src/commands.rs:1-17` (imports), `src-tauri/src/commands.rs:55-75` (AppState), `src-tauri/src/commands.rs:112-166` (apply_window_surface + new functions)
- Modify: `src-tauri/src/lib.rs:110` (register command)

- [ ] **Step 1: Add imports in commands.rs**

Add to the `#[cfg(target_os = "macos")]` import block at the top of `commands.rs`:

```rust
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
};
```

No need to import `NSObject` — we identify the effect view by checking its class type.

- [ ] **Step 2: Add `glass_enabled` to AppState**

In `AppState` struct, add:

```rust
pub glass_enabled: Arc<RwLock<bool>>,
```

In `AppState::new()`, add:

```rust
glass_enabled: Arc::new(RwLock::new(true)),
```

- [ ] **Step 3: Implement `apply_glass_effect` function**

Add this function after `apply_window_surface`:

```rust
#[cfg(target_os = "macos")]
fn apply_glass_effect(
    window: &tauri::WebviewWindow,
    enabled: bool,
    corner_radius: f64,
) -> Result<(), String> {
    use objc2::ClassType;

    let ns_window = window
        .ns_window()
        .map_err(|e| format!("Failed to access NSWindow: {e}"))?;
    let ns_window = unsafe { &*(ns_window.cast::<NSWindow>()) };
    let content_view = ns_window
        .contentView()
        .ok_or_else(|| String::from("NSWindow is missing a content view"))?;

    // Helper: find existing NSVisualEffectView among subviews (by class type)
    let find_effect_view = || -> Option<usize> {
        let subviews = content_view.subviews();
        for i in 0..subviews.len() {
            if subviews[i].is_kind_of::<NSVisualEffectView>() {
                return Some(i);
            }
        }
        None
    };

    if enabled {
        if find_effect_view().is_none() {
            let frame = content_view.frame();
            let effect_view =
                unsafe { NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(), frame) };
            effect_view.setMaterial(NSVisualEffectMaterial::Popover);
            effect_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            effect_view.setState(NSVisualEffectState::Active);

            // Auto-resize with parent
            unsafe {
                effect_view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
            }

            // Corner radius on the effect view's layer
            effect_view.setWantsLayer(true);
            if let Some(layer) = effect_view.layer() {
                layer.setCornerRadius(corner_radius);
                layer.setMasksToBounds(true);
            }

            // Insert behind all other subviews (behind webview)
            unsafe {
                content_view.addSubview_positioned_relativeTo(
                    &effect_view,
                    NSWindowOrderingMode::Below,
                    None,
                );
            }
        }

        // Clear corner radius from content view's own layer (effect view handles it)
        if let Some(layer) = content_view.layer() {
            layer.setCornerRadius(0.0);
        }
    } else {
        // Remove the visual effect view by class type
        if let Some(idx) = find_effect_view() {
            let subviews = content_view.subviews();
            subviews[idx].removeFromSuperview();
        }

        // Restore corner radius on content view's own layer
        if let Some(layer) = content_view.layer() {
            layer.setCornerRadius(corner_radius);
            layer.setMasksToBounds(true);
        }
    }

    Ok(())
}
```

Note: We identify the visual effect view by class type (`is_kind_of::<NSVisualEffectView>()`) instead of tags, since `NSView::setTag()` is not exposed in `objc2-app-kit` 0.3.2.

- [ ] **Step 4: Modify `apply_window_surface` to accept `glass_enabled`**

Update the signature:

```rust
#[cfg(target_os = "macos")]
fn apply_window_surface(
    window: &tauri::WebviewWindow,
    surface: WindowSurface,
    corner_radius: f64,
    glass_enabled: bool,
) -> Result<(), String> {
```

Inside the function, after computing `color`, override alpha when glass is disabled:

```rust
let effective_alpha = if glass_enabled {
    f64::from(surface.alpha) / 255.0
} else {
    1.0 // Force opaque when glass is off
};

let color = NSColor::colorWithSRGBRed_green_blue_alpha(
    f64::from(surface.red) / 255.0,
    f64::from(surface.green) / 255.0,
    f64::from(surface.blue) / 255.0,
    effective_alpha,
);
```

When glass is enabled, don't set corner radius on content view layer (the effect view owns it):

```rust
if !glass_enabled {
    layer.setCornerRadius(corner_radius);
}
layer.setMasksToBounds(true);
```

- [ ] **Step 5: Update `apply_default_window_surface` and `set_window_surface` callers**

`apply_default_window_surface` should pass `false` for glass_enabled (safe opaque default at startup):

```rust
#[cfg(target_os = "macos")]
pub fn apply_default_window_surface(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| String::from("Main window not found"))?;
    apply_window_surface(&window, DEFAULT_DARK_SURFACE, DEFAULT_WINDOW_CORNER_RADIUS, false)
}
```

Update the existing `set_window_surface` Tauri command (currently at line 317) to read glass state. Add `state: State<'_, AppState>` parameter and pass `glass` to `apply_window_surface`:

```rust
#[tauri::command]
pub async fn set_window_surface(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    surface: WindowSurface,
    corner_radius: Option<f64>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let glass = *state.glass_enabled.read().await;
        let window = app
            .get_webview_window("main")
            .ok_or_else(|| String::from("Main window not found"))?;
        let next_radius = corner_radius.unwrap_or(DEFAULT_WINDOW_CORNER_RADIUS);
        let (tx, rx) = oneshot::channel();
        let window_for_main_thread = window.clone();

        window
            .run_on_main_thread(move || {
                let _ = tx.send(apply_window_surface(
                    &window_for_main_thread,
                    surface,
                    next_radius,
                    glass,
                ));
            })
            .map_err(|e| format!("Failed to schedule native window surface update: {e}"))?;

        return rx
            .await
            .map_err(|_| String::from("Native window surface update was cancelled"))?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, surface, corner_radius);
        Ok(())
    }
}
```

- [ ] **Step 6: Add `set_glass_effect` Tauri command**

```rust
#[tauri::command]
pub async fn set_glass_effect(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    *state.glass_enabled.write().await = enabled;

    #[cfg(target_os = "macos")]
    {
        let window = app
            .get_webview_window("main")
            .ok_or_else(|| String::from("Main window not found"))?;
        let (tx, rx) = oneshot::channel();
        let window_clone = window.clone();

        // AppKit operations MUST run on the main thread
        window
            .run_on_main_thread(move || {
                let _ = tx.send(apply_glass_effect(
                    &window_clone,
                    enabled,
                    DEFAULT_WINDOW_CORNER_RADIUS,
                ));
            })
            .map_err(|e| format!("Failed to schedule glass effect update: {e}"))?;

        return rx
            .await
            .map_err(|_| String::from("Glass effect update was cancelled"))?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(())
    }
}
```

Important: All NSView/AppKit operations must use `run_on_main_thread`, matching the existing pattern in `set_window_surface` (see `commands.rs:328-339`).

- [ ] **Step 7: Register the command in lib.rs**

In `src-tauri/src/lib.rs`, add `commands::set_glass_effect` to the `invoke_handler`:

```rust
.invoke_handler(tauri::generate_handler![
    commands::get_usage_data,
    commands::get_monthly_usage,
    commands::get_known_models,
    commands::get_last_usage_debug,
    commands::set_window_surface,
    commands::set_refresh_interval,
    commands::set_tray_config,
    commands::clear_cache,
    commands::get_rate_limits,
    commands::set_glass_effect,
])
```

- [ ] **Step 8: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: compiles without errors

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(glass): add NSVisualEffectView glass effect with set_glass_effect command"
```

---

### Task 3: Add glass CSS overrides in `app.css`

**Files:**
- Modify: `src/app.css` (after line 127, before the `@keyframes` block)

- [ ] **Step 1: Add glass-active CSS overrides**

Insert after the closing `}` of the `@media (prefers-color-scheme: light)` block (line 127) and before `@keyframes popIn`:

```css
/* ── Glass effect overrides (semi-transparent surfaces for native blur) ── */
[data-glass="true"][data-theme="dark"] {
  --bg: rgba(12, 12, 14, 0.55);
  --surface: rgba(20, 20, 22, 0.60);
}
[data-glass="true"][data-theme="light"] {
  --bg: rgba(245, 245, 247, 0.55);
  --surface: rgba(255, 255, 255, 0.55);
}
/* System theme: dark default */
[data-glass="true"]:root:not([data-theme]) {
  --bg: rgba(12, 12, 14, 0.55);
  --surface: rgba(20, 20, 22, 0.60);
}
@media (prefers-color-scheme: light) {
  [data-glass="true"]:root:not([data-theme]) {
    --bg: rgba(245, 245, 247, 0.55);
    --surface: rgba(255, 255, 255, 0.55);
  }
}
```

- [ ] **Step 2: Verify the CSS is valid**

Run: `npm run check` (or `npx svelte-check` if available)
Expected: no CSS parsing errors

- [ ] **Step 3: Commit**

```bash
git add src/app.css
git commit -m "feat(glass): add semi-transparent surface CSS overrides for glass effect"
```

---

### Task 4: Add `glassEffect` to Settings store

**Files:**
- Modify: `src/lib/stores/settings.ts:6-18` (Settings interface), `src/lib/stores/settings.ts:21-41` (DEFAULTS)

- [ ] **Step 1: Add `glassEffect` to the Settings interface**

In `src/lib/stores/settings.ts`, add to the `Settings` interface:

```typescript
export interface Settings {
  // ... existing fields ...
  glassEffect: boolean;
}
```

- [ ] **Step 2: Add default value**

In the `DEFAULTS` object:

```typescript
const DEFAULTS: Settings = {
  // ... existing fields ...
  glassEffect: true,
};
```

- [ ] **Step 3: Add `applyGlass` helper function**

After `applyProvider`:

```typescript
export function applyGlass(enabled: boolean) {
  const root = document.documentElement;
  root.setAttribute("data-glass", enabled ? "true" : "false");
}
```

- [ ] **Step 4: Commit**

```bash
git add src/lib/stores/settings.ts
git commit -m "feat(glass): add glassEffect setting with applyGlass helper"
```

---

### Task 5: Update `windowAppearance.ts` for glass-aware webview background

**Files:**
- Modify: `src/lib/windowAppearance.ts:95-112`

- [ ] **Step 1: Add `glassEnabled` parameter to `syncNativeWindowSurface`**

```typescript
export async function syncNativeWindowSurface(
  invokeFn: typeof invoke = invoke,
  glassEnabled: boolean = false,
): Promise<void> {
  if (typeof document === "undefined") return;

  const surface = readSurfaceColor();
  if (!surface) return;

  logResizeDebug("native-surface:sync-request", { surface, glassEnabled });

  const webviewBg = glassEnabled
    ? { red: 0, green: 0, blue: 0, alpha: 0 }
    : surface;

  await Promise.all([
    getCurrentWebviewWindow().setBackgroundColor(webviewBg),
    invokeFn("set_window_surface", {
      surface,
      cornerRadius: WINDOW_CORNER_RADIUS,
    }),
  ]);
  logResizeDebug("native-surface:sync-resolved", { surface, glassEnabled });
}
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/windowAppearance.ts
git commit -m "feat(glass): make webview background transparent when glass is active"
```

---

### Task 6: Update `bootstrap.ts` to apply glass on startup

**Files:**
- Modify: `src/lib/bootstrap.ts`

- [ ] **Step 1: Update `StartupDeps` and `initializeRuntimeFromSettings`**

```typescript
import { invoke } from "@tauri-apps/api/core";
import { activePeriod, activeProvider } from "./stores/usage.js";
import { applyGlass, applyTheme, type Settings } from "./stores/settings.js";
import { syncTrayConfig } from "./traySync.js";
import { syncNativeWindowSurface } from "./windowAppearance.js";

type StartupDeps = {
  invokeFn?: typeof invoke;
  applyThemeFn?: typeof applyTheme;
  applyGlassFn?: typeof applyGlass;
  syncNativeWindowSurfaceFn?: (invokeFn?: typeof invoke, glassEnabled?: boolean) => Promise<void>;
};

export async function initializeRuntimeFromSettings(
  saved: Settings,
  deps: StartupDeps = {},
) {
  const invokeFn = deps.invokeFn ?? invoke;
  const applyThemeFn = deps.applyThemeFn ?? applyTheme;
  const applyGlassFn = deps.applyGlassFn ?? applyGlass;
  const syncNativeWindowSurfaceFn =
    deps.syncNativeWindowSurfaceFn ?? syncNativeWindowSurface;

  applyThemeFn(saved.theme);
  applyGlassFn(saved.glassEffect);
  activeProvider.set(saved.defaultProvider);
  activePeriod.set(saved.defaultPeriod);

  try {
    // Enable/disable native glass effect
    await invokeFn("set_glass_effect", { enabled: saved.glassEffect });
  } catch {
    // Keep startup resilient
  }

  try {
    await syncNativeWindowSurfaceFn(invokeFn, saved.glassEffect);
  } catch {
    // Keep startup resilient if the backend IPC is not ready yet.
  }

  try {
    await invokeFn("set_refresh_interval", { interval: saved.refreshInterval });
    await syncTrayConfig(saved.trayConfig, null, invokeFn);
  } catch {
    // Keep startup resilient if the backend IPC is not ready yet.
  }

  return {
    provider: saved.defaultProvider,
    period: saved.defaultPeriod,
  };
}
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/bootstrap.ts
git commit -m "feat(glass): apply glass effect during bootstrap"
```

---

### Task 7: Add Glass Effect toggle to Settings UI

**Files:**
- Modify: `src/lib/components/Settings.svelte`

- [ ] **Step 1: Import `applyGlass` and add handler**

At the top of the `<script>` block, update the import from settings store:

```typescript
import { settings, updateSetting, applyTheme, applyGlass, type Settings as SettingsType } from "../stores/settings.js";
```

Add a handler function alongside the others:

```typescript
async function handleGlassEffect(checked: boolean) {
  updateSetting("glassEffect", checked);
  applyGlass(checked);
  try {
    await invoke("set_glass_effect", { enabled: checked });
    await syncNativeWindowSurface(invoke, checked);
  } catch (e) {
    console.error("Failed to toggle glass effect:", e);
  }
}
```

- [ ] **Step 2: Update the `handleTheme` function**

The theme change also needs to sync the native surface with current glass state:

```typescript
function handleTheme(val: string) {
  const theme = val as SettingsType["theme"];
  updateSetting("theme", theme);
  applyTheme(theme);
  void syncNativeWindowSurface(invoke, current.glassEffect).catch(() => {});
}
```

- [ ] **Step 3: Add the toggle to the General section**

In the template, after the "Brand Theming" row and before the closing `</div>` of the General card, add:

```svelte
        <div class="row border">
          <span class="label">Brand Theming</span>
          <ToggleSwitch
            checked={current.brandTheming}
            onChange={handleBrandTheming}
          />
        </div>
        <div class="row">
          <span class="label">Glass Effect</span>
          <ToggleSwitch
            checked={current.glassEffect}
            onChange={handleGlassEffect}
          />
        </div>
```

Note: The existing "Brand Theming" row needs `border` class added (it's the last row currently, so has no border — but now Glass Effect follows it). And "Glass Effect" becomes the last row (no border).

- [ ] **Step 4: Add `glassEffect` to the local `current` state default**

In the `current` state initialization at the top:

```typescript
let current = $state<SettingsType>({
  // ... existing fields ...
  glassEffect: true,
});
```

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/Settings.svelte
git commit -m "feat(glass): add Glass Effect toggle to Settings UI"
```

---

### Task 8: Update callers of `syncNativeWindowSurface` in App.svelte

**Files:**
- Modify: `src/App.svelte`

- [ ] **Step 1: Find and update all `syncNativeWindowSurface` calls**

Search for `syncNativeWindowSurface` in `App.svelte`. Each call needs to pass the current glass state. The glass state can be derived from the settings store:

```typescript
import { applyGlass } from "./lib/stores/settings.js";
```

For calls like:
```typescript
void syncNativeWindowSurface().catch(() => {});
```

Update to:
```typescript
void syncNativeWindowSurface(undefined, get(settings).glassEffect).catch(() => {});
```

Identify all occurrences in App.svelte and update them.

- [ ] **Step 2: Commit**

```bash
git add src/App.svelte
git commit -m "feat(glass): pass glass state to syncNativeWindowSurface calls in App"
```

---

### Task 9: Update bootstrap tests

**Files:**
- Modify: `src/lib/bootstrap.test.ts`

- [ ] **Step 1: Update `makeSettings` helper and existing test fixtures**

Add `glassEffect: true` to `makeSettings` defaults (alongside the existing fields) and add `claudePlan: 0, codexPlan: 0` if not already present:

```typescript
function makeSettings(overrides: Partial<Settings> = {}): Settings {
  return {
    // ... existing fields ...
    claudePlan: 0,
    codexPlan: 0,
    glassEffect: true,
    ...overrides,
  };
}
```

- [ ] **Step 2: Update existing test assertions**

The existing tests assert `syncNativeWindowSurfaceFn` is called with one argument: `(invokeFn)`. Now it's called with two: `(invokeFn, glassEnabled)`. Update:

```typescript
// Before:
expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn);
// After:
expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, true);
```

Also add `applyGlassFn` mock to existing test `deps` objects so the new `applyGlassFn` parameter doesn't cause issues:

```typescript
const applyGlassFn = vi.fn();
// pass in deps: { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowSurfaceFn }
```

- [ ] **Step 3: Add test for glass initialization**

```typescript
it("applies glass effect on startup", async () => {
  const invokeFn = vi.fn().mockResolvedValue(undefined);
  const applyGlassFn = vi.fn();
  const applyThemeFn = vi.fn();
  const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

  await initializeRuntimeFromSettings(
    makeSettings({ glassEffect: true }),
    { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowSurfaceFn },
  );

  expect(applyGlassFn).toHaveBeenCalledWith(true);
  expect(invokeFn).toHaveBeenCalledWith("set_glass_effect", { enabled: true });
  expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, true);
});

it("does not enable glass when setting is false", async () => {
  const invokeFn = vi.fn().mockResolvedValue(undefined);
  const applyGlassFn = vi.fn();
  const applyThemeFn = vi.fn();
  const syncNativeWindowSurfaceFn = vi.fn().mockResolvedValue(undefined);

  await initializeRuntimeFromSettings(
    makeSettings({ glassEffect: false }),
    { invokeFn, applyThemeFn, applyGlassFn, syncNativeWindowSurfaceFn },
  );

  expect(applyGlassFn).toHaveBeenCalledWith(false);
  expect(invokeFn).toHaveBeenCalledWith("set_glass_effect", { enabled: false });
  expect(syncNativeWindowSurfaceFn).toHaveBeenCalledWith(invokeFn, false);
});
```

- [ ] **Step 3: Run tests**

Run: `npm test`
Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
git add src/lib/bootstrap.test.ts
git commit -m "test(glass): add bootstrap tests for glass effect initialization"
```

---

### Task 10: Manual integration testing

- [ ] **Step 1: Run the app**

Run: `npm run tauri dev`

- [ ] **Step 2: Verify glass effect is visible**

Expected: the popover window shows a blurred background (desktop content visible through the semi-transparent surface)

- [ ] **Step 3: Toggle glass off in Settings**

Expected: window immediately switches to opaque background (current look)

- [ ] **Step 4: Toggle glass back on**

Expected: blur returns

- [ ] **Step 5: Switch themes (dark → light → system) with glass on**

Expected: surface color changes but blur remains visible through each theme

- [ ] **Step 6: Verify provider switching**

Expected: provider accent tints are still visible over the glass background

- [ ] **Step 7: Test window resize**

Expected: glass effect tracks the window height changes without visual glitches

---

### Task 11: App Icon — macOS 26 Liquid Glass preparation

**Files:**
- Modify: `src-tauri/icons/icon.png`, `icon.icns`, `128x128.png`, `128x128@2x.png`, `32x32.png`

This is a creative/asset task. The current icon needs to be evaluated and potentially simplified for Liquid Glass rendering:

- [ ] **Step 1: Review current icon**

Open `src-tauri/icons/icon.png` and evaluate whether it has:
- Complex gradients or shadows that would conflict with system-applied glass
- Multi-layered design that would look muddy under glass refraction
- Good silhouette/contrast on transparent background

- [ ] **Step 2: If needed, create simplified icon variant**

Design guidelines:
- Single foreground shape on transparent background
- No internal shadows or gradients (system adds glass depth)
- High contrast outline/fill
- Export at all required sizes: 32×32, 128×128, 128×128@2x, 1024×1024
- Generate `.icns` from the set using `iconutil`

```bash
# Create iconset directory
mkdir -p icon.iconset
cp 32x32.png icon.iconset/icon_32x32.png
cp 128x128.png icon.iconset/icon_128x128.png
cp 128x128@2x.png icon.iconset/icon_128x128@2x.png
# ... add all required sizes ...
iconutil -c icns icon.iconset -o icon.icns
```

- [ ] **Step 3: Replace icon files and commit**

```bash
git add src-tauri/icons/
git commit -m "feat(icons): update app icon for macOS 26 Liquid Glass compatibility"
```

---

### Task 12: Tray icon verification

**Files:**
- Review: `src-tauri/src/tray_render.rs`

- [ ] **Step 1: Test tray icon rendering**

With the app running (`npm run tauri dev`), verify:
- Template icon renders correctly in the menu bar
- Progress bar overlays display properly
- Dark/light menu bar detection works
- Icon does not appear clipped or distorted

- [ ] **Step 2: If issues found, fix tray_render.rs**

Check if macOS 26 (when available) introduces new appearance names beyond `NSAppearanceNameAqua` and `NSAppearanceNameDarkAqua`.

No commit needed unless fixes are required.
