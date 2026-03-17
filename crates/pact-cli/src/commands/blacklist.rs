//! `pact blacklist` — manage drift detection exclusions.
//!
//! Paths in the blacklist are excluded from drift detection.
//! Default blacklist: /tmp/**, /var/log/**, /proc/**, /sys/**, /dev/**, /run/user/**.

/// Blacklist operation result.
#[derive(Debug, Clone)]
pub struct BlacklistResult {
    pub operation: BlacklistOp,
    pub paths: Vec<String>,
}

/// Blacklist operation type.
#[derive(Debug, Clone)]
pub enum BlacklistOp {
    List,
    Add(String),
    Remove(String),
}

/// Format blacklist result for display.
pub fn format_blacklist_result(result: &BlacklistResult) -> String {
    match &result.operation {
        BlacklistOp::List => {
            if result.paths.is_empty() {
                "No blacklist entries.".into()
            } else {
                let mut output = "Blacklisted paths:\n".to_string();
                for path in &result.paths {
                    output.push_str(&format!("  {path}\n"));
                }
                output
            }
        }
        BlacklistOp::Add(path) => format!("Added to blacklist: {path}"),
        BlacklistOp::Remove(path) => format!("Removed from blacklist: {path}"),
    }
}

/// Default blacklist patterns (from invariant D1).
pub fn default_blacklist() -> Vec<String> {
    vec![
        "/tmp/**".into(),
        "/var/log/**".into(),
        "/proc/**".into(),
        "/sys/**".into(),
        "/dev/**".into(),
        "/run/user/**".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_list_empty() {
        let result = BlacklistResult { operation: BlacklistOp::List, paths: vec![] };
        assert!(format_blacklist_result(&result).contains("No blacklist"));
    }

    #[test]
    fn format_list_with_entries() {
        let result = BlacklistResult { operation: BlacklistOp::List, paths: default_blacklist() };
        let output = format_blacklist_result(&result);
        assert!(output.contains("/tmp/**"));
        assert!(output.contains("/proc/**"));
    }

    #[test]
    fn format_add() {
        let result = BlacklistResult {
            operation: BlacklistOp::Add("/custom/path/**".into()),
            paths: vec![],
        };
        assert!(format_blacklist_result(&result).contains("Added"));
    }

    #[test]
    fn format_remove() {
        let result =
            BlacklistResult { operation: BlacklistOp::Remove("/tmp/**".into()), paths: vec![] };
        assert!(format_blacklist_result(&result).contains("Removed"));
    }

    #[test]
    fn default_blacklist_has_six_entries() {
        assert_eq!(default_blacklist().len(), 6);
    }
}
