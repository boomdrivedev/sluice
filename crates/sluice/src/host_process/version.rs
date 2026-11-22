//! sluice's release numbers, as a command and the host process compare
//! them in `hello` (D8).

/// Whether `version` is older than `own`, both as release numbers: three
/// numbers, dot-separated, compared in order. Anything else is no version,
/// which is never older, nor has any older than it.
pub fn older(version: &str, own: &str) -> bool {
    matches!((release(version), release(own)), (Some(v), Some(o)) if v < o)
}

/// `version`'s three release numbers; `None` when it is no release.
fn release(version: &str) -> Option<[u64; 3]> {
    let number = |p: &str| {
        let digits = !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
        digits.then(|| p.parse().ok()).flatten()
    };
    let mut parts = version.split('.');
    let numbers = [
        number(parts.next()?)?,
        number(parts.next()?)?,
        number(parts.next()?)?,
    ];
    parts.next().is_none().then_some(numbers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 129: versions compare as release numbers; anything else is no
    /// version, which is neither older nor newer than any.
    #[test]
    fn versions_compare_as_release_numbers() {
        const OLD: &str = "0.51.0";
        const NEW: &str = "0.52.0";
        assert!(older("0.9.0", "0.10.0"));
        assert!(!older("0.10.0", "0.9.0"));
        assert!(older("0.0.0", "0.52.0"));
        assert!(older(OLD, NEW));
        assert!(!older(NEW, OLD));
        assert!(!older(NEW, NEW));
        for none in [
            "dev",
            "",
            "0.52",
            "0.52.0.1",
            "0..52",
            "+0.52.0",
            "0.52.0-rc.1",
        ] {
            assert_eq!(release(none), None, "{none:?}");
            assert!(!older(none, NEW) && !older(OLD, none), "{none:?}");
        }
    }
}
