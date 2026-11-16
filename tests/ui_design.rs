//! Design guardrails for ui/index.html. These keep the UI on its design system
//! (DESIGN.md) as agents and people change it. If one fails, fix the UI to fit
//! the system; change a rule here only together with DESIGN.md.

const UI: &str = include_str!("../ui/index.html");

fn style() -> &'static str {
    let start = UI.find("<style>").expect("ui has a <style> block") + "<style>".len();
    let end = UI.find("</style>").expect("ui closes its <style> block");
    &UI[start..end]
}

/// The token blocks open the stylesheet and end at the TOKENS:END marker.
fn token_block() -> &'static str {
    let s = style();
    assert!(
        s.contains("TOKENS:BEGIN"),
        "keep the TOKENS:BEGIN marker in :root"
    );
    &s[..s.find("TOKENS:END").expect("TOKENS:END marker")]
}

fn outside_tokens() -> &'static str {
    let s = style();
    &s[s.find("TOKENS:END").unwrap()..]
}

/// `--name: value;` declarations inside one `{ ... }` block that starts at `selector`.
fn tokens_in(selector: &str) -> Vec<String> {
    let block = token_block();
    let at = block
        .find(selector)
        .unwrap_or_else(|| panic!("token block has {selector}"));
    let open = at + block[at..].find('{').unwrap();
    let close = open + block[open..].find('}').unwrap();
    let mut names: Vec<String> = strip_comments(&block[open..close])
        .split(';')
        .filter_map(|d| d.trim().split(':').next())
        .map(|n| n.trim().to_string())
        .filter(|n| n.starts_with("--"))
        .collect();
    names.sort();
    names
}

fn strip_comments(css: &str) -> String {
    let mut out = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = rest[i..].find("*/").map_or("", |j| &rest[i + j + 2..]);
    }
    out.push_str(rest);
    out
}

fn has_colour_literal(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    for fun in ["rgb(", "rgba(", "hsl(", "hsla(", "oklch("] {
        if let Some(i) = lower.find(fun) {
            return Some(lower[i..].chars().take(30).collect());
        }
    }
    let bytes = lower.as_bytes();
    for (i, _) in lower.match_indices('#') {
        let hex = bytes[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_hexdigit())
            .count();
        let next = bytes.get(i + 1 + hex).copied().unwrap_or(b' ');
        if matches!(hex, 3 | 4 | 6 | 8) && !next.is_ascii_alphanumeric() && next != b'-' {
            return Some(lower[i..i + 1 + hex].to_string());
        }
    }
    for named in ["white", "black"] {
        for (i, _) in lower.match_indices(named) {
            let before = if i == 0 { b' ' } else { bytes[i - 1] };
            let after = bytes.get(i + named.len()).copied().unwrap_or(b' ');
            if matches!(before, b' ' | b':' | b'(' | b',')
                && !after.is_ascii_alphanumeric()
                && after != b'-'
            {
                return Some(named.to_string());
            }
        }
    }
    None
}

#[test]
fn colours_are_only_written_as_tokens() {
    if let Some(found) = has_colour_literal(&strip_comments(outside_tokens())) {
        panic!(
            "colour literal {found:?} outside the TOKENS block; add or reuse a token in DESIGN.md"
        );
    }
    for attr in UI.split("style=\"").skip(1) {
        let value = &attr[..attr.find('"').unwrap()];
        if let Some(found) = has_colour_literal(value) {
            panic!("colour literal {found:?} in a style attribute; use a class and a token");
        }
    }
}

#[test]
fn both_dark_blocks_redefine_the_same_tokens() {
    let light = tokens_in(":root {");
    let dark_media = tokens_in(":root:not([data-theme=\"light\"])");
    let dark_attr = tokens_in(":root[data-theme=\"dark\"]");
    assert_eq!(
        dark_media, dark_attr,
        "the two dark blocks must stay identical"
    );
    for t in &dark_media {
        assert!(
            light.contains(t),
            "{t} is set for dark but never defined in the light :root"
        );
    }
    let colour_tokens: Vec<_> = light
        .iter()
        .filter(|t| {
            !t.starts_with("--font")
                && !t.starts_with("--step")
                && !t.starts_with("--s")
                && !t.starts_with("--r-")
                && *t != "--ease"
        })
        .collect();
    for t in colour_tokens {
        assert!(dark_media.contains(t), "colour token {t} has no dark value");
    }
}

#[test]
fn type_sizes_and_families_come_from_tokens() {
    let css = outside_tokens();
    for decl in css.split(';') {
        let decl = decl.trim();
        for prop in ["font-size:", "font-family:", "font:"] {
            if let Some(rest) = decl.split_once(prop).map(|(_, v)| v) {
                let ok = rest.contains("var(--") || rest.trim() == "inherit";
                assert!(ok, "`{decl}` should use a --step-* / --font-* token");
            }
        }
    }
}

#[test]
fn ui_works_offline_and_inside_the_webview() {
    // Links out (GitHub) are fine; anything the page *loads* must be inline.
    for bad in [
        "src=\"http",
        "<link rel=\"stylesheet\" href=\"http",
        "@import",
        "fonts.googleapis",
        "url(http",
    ] {
        assert!(
            !UI.contains(bad),
            "ui loads {bad}...: the app must work offline, inline it"
        );
    }
    for link in UI.split("href=\"http").skip(1) {
        assert!(
            link.starts_with("s://github.com/"),
            "external link to {}: only GitHub links (opened in the browser) are expected",
            &link[..link.find('"').unwrap_or(30).min(60)]
        );
    }
    for bad in ["alert(", "confirm(", "prompt("] {
        assert!(
            !UI.contains(bad),
            "ui uses {bad}): build the confirmation into the page"
        );
    }
    assert!(
        UI.contains("prefers-reduced-motion"),
        "keep the reduced-motion rule"
    );
    assert!(
        UI.contains(":focus-visible"),
        "keep a visible keyboard focus style"
    );
}

#[test]
fn demo_mode_covers_every_health_state_the_server_reports() {
    // DESIGN.md asks for a visual check with /?demo; it must show the states that matter.
    for state in [
        "healthy", "failing", "starting", "held", "gave_up", "stopped",
    ] {
        assert!(
            UI.contains(&format!("health: '{state}'")),
            "demo data lacks a {state} app"
        );
    }
    let server = include_str!("../src/tools/apps/supervisor.rs");
    for variant in [
        "Unknown", "Healthy", "Failing", "Starting", "Held", "Stopped", "GaveUp",
    ] {
        assert!(
            server.contains(variant),
            "Health::{variant} renamed? update STATES in ui/index.html"
        );
    }
    for key in [
        "healthy", "starting", "acting", "failing", "gave_up", "held", "stopped", "unknown",
    ] {
        assert!(
            UI.contains(&format!("{key}:")),
            "STATES in ui/index.html lacks {key}"
        );
        assert!(
            UI.contains(&format!(".st-{key}")),
            "no .st-{key} colour class in ui/index.html"
        );
    }
}

#[test]
fn every_page_and_tool_api_has_demo_data() {
    // New server endpoints the page calls need a demo answer, so /?demo keeps working.
    for path in [
        "/api/info",
        "/api/apps",
        "/api/disk/drives",
        "/api/disk/status",
        "/api/disk/list",
    ] {
        assert!(
            UI.contains(&format!("'{path}'")),
            "the page or demoApi doesn't know {path}"
        );
    }
    let demo = &UI[UI
        .find("async function demoApi")
        .expect("demoApi in ui/index.html")..];
    for path in [
        "/api/info",
        "/api/apps",
        "/api/disk/drives",
        "/api/disk/status",
        "/api/disk/list",
    ] {
        assert!(
            demo.contains(&format!("'{path}'")),
            "demoApi has no answer for {path}"
        );
    }
}
