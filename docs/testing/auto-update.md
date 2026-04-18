# Auto-Update Manual Test Matrix

## Pre-release smoke test

Before tagging a real version:

1. Build the app from the current branch: `npm run tauri build`.
2. Temporarily edit `src-tauri/tauri.conf.json` `version` back one patch (e.g. `0.7.1` if current is `0.7.2`). Also bump `package.json` and `src-tauri/Cargo.toml` to match.
3. Re-build and install the older version. Quit.
4. Restore the real version. Build + publish the release with the updated workflow.
5. Launch the older version — within ~10 seconds you should see the banner.

## Per-platform checks

### macOS (Apple Silicon)
- [ ] DMG installer is signed + notarized (unchanged from previous flow)
- [ ] `.app.tar.gz` + `.app.tar.gz.sig` appear in the release assets
- [ ] `latest.json` is present in release assets
- [ ] Banner appears in popover on old version
- [ ] Tray icon shows red dot in top-right corner
- [ ] OS notification fires once per new version (deduped across 6h checks)
- [ ] "Update Now" → download progress → app relaunches on new version
- [ ] "Skip" hides banner; next release re-triggers
- [ ] "Later" dismisses for this session only

### Windows 11
- [ ] NSIS `.exe` + `.nsis.zip` + `.sig` appear in release assets
- [ ] Banner + tray tooltip show on old version
- [ ] "Update Now" runs installer in passive mode and relaunches

### Linux (Ubuntu 22.04)
- [ ] `.deb`, `.AppImage`, `.AppImage.sig` appear in release assets
- [ ] Running via AppImage: full auto-update flow works
- [ ] Running via .deb: banner shows "Download" (not "Update Now") — opens GitHub release page in browser

## Failure-mode checks

### Offline
- [ ] Disconnect network; launch app; no crash, no banner, Settings shows "Last checked: never" or preserved timestamp

### Rate-limited (403)
- [ ] Simulate by pointing `endpoints` at a non-existent 403-returning URL; scheduler should back off 12h → 24h (verify in logs at `~/Library/Logs/TokenMonitor/` on macOS)

### Corrupt signature
- [ ] Manually tamper with `latest.json` `signature` field; install should fail with red banner + error message

## Dev-mode note

The updater plugin does not fetch endpoints in debug builds by default. Verify by running `npm run tauri build` (release) and launching the bundled binary directly — not `npm run tauri dev`.
