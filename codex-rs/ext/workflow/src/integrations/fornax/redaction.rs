/// Replaces configured secret values before external diagnostics cross the adapter boundary.
#[derive(Clone, Debug)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    /// Creates a redactor. Empty values are ignored and longer values win first.
    pub fn new(mut secrets: Vec<String>) -> Self {
        secrets.retain(|secret| !secret.is_empty());
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        secrets.dedup();
        Self { secrets }
    }

    /// Redacts exact configured values and common credential assignments.
    pub fn redact(&self, value: &str) -> String {
        let mut redacted = value.to_string();
        for secret in &self.secrets {
            redacted = redacted.replace(secret, "[REDACTED]");
        }
        for marker in [
            "FORNAX_AK",
            "FORNAX_SK",
            "FORNAX_BYTED_JWT_TOKEN",
            "authorization",
            "token",
            "secret",
        ] {
            redact_assignment(&mut redacted, marker);
        }
        redacted
    }
}

fn redact_assignment(value: &mut String, marker: &str) {
    let marker = marker.to_ascii_lowercase();
    let mut search_from = 0;
    loop {
        let lower = value.to_ascii_lowercase();
        let Some(relative) = lower[search_from..].find(&marker) else {
            break;
        };
        let marker_start = search_from + relative;
        let suffix = &value[marker_start + marker.len()..];
        let Some(separator) = suffix.find(['=', ':']) else {
            search_from = marker_start + marker.len();
            continue;
        };
        let secret_start = marker_start + marker.len() + separator + 1;
        let secret_end = value[secret_start..]
            .find(|character: char| {
                character.is_whitespace() || matches!(character, ',' | '}' | '"' | '\'')
            })
            .map_or(value.len(), |offset| secret_start + offset);
        if secret_end > secret_start {
            value.replace_range(secret_start..secret_end, "[REDACTED]");
        }
        search_from = secret_start + "[REDACTED]".len();
    }
}
