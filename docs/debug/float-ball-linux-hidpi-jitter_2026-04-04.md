# Float Ball Linux HiDPI Jitter

## Summary

The floating ball jitter on Linux was caused by the resize path moving the
window through GTK/GDK while drag moves still used Tauri's physical-pixel
positioning API. On HiDPI displays, expand/collapse repositioning drifted away
from the authoritative `last_rect`, so subsequent drags started from stale
coordinates and the ball appeared to jump around.

## Evidence

Observed in:

- `/home/thomas/.local/share/com.tokenmonitor.app/logs/backend.log.2026-04-05`
- `/home/thomas/.local/share/com.tokenmonitor.app/logs/frontend.log.2026-04-05`

Representative backend mismatches:

- `2026-04-05T00:54:36.906889Z`
  expected expand rect: `FloatBallRect { x: 2963, y: 1545, width: 304, height: 112 }`
  actual probed rect: `FloatBallRect { x: 3536, y: 2048, width: 304, height: 112 }`
- `2026-04-05T00:54:39.268800Z`
  expected expand rect: `FloatBallRect { x: 2995, y: 505, width: 304, height: 112 }`
  actual probed rect: `FloatBallRect { x: 3536, y: 1010, width: 304, height: 112 }`
- `2026-04-05T00:54:44.184969Z`
  expected collapse rect: `FloatBallRect { x: 3227, y: 717, width: 112, height: 112 }`
  actual probed rect: `FloatBallRect { x: 3536, y: 1434, width: 112, height: 112 }`

Representative frontend correlation:

- `2026-04-05T00:54:36.706Z` drag ended at `(3155, 1545)`
- `2026-04-05T00:54:36.803Z` next pointer-down happened at screen coordinates
  consistent with the moved window
- `2026-04-05T00:54:37.741Z` after expand, pointer-down happened at screen
  coordinates consistent with the probed wrong rect instead

This shows the probe was not a false positive: the visible window really moved
to the wrong place after expand/collapse.

## Hypotheses

1. `current_float_ball_rect()` was only reading stale WM state.
   Rejected: frontend pointer coordinates matched the probed wrong rect after
   expand/collapse.
2. Frontend drag scaling was the main cause.
   Rejected as root cause: drag moves landed correctly before resize, then the
   window jumped only after expand/collapse.
3. Linux resize flow mixed incompatible positioning APIs.
   Confirmed: drag uses `window.set_position(...)`, while resize used
   `gdk_window.move_resize(...)` / `GtkWindowExt::move_(...)`.

## Fix

Initial mitigation:

Changed `src-tauri/src/commands/float_ball.rs`:

- Linux resize flow now keeps GTK helper logic focused on size negotiation.
- Final position during resize is applied through
  `window.set_position(tauri::PhysicalPosition::new(rect.x, rect.y))`.
- Removed GTK direct move calls from `set_gtk_float_ball_size()`.

Follow-up after newer logs on April 5, 2026:

- The backend started logging `float_ball probe corrective reapply matched`,
  which proved the window was first being placed at the wrong collapsed rect
  and only then corrected by the probe path.
- The Linux resize path was updated again so `set_gtk_float_ball_size()` now
  converts the target rect to logical coordinates and performs the GTK/GDK
  move plus resize on the GTK main thread in one native step.
- The extra resize-time `window.set_position(...)` call was removed so collapse
  no longer depends on a late corrective jump.

Second follow-up after more right-edge repros on April 5, 2026:

- The latest logs still showed collapse targeting `x=3712, width=112` but
  landing first at `x=3536`, exactly `3840 - 304`.
- That pattern indicates the WM was still clamping the move using the previous
  expanded width, even though the final collapsed width was already `112`.
- The Linux resize path was tightened further so GTK owns the resize sequence:
  `set_gtk_float_ball_size()` now applies fixed geometry hints for the target
  logical size, sets north-west gravity, performs native move/resize, and
  re-applies the same logical rect on the next GTK idle tick.
- Tauri `set_min_size()` / `set_max_size()` / `set_size()` were removed from
  the Linux resize hot path so stale width hints cannot race the WM anymore.

## Validation

- `cargo fmt --manifest-path src-tauri/Cargo.toml --all`
- `cargo test --manifest-path src-tauri/Cargo.toml float_ball -- --nocapture`

Result: formatting check passed and 18 float-ball tests passed.
