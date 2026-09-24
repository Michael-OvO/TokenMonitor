//! Gate for the popover's hide-on-focus-loss.
//!
//! Some commands knowingly cost the main window its focus (opening a native
//! dialog, switching the Dock activation policy, creating the float ball) and
//! must not dismiss the popover as a side effect. They arm the gate, and the
//! blur handler consumes the arm instead of hiding. The arm is for that one
//! blur only, so two rules keep it from swallowing a real dismissal later:
//! it is refused while the popover is hidden (a hidden window never blurs;
//! startup applies the Dock setting before the popover is first shown, and a
//! leftover arm there used to eat the first click-away after launch), and it
//! expires, so an arm whose blur never came cannot outlive it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an arm waits for its blur before it lapses.
pub const ARM_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct AutoHideGate {
    armed_until: Mutex<Option<Instant>>,
}

impl AutoHideGate {
    /// Arm for the focus loss the caller is about to cause. Returns whether
    /// the gate armed; it does not while the popover is hidden.
    pub fn arm(&self, popover_visible: bool) -> bool {
        self.arm_at(popover_visible, Instant::now())
    }

    /// Called by the blur handler: whether this blur should be ignored.
    /// Consumes the arm either way.
    pub fn take(&self) -> bool {
        self.take_at(Instant::now())
    }

    fn arm_at(&self, popover_visible: bool, now: Instant) -> bool {
        if !popover_visible {
            return false;
        }
        *self.lock() = Some(now + ARM_TTL);
        true
    }

    fn take_at(&self, now: Instant) -> bool {
        matches!(self.lock().take(), Some(until) if now <= until)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Instant>> {
        // The guarded value is a plain Option, so a poisoned lock is still usable.
        self.armed_until
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn take_without_an_arm_lets_the_blur_hide() {
        let gate = AutoHideGate::default();
        assert!(!gate.take_at(Instant::now()));
    }

    #[test]
    fn refuses_to_arm_while_the_popover_is_hidden() {
        let gate = AutoHideGate::default();
        let t0 = Instant::now();
        assert!(!gate.arm_at(false, t0));
        assert!(!gate.take_at(t0 + ms(150)));
    }

    #[test]
    fn an_arm_swallows_exactly_one_blur() {
        let gate = AutoHideGate::default();
        let t0 = Instant::now();
        assert!(gate.arm_at(true, t0));
        assert!(gate.take_at(t0 + ms(150)));
        assert!(!gate.take_at(t0 + ms(300)));
    }

    #[test]
    fn an_arm_whose_blur_never_came_lapses() {
        let gate = AutoHideGate::default();
        let t0 = Instant::now();
        assert!(gate.arm_at(true, t0));
        assert!(
            gate.take_at(t0 + ARM_TTL),
            "a blur at the deadline still counts"
        );
        assert!(gate.arm_at(true, t0));
        assert!(!gate.take_at(t0 + ARM_TTL + ms(1)));
    }

    #[test]
    fn re_arming_moves_the_deadline() {
        let gate = AutoHideGate::default();
        let t0 = Instant::now();
        assert!(gate.arm_at(true, t0));
        assert!(gate.arm_at(true, t0 + ms(2000)));
        assert!(gate.take_at(t0 + ARM_TTL + ms(1000)));
    }
}
