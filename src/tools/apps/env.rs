//! `%VAR%` expansion for paths and arguments in app settings.

/// Replaces each `%NAME%` with the environment variable's value. Unknown names
/// are left as written so the mistake shows up in logs.
pub fn expand(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(value) => out.push_str(&value),
                    Err(_) => out.push_str(&rest[start..start + end + 2]),
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::expand;

    #[test]
    fn expands_known_and_keeps_unknown() {
        std::env::set_var("SPT_TEST_DIR", "C:\\x");
        assert_eq!(expand("%SPT_TEST_DIR%\\a"), "C:\\x\\a");
        assert_eq!(expand("%SPT_NOPE%\\a"), "%SPT_NOPE%\\a");
        assert_eq!(expand("100% sure"), "100% sure");
    }
}
