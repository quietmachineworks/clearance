//! Checks on the page (ADR-0011): colours live in the skin blocks only, every skin keeps text readable,
//! both languages say everything.

use std::collections::HashMap;

const PAGE: &str = include_str!("../public/index.html");
const END_OF_SKINS: &str = "/* ---- end of skins ---- */";

fn style() -> &'static str {
    let open = PAGE.find("<style").unwrap();
    let start = open + PAGE[open..].find('>').unwrap() + 1;
    &PAGE[start..PAGE.find("</style>").unwrap()]
}

#[test]
fn the_page_runs_only_its_own_script_and_styles() {
    // One script and one style block, both carrying the nonce the server replaces (ADR-0014).
    assert_eq!(PAGE.matches("<script").count(), 1);
    assert_eq!(PAGE.matches("<style").count(), 1);
    assert!(PAGE.contains("<script nonce=\"__NONCE__\">") && PAGE.contains("<style nonce=\"__NONCE__\">"));
    // No style attribute (the policy refuses it) and nothing loaded from another site.
    assert!(!PAGE.contains(" style=\""), "a style attribute: use a class, or h()'s style through the CSSOM");
    assert!(!PAGE.contains("setAttribute('style'"));
    for far in ["https://fonts.", "src=\"http", "href=\"http", "url(http"] {
        assert!(!PAGE.contains(far), "loaded from another site: {far}");
    }
    let fonts: Vec<&str> = PAGE.match_indices("url(/fonts/").map(|(i, _)| &PAGE[i + 11..i + PAGE[i..].find(')').unwrap()]).collect();
    assert_eq!(fonts.len(), 13);
    for f in fonts {
        assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("public/fonts").join(f).is_file(), "missing font {f}");
    }
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
fn xterm_is_the_frozen_copy_its_readme_lists() {
    // ADR-0016: copied from the npm registry, never edited.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("public/vendor/xterm");
    let readme = std::fs::read_to_string(dir.join("README.md")).unwrap();
    let rows: Vec<(&str, &str)> = readme
        .lines()
        .filter_map(|l| {
            let cells: Vec<&str> = l.split('|').map(|c| c.trim().trim_matches('`')).collect();
            (cells.len() > 4 && cells[3].len() == 64).then(|| (cells[1], cells[3]))
        })
        .collect();
    assert_eq!(rows.len(), 5, "{rows:?}");
    for (file, hash) in rows {
        assert_eq!(crate::sys::sha256(&std::fs::read(dir.join(file)).unwrap()), hash, "{file} differs from the copy listed");
    }
}

#[test]
fn the_contrast_check_measures_what_wcag_does() {
    assert!((contrast("#000000", "#FFFFFF").unwrap() - 21.0).abs() < 1e-9);
    assert!((contrast("#777777", "#FFFFFF").unwrap() - 4.48).abs() < 0.01);
}

/// The page's pure helpers (search, the thread in turns), run under node when it is installed: the
/// repository needs no JS tool, so without node this check says it was skipped.
#[test]
fn search_and_the_thread_in_turns_behave() {
    let (start, end) = (PAGE.find("// ---- pure:").unwrap(), PAGE.find("// ---- end pure ----").unwrap());
    let checks = r#"
const assert = (ok, what) => { if (!ok) { console.error('FAILED: ' + what); process.exit(1) } }
assert(matches('eloi cafe', 'Café chez Éloi'), 'accents and case ignored');
assert(matches('chez cafe', 'Café chez Éloi'), 'any order');
assert(!matches('cafe the', 'Café chez Éloi'), 'every word');
assert(matches('  ', 'anything'), 'an empty query finds all');
const tool = (name, target = '', error = false) => ({ kind: 'tool', tool: name, target, error, at: name });
const said = (text) => ({ kind: 'text', text, at: text });
const turns = turnsOf([{ kind: 'resumed', at: '0' }, { kind: 'user', text: 'Ship it', at: '1' }, tool('Read', 'a.rs'), tool('Read', 'b.rs'), said('Looking.'),
  tool('Bash', 'cargo test', true), tool('Edit', 'a.rs'), tool('Bash', 'cargo test'), tool('Bash', 'git commit -m x'), said('Shipped.'), { kind: 'user', text: 'More?', at: '2' }, tool('Grep', 'x'), said('Wait'), tool('Read', 'c.rs')]);
assert(turns.length === 3 && turns[0].resumed && !turns[0].ask, 'a resume opens a turn of its own');
assert(turns[1].reply?.text === 'Shipped.' && turns[1].steps.length === 7, 'the last text is the reply, the rest are steps');
assert(turns[2].reply === null, 'a text followed by more work is not a reply yet');
const b = bilanOf(turns[1].steps);
assert(b.n === 6 && b.files === 1 && b.tests === true && b.commits === 1 && b.failed === 1 && !b.mr, 'a turn sums up: ' + JSON.stringify(b));
const acts = actsOf(turns[1].steps);
assert(acts[0].verb === 'read' && acts[0].more === 1 && acts[1].verb === 'say' && acts[2].verb === 'run' && acts[2].error, 'reads in a row become one: ' + JSON.stringify(acts.map((a) => a.verb)));
assert(verbOf({ tool: 'mcp__tracker__create_issue' }) === 'tool' && verbOf({ tool: 'Artifact', artifact: 'u' }) === 'artifact', 'verbs');
console.log('pure helpers ok');
"#;
    let script = format!("{}\n{checks}", &PAGE[start..end]);
    match std::process::Command::new("node").args(["-e", &script]).output() {
        Ok(out) => assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
        Err(_) => eprintln!("node not found: the page's pure helpers were not run"),
    }
}

/// The page's whole script compiles (not run), under node when it is installed: a syntax error there
/// leaves a blank page that no other check sees.
#[test]
fn the_page_script_compiles() {
    use std::io::Write;
    let open = r#"<script nonce="__NONCE__">"#;
    let start = PAGE.find(open).unwrap() + open.len();
    let script = &PAGE[start..start + PAGE[start..].find("</script>").unwrap()];
    let child = std::process::Command::new("node")
        .args(["-e", "new (require('vm').Script)(require('fs').readFileSync(0, 'utf8'), { filename: 'index.html' })"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let Ok(mut child) = child else { return eprintln!("node not found: the page's script was not compiled") };
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}
