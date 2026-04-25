use crate::executor::ExecutorError;

/// Validate a username: `[a-zA-Z0-9._-]`, 1-32 chars, must not start with `-`.
pub fn validated_username(s: &str, param: &'static str) -> Result<String, ExecutorError> {
    if s.is_empty() || s.len() > 32 {
        return Err(ExecutorError::InvalidParam(param));
    }
    if s.starts_with('-') {
        return Err(ExecutorError::InvalidParam(param));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(ExecutorError::InvalidParam(param));
    }
    Ok(s.to_string())
}

/// Validate a group name: same rules as username.
pub fn validated_group(s: &str, param: &'static str) -> Result<String, ExecutorError> {
    validated_username(s, param)
}

/// Validate a systemd unit name: must match `[a-zA-Z0-9@._:-]+` (no slashes, no spaces).
pub fn validated_unit_name(s: &str, param: &'static str) -> Result<String, ExecutorError> {
    if s.is_empty() {
        return Err(ExecutorError::InvalidParam(param));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | ':' | '-'))
    {
        return Err(ExecutorError::InvalidParam(param));
    }
    Ok(s.to_string())
}

/// Validate a hostname per RFC 1123: `[a-zA-Z0-9.-]`, 1-253 chars, labels 1-63 chars.
pub fn validated_hostname(s: &str, param: &'static str) -> Result<String, ExecutorError> {
    if s.is_empty() || s.len() > 253 {
        return Err(ExecutorError::InvalidParam(param));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(ExecutorError::InvalidParam(param));
    }
    // Each label between dots must be 1-63 chars.
    for label in s.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(ExecutorError::InvalidParam(param));
        }
    }
    Ok(s.to_string())
}

/// Validate a timezone: `[a-zA-Z0-9/_+-]`, no `..`.
pub fn validated_timezone(s: &str, param: &'static str) -> Result<String, ExecutorError> {
    if s.is_empty() {
        return Err(ExecutorError::InvalidParam(param));
    }
    if s.contains("..") {
        return Err(ExecutorError::InvalidParam(param));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '+' | '-'))
    {
        return Err(ExecutorError::InvalidParam(param));
    }
    Ok(s.to_string())
}

/// Validate a locale: `[a-zA-Z0-9._-]`.
pub fn validated_locale(s: &str, param: &'static str) -> Result<String, ExecutorError> {
    if s.is_empty() {
        return Err(ExecutorError::InvalidParam(param));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(ExecutorError::InvalidParam(param));
    }
    Ok(s.to_string())
}

/// General safe-arg validator with strict allowlist `[A-Za-z0-9._:/+@-]`,
/// 1-254 bytes, must not start with `-`.
///
/// This is the last line of defence against shell injection when arguments are
/// interpolated into command strings (e.g. `runuser -l user -c "<cmd>"`). The
/// allowlist deliberately excludes every shell metacharacter — quotes,
/// backticks, `$`, `;`, `&`, `|`, `>`, `<`, `\`, whitespace, control bytes,
/// and all non-ASCII. Callers that need richer character sets must use a
/// dedicated validator (e.g. `validated_hostname`, `validated_unit_name`).
pub fn validated_safe_arg(s: &str, param: &'static str) -> Result<String, ExecutorError> {
    if s.is_empty() || s.len() > 254 {
        return Err(ExecutorError::InvalidParam(param));
    }
    if s.starts_with('-') {
        return Err(ExecutorError::InvalidParam(param));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '+' | '@' | '-'))
    {
        return Err(ExecutorError::InvalidParam(param));
    }
    Ok(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validated_username / validated_group ──────────────────────────────

    #[test]
    fn username_accepts_valid() {
        assert_eq!(
            validated_username("alice", "username").unwrap(),
            "alice".to_string()
        );
        assert_eq!(
            validated_username("bob_99", "username").unwrap(),
            "bob_99".to_string()
        );
        assert_eq!(
            validated_username("user.name", "username").unwrap(),
            "user.name".to_string()
        );
        assert_eq!(
            validated_username("a-b", "username").unwrap(),
            "a-b".to_string()
        );
    }

    #[test]
    fn username_rejects_empty() {
        assert!(validated_username("", "username").is_err());
    }

    #[test]
    fn username_rejects_starts_with_dash() {
        assert!(validated_username("-alice", "username").is_err());
    }

    #[test]
    fn username_rejects_too_long() {
        let long = "a".repeat(33);
        assert!(validated_username(&long, "username").is_err());
    }

    #[test]
    fn username_accepts_max_length() {
        let max = "a".repeat(32);
        assert!(validated_username(&max, "username").is_ok());
    }

    #[test]
    fn username_rejects_spaces() {
        assert!(validated_username("al ice", "username").is_err());
    }

    #[test]
    fn username_rejects_slashes() {
        assert!(validated_username("al/ice", "username").is_err());
    }

    #[test]
    fn username_rejects_null_bytes() {
        assert!(validated_username("al\0ice", "username").is_err());
    }

    #[test]
    fn group_delegates_to_username_rules() {
        assert!(validated_group("wheel", "group").is_ok());
        assert!(validated_group("-bad", "group").is_err());
        assert!(validated_group("", "group").is_err());
    }

    // ── validated_unit_name ──────────────────────────────────────────────

    #[test]
    fn unit_name_accepts_valid() {
        assert!(validated_unit_name("sshd.service", "unit").is_ok());
        assert!(validated_unit_name("NetworkManager.service", "unit").is_ok());
        assert!(validated_unit_name("user@1000.service", "unit").is_ok());
        assert!(validated_unit_name("dbus-org.freedesktop.resolve1.service", "unit").is_ok());
        assert!(validated_unit_name("system-getty.slice:0", "unit").is_ok());
    }

    #[test]
    fn unit_name_rejects_empty() {
        assert!(validated_unit_name("", "unit").is_err());
    }

    #[test]
    fn unit_name_rejects_slashes() {
        assert!(validated_unit_name("foo/bar.service", "unit").is_err());
    }

    #[test]
    fn unit_name_rejects_spaces() {
        assert!(validated_unit_name("foo bar.service", "unit").is_err());
    }

    #[test]
    fn unit_name_rejects_null_bytes() {
        assert!(validated_unit_name("foo\0.service", "unit").is_err());
    }

    // ── validated_hostname ───────────────────────────────────────────────

    #[test]
    fn hostname_accepts_valid() {
        assert!(validated_hostname("sysknife-lab", "hostname").is_ok());
        assert!(validated_hostname("my.host.example", "hostname").is_ok());
        assert!(validated_hostname("a", "hostname").is_ok());
    }

    #[test]
    fn hostname_rejects_empty() {
        assert!(validated_hostname("", "hostname").is_err());
    }

    #[test]
    fn hostname_rejects_too_long() {
        let long = format!(
            "{}.{}",
            "a".repeat(63),
            "b".repeat(253 - 63 - 1 + 1) // total > 253
        );
        assert!(validated_hostname(&long, "hostname").is_err());
    }

    #[test]
    fn hostname_accepts_max_length() {
        // 4 labels of 63 chars separated by dots = 63*4+3 = 255, too long.
        // 3 labels of 63 chars separated by dots = 63*3+2 = 191, fine.
        let hostname = format!("{}.{}.{}", "a".repeat(63), "b".repeat(63), "c".repeat(63));
        assert!(validated_hostname(&hostname, "hostname").is_ok());
    }

    #[test]
    fn hostname_rejects_label_too_long() {
        let long_label = "a".repeat(64);
        assert!(validated_hostname(&long_label, "hostname").is_err());
    }

    #[test]
    fn hostname_rejects_empty_label() {
        assert!(validated_hostname("foo..bar", "hostname").is_err());
        assert!(validated_hostname(".foo", "hostname").is_err());
        assert!(validated_hostname("foo.", "hostname").is_err());
    }

    #[test]
    fn hostname_rejects_spaces() {
        assert!(validated_hostname("my host", "hostname").is_err());
    }

    #[test]
    fn hostname_rejects_underscores() {
        assert!(validated_hostname("my_host", "hostname").is_err());
    }

    // ── validated_timezone ───────────────────────────────────────────────

    #[test]
    fn timezone_accepts_valid() {
        assert!(validated_timezone("America/Mexico_City", "timezone").is_ok());
        assert!(validated_timezone("UTC", "timezone").is_ok());
        assert!(validated_timezone("Etc/GMT+5", "timezone").is_ok());
        assert!(validated_timezone("US/Eastern", "timezone").is_ok());
    }

    #[test]
    fn timezone_rejects_empty() {
        assert!(validated_timezone("", "timezone").is_err());
    }

    #[test]
    fn timezone_rejects_dot_dot() {
        assert!(validated_timezone("America/../etc/passwd", "timezone").is_err());
        assert!(validated_timezone("..", "timezone").is_err());
    }

    #[test]
    fn timezone_rejects_spaces() {
        assert!(validated_timezone("US/ Eastern", "timezone").is_err());
    }

    #[test]
    fn timezone_rejects_null_bytes() {
        assert!(validated_timezone("UTC\0", "timezone").is_err());
    }

    // ── validated_locale ─────────────────────────────────────────────────

    #[test]
    fn locale_accepts_valid() {
        assert!(validated_locale("en_US.UTF-8", "locale").is_ok());
        assert!(validated_locale("C", "locale").is_ok());
        assert!(validated_locale("POSIX", "locale").is_ok());
    }

    #[test]
    fn locale_rejects_empty() {
        assert!(validated_locale("", "locale").is_err());
    }

    #[test]
    fn locale_rejects_spaces() {
        assert!(validated_locale("en US.UTF-8", "locale").is_err());
    }

    #[test]
    fn locale_rejects_slashes() {
        assert!(validated_locale("en/US", "locale").is_err());
    }

    #[test]
    fn locale_rejects_null_bytes() {
        assert!(validated_locale("en\0US", "locale").is_err());
    }

    // ── validated_safe_arg ───────────────────────────────────────────────

    #[test]
    fn safe_arg_accepts_valid() {
        assert!(validated_safe_arg("org.mozilla.firefox", "app_id").is_ok());
        assert!(validated_safe_arg("flathub", "remote").is_ok());
        assert!(validated_safe_arg("my-container", "name").is_ok());
        assert!(validated_safe_arg("registry.example.com/image:tag", "image").is_ok());
    }

    #[test]
    fn safe_arg_rejects_empty() {
        assert!(validated_safe_arg("", "name").is_err());
    }

    #[test]
    fn safe_arg_rejects_null_bytes() {
        assert!(validated_safe_arg("hello\0world", "name").is_err());
    }

    #[test]
    fn safe_arg_rejects_starts_with_dash() {
        assert!(validated_safe_arg("-evil", "name").is_err());
        assert!(validated_safe_arg("--rm", "name").is_err());
    }

    #[test]
    fn safe_arg_accepts_dash_not_at_start() {
        assert!(validated_safe_arg("my-container", "name").is_ok());
    }

    #[test]
    fn safe_arg_rejects_unicode_and_non_ascii() {
        // Strict ASCII allowlist — non-ASCII (including printable Unicode) is rejected
        // because it can include homoglyphs / control codepoints that survive shell
        // interpolation in surprising ways.
        assert!(validated_safe_arg("café", "name").is_err());
        assert!(validated_safe_arg("über", "name").is_err());
    }

    #[test]
    fn safe_arg_rejects_every_shell_metacharacter() {
        // CVE-class regression: every one of these has been used to inject a
        // command into a `sh -c "<arg>"` style call somewhere in the wild.
        for meta in [
            "a b",   // space
            "a\tb",  // tab
            "a\nb",  // newline
            "a\rb",  // CR
            "a\0b",  // NUL
            "a;b",   // command separator
            "a&b",   // background / AND
            "a|b",   // pipe
            "a$b",   // var expansion
            "a`b`",  // command substitution
            "a$(b)", // command substitution
            "a>b",   // redirect
            "a<b",   // redirect
            "a\\b",  // backslash
            "a\"b",  // double quote
            "a'b",   // single quote
            "a*b",   // glob
            "a?b",   // glob
            "a[b]",  // glob
            "a{b}",  // brace expansion
            "a~b",   // tilde
            "a!b",   // history
            "a#b",   // comment
            "a%b",   // job control / printf
            "a^b",   // history quick-substitution (csh)
            "a=b",   // assignment in some contexts
            "a,b",   // brace expansion list
            "a(b)",  // subshell
        ] {
            assert!(
                validated_safe_arg(meta, "arg").is_err(),
                "should reject metacharacter sequence {meta:?}"
            );
        }
    }

    #[test]
    fn safe_arg_rejects_oversized_input() {
        let big = "a".repeat(255);
        assert!(validated_safe_arg(&big, "name").is_err());
        let max = "a".repeat(254);
        assert!(validated_safe_arg(&max, "name").is_ok());
    }

    // ── error variant check ──────────────────────────────────────────────

    #[test]
    fn validators_return_invalid_param_with_correct_field_name() {
        let err = validated_username("", "username").unwrap_err();
        assert!(matches!(err, ExecutorError::InvalidParam("username")));

        let err = validated_group("-bad", "group").unwrap_err();
        assert!(matches!(err, ExecutorError::InvalidParam("group")));

        let err = validated_unit_name("foo/bar", "unit").unwrap_err();
        assert!(matches!(err, ExecutorError::InvalidParam("unit")));

        let err = validated_hostname("", "hostname").unwrap_err();
        assert!(matches!(err, ExecutorError::InvalidParam("hostname")));

        let err = validated_timezone("..", "timezone").unwrap_err();
        assert!(matches!(err, ExecutorError::InvalidParam("timezone")));

        let err = validated_locale("", "locale").unwrap_err();
        assert!(matches!(err, ExecutorError::InvalidParam("locale")));

        let err = validated_safe_arg("-x", "name").unwrap_err();
        assert!(matches!(err, ExecutorError::InvalidParam("name")));
    }
}
