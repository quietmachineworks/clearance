//! Checks on the page (ADR-0011): colours live in the skin blocks only, every skin keeps text readable,
//! both languages say everything.

use std::collections::HashMap;

const PAGE: &str = include_str!("../public/index.html");
const END_OF_SKINS: &str = "/* ---- end of skins ---- */";

fn style() -> &'static str {
    let start = PAGE.find("<style>").unwrap() + 7;
    &PAGE[start..PAGE.find("</style>").unwrap()]
}

/// Each skin's variables, the defaults of `:root` filled in.
fn skins() -> Vec<(String, HashMap<String, String>)> {
    let css = &style()[..style().find(END_OF_SKINS).unwrap()];
    let mut out: Vec<(String, HashMap<String, String>)> = Vec::new();
    for block in css.split('}') {
        let Some((head, body)) = block.split_once('{') else { continue };
        let Some(name) = head.rsplit("[data-skin=").next().filter(|_| head.contains("[data-skin=")).map(|n| n.trim().trim_end_matches(']').to_string()) else { continue };
        let mut vars = out.first().map(|(_, v)| v.clone()).unwrap_or_default();
        for decl in body.split(';') {
            if let Some((k, v)) = decl.trim().split_once(':').filter(|(k, _)| k.trim().starts_with("--")) {
                vars.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        out.push((name, vars));
    }
    out
}

fn luminance(hex: &str) -> Option<f64> {
    let h = hex.strip_prefix('#').filter(|h| h.len() == 6)?;
    let c = |i: usize| -> Option<f64> {
        let v = u8::from_str_radix(&h[i..i + 2], 16).ok()? as f64 / 255.0;
        Some(if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) })
    };
    Some(0.2126 * c(0)? + 0.7152 * c(2)? + 0.0722 * c(4)?)
}

fn contrast(a: &str, b: &str) -> Option<f64> {
    let (x, y) = (luminance(a)?, luminance(b)?);
    Some((x.max(y) + 0.05) / (x.min(y) + 0.05))
}

#[test]
fn colours_are_written_in_the_skins_only() {
    let rest = &style()[style().find(END_OF_SKINS).unwrap()..];
    for bad in ["#", "rgb(", "rgba(", "hsl("] {
        assert!(!rest.contains(bad), "a colour written in place: {}", &rest[rest.find(bad).unwrap()..][..60.min(rest.len() - rest.find(bad).unwrap())]);
    }
}

#[test]
fn every_skin_keeps_text_readable() {
    // Text colour, background: the pairs the page actually draws text with.
    let pairs = [
        ("--ink", "--paper"),
        ("--ink", "--page"),
        ("--ink", "--sel"),
        ("--ink", "--soft"),
        ("--ink", "--verified"),
        ("--ink", "--badBg"),
        ("--ink", "--ctxBg"),
        ("--text2", "--paper"),
        ("--text2", "--sel"),
        ("--muted", "--paper"),
        ("--muted", "--page"),
        ("--muted", "--panel"),
        ("--muted", "--soft"),
        ("--muted", "--settled"),
        ("--danger", "--paper"),
        ("--danger", "--badBg"),
        ("--green", "--paper"),
        ("--okInk", "--okBg"),
        ("--ctxInk", "--paper"),
        ("--ctxInk", "--ctxBg"),
        ("--screenInk", "--screen"),
        ("--screenInk", "--screenHead"),
        ("--onAccent", "--accent"),
        ("--onRed", "--red"),
        ("--onBlue", "--blue"),
        ("--onOrange", "--orange"),
        ("--onPrimary", "--primary"),
        ("--onStop", "--stop"),
        ("--tape-ink", "--tape-bg"),
    ];
    let all = skins();
    assert_eq!(all.len(), 12, "{:?}", all.iter().map(|s| &s.0).collect::<Vec<_>>());
    let mut failures = Vec::new();
    for (skin, vars) in &all {
        for (fg, bg) in pairs {
            let mut back = vars[bg].as_str();
            if back == "transparent" {
                back = &vars["--page"];
            }
            let ratio = contrast(&vars[fg], back).unwrap_or_else(|| panic!("{skin}: {fg} {} or {bg} {back} is not #rrggbb", vars[fg]));
            if ratio < 4.5 {
                failures.push(format!("{skin}: {fg} on {bg} = {ratio:.2}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The keys of one language of the dictionary.
fn keys(lang: &str) -> Vec<String> {
    let start = PAGE.find(&format!("\n  {lang}: {{")).unwrap();
    let block = &PAGE[start..][..PAGE[start..].find("\n  },").unwrap()];
    let mut out = Vec::new();
    for (i, _) in block.match_indices(": ") {
        if !block[i + 2..].starts_with(['\'', '"']) {
            continue;
        }
        let before = &block[..i];
        let raw = &before[before.rfind([' ', ',', '{', '\n']).unwrap() + 1..];
        // 'Fix: ' and 'Corrige : ' are values that end in a colon: a key is quoted on both sides or not at all.
        if !raw.is_empty() && raw.starts_with('\'') == raw.ends_with('\'') {
            out.push(raw.trim_matches('\'').to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn both_languages_say_everything() {
    let (en, fr) = (keys("en"), keys("fr"));
    assert!(en.len() > 150, "{}", en.len());
    let missing = |a: &[String], b: &[String]| a.iter().filter(|k| !b.contains(k)).cloned().collect::<Vec<_>>();
    assert_eq!(missing(&en, &fr), Vec::<String>::new(), "missing in French");
    assert_eq!(missing(&fr, &en), Vec::<String>::new(), "missing in English");
}

#[test]
fn the_contrast_check_measures_what_wcag_does() {
    assert!((contrast("#000000", "#FFFFFF").unwrap() - 21.0).abs() < 1e-9);
    assert!((contrast("#777777", "#FFFFFF").unwrap() - 4.48).abs() < 0.01);
}
