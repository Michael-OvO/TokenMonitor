/// Returns true if `candidate` is strictly newer than `current` by semver.
/// Uses a simple dotted-numeric comparison. Rejects candidates that look like
/// prereleases (contain `-`), per the "stable-only" design decision.
#[allow(dead_code)]
pub fn is_newer(current: &str, candidate: &str) -> bool {
    if candidate.contains('-') {
        return false;
    }
    let parse =
        |s: &str| -> Option<Vec<u64>> { s.split('.').map(|p| p.parse::<u64>().ok()).collect() };
    match (parse(current), parse(candidate)) {
        (Some(c), Some(n)) => n > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch() {
        assert!(is_newer("0.7.2", "0.7.3"));
    }

    #[test]
    fn newer_minor() {
        assert!(is_newer("0.7.2", "0.8.0"));
    }

    #[test]
    fn newer_major() {
        assert!(is_newer("0.7.2", "1.0.0"));
    }

    #[test]
    fn same_version_is_not_newer() {
        assert!(!is_newer("0.7.2", "0.7.2"));
    }

    #[test]
    fn older_is_not_newer() {
        assert!(!is_newer("0.7.2", "0.7.1"));
    }

    #[test]
    fn prerelease_is_rejected() {
        assert!(!is_newer("0.7.2", "0.8.0-beta.1"));
    }

    #[test]
    fn malformed_is_not_newer() {
        assert!(!is_newer("0.7.2", "garbage"));
        assert!(!is_newer("garbage", "0.8.0"));
    }
}
