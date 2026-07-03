//! Core logic for the Kconfig-style build configuration tool.
//!
//! This is a Rust reimplementation of the original `kconfig.sh` (POSIX
//! sh + awk). The contract is intentionally identical: Kconfig/defconfig
//! file formats, `.config`/`config.mk` output formats, `@if`/`@endif`
//! marker semantics in `.system` templates, and error message text (every
//! error starts with `kconfig: error: ` once printed by the CLI in
//! `main.rs`; the messages produced here omit that prefix).
//!
//! Kept dependency-free (std only) so it builds with stable Rust and no
//! network access, since `config/kconfig.mk` builds it lazily at make
//! parse time.

use std::collections::{HashMap, HashSet};

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn trim_ws_prefix(s: &str) -> &str {
    s.trim_start_matches([' ', '\t'])
}

fn strip_leading_ws(line: &str) -> Option<&str> {
    let stripped = trim_ws_prefix(line);
    if stripped.len() == line.len() {
        None
    } else {
        Some(stripped)
    }
}

/// Splits off a leading run of name characters; returns (name, rest).
fn split_name(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !is_name_char(c)).unwrap_or(s.len());
    s.split_at(end)
}

// ---------------------------------------------------------------------------
// Kconfig declarations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct OptionDecl {
    pub default: bool,
    pub depends: Option<String>,
}

#[derive(Debug, Default)]
pub struct Kconfig {
    pub order: Vec<String>,
    pub options: HashMap<String, OptionDecl>,
}

/// Matches `^config[ \t]+[A-Za-z0-9_]+[ \t]*$`.
fn match_config_decl(line: &str) -> Option<String> {
    let rest = line.strip_prefix("config")?;
    let rest = strip_leading_ws(rest)?;
    let trimmed = rest.trim_end_matches([' ', '\t']);
    if !trimmed.is_empty() && trimmed.chars().all(is_name_char) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Matches `^[ \t]+default[ \t]+` and returns the following whitespace token.
fn match_default_line(line: &str) -> Option<String> {
    let rest = strip_leading_ws(line)?;
    let rest = rest.strip_prefix("default")?;
    let rest = strip_leading_ws(rest)?;
    let token = rest.split_whitespace().next().unwrap_or("");
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Matches `^[ \t]+depends on[ \t]+` and returns the rest of the line.
fn match_depends_line(line: &str) -> Option<String> {
    let rest = strip_leading_ws(line)?;
    let rest = rest.strip_prefix("depends on")?;
    let rest = strip_leading_ws(rest)?;
    Some(rest.to_string())
}

pub fn parse_kconfig(text: &str) -> Result<Kconfig, String> {
    let mut kconfig = Kconfig::default();
    let mut current: Option<String> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');

        if let Some(name) = match_config_decl(line) {
            if kconfig.options.contains_key(&name) {
                return Err(format!("duplicate option {name} in Kconfig"));
            }
            kconfig.options.insert(
                name.clone(),
                OptionDecl {
                    default: false,
                    depends: None,
                },
            );
            kconfig.order.push(name.clone());
            current = Some(name);
            continue;
        }

        if let Some(value_tok) = match_default_line(line) {
            let cur = current
                .clone()
                .ok_or_else(|| "Kconfig: default outside config block".to_string())?;
            let value = match value_tok.as_str() {
                "y" => true,
                "n" => false,
                other => {
                    return Err(format!(
                        "Kconfig: option {cur} has non-bool default '{other}'"
                    ))
                }
            };
            kconfig.options.get_mut(&cur).unwrap().default = value;
            continue;
        }

        if let Some(expr) = match_depends_line(line) {
            let cur = current
                .clone()
                .ok_or_else(|| "Kconfig: depends outside config block".to_string())?;
            kconfig.options.get_mut(&cur).unwrap().depends = Some(expr);
            continue;
        }

        // bool/help/menu/comment lines carry no resolution semantics.
    }

    Ok(kconfig)
}

// ---------------------------------------------------------------------------
// Assignment layering (defconfig + command-line overrides)
// ---------------------------------------------------------------------------

fn assign(
    values: &mut HashMap<String, bool>,
    declared: &HashSet<String>,
    name: &str,
    raw_value: &str,
    src: &str,
) -> Result<(), String> {
    if !declared.contains(name) {
        return Err(format!(
            "{src}: unknown option CONFIG_{name} (not declared in Kconfig)"
        ));
    }
    let value = match raw_value {
        "y" => true,
        "n" => false,
        other => {
            return Err(format!(
                "{src}: CONFIG_{name} must be y or n, got '{other}'"
            ))
        }
    };
    values.insert(name.to_string(), value);
    Ok(())
}

fn is_blank_or_comment(line: &str) -> bool {
    let t = trim_ws_prefix(line);
    t.is_empty() || t.starts_with('#')
}

/// Matches `^#[ \t]*CONFIG_[A-Za-z0-9_]+[ \t]+is not set[ \t]*$` (strict,
/// used for defconfig parsing).
fn match_not_set_strict(line: &str) -> Option<String> {
    let rest = line.strip_prefix('#')?;
    let rest = trim_ws_prefix(rest);
    let rest = rest.strip_prefix("CONFIG_")?;
    let (name, remainder) = split_name(rest);
    if name.is_empty() {
        return None;
    }
    let remainder = strip_leading_ws(remainder)?;
    let remainder = remainder.strip_prefix("is not set")?;
    let remainder = remainder.trim_matches(|c| c == ' ' || c == '\t');
    if remainder.is_empty() {
        Some(name.to_string())
    } else {
        None
    }
}

/// Matches `^CONFIG_[A-Za-z0-9_]+=`, returning (name, raw value text).
fn match_config_assign(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("CONFIG_")?;
    let (name, remainder) = split_name(rest);
    if name.is_empty() {
        return None;
    }
    let value = remainder.strip_prefix('=')?;
    Some((name.to_string(), value.to_string()))
}

fn parse_defconfig(
    text: &str,
    declared: &HashSet<String>,
    src: &str,
    values: &mut HashMap<String, bool>,
) -> Result<(), String> {
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');

        if let Some(name) = match_not_set_strict(line) {
            assign(values, declared, &name, "n", src)?;
            continue;
        }
        if is_blank_or_comment(line) {
            continue;
        }
        if let Some((name, value)) = match_config_assign(line) {
            assign(values, declared, &name, &value, src)?;
            continue;
        }
        return Err(format!("{src}: unrecognized line: {line}"));
    }
    Ok(())
}

/// Matches `^CONFIG_[A-Za-z0-9_]+=(y|n)$` exactly, for `--set` overrides.
fn parse_override(spec: &str) -> Option<(String, String)> {
    let rest = spec.strip_prefix("CONFIG_")?;
    let (name, remainder) = split_name(rest);
    if name.is_empty() {
        return None;
    }
    let value = remainder.strip_prefix('=')?;
    if value == "y" || value == "n" {
        Some((name.to_string(), value.to_string()))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// resolve
// ---------------------------------------------------------------------------

/// Resolves option values from Kconfig defaults, an optional defconfig, and
/// `--set CONFIG_NAME=y|n` overrides. Returns resolved (name, value) pairs
/// in Kconfig declaration order.
pub fn resolve(
    kconfig_text: &str,
    defconfig: Option<(&str, &str)>,
    overrides: &[String],
) -> Result<Vec<(String, bool)>, String> {
    let kconfig = parse_kconfig(kconfig_text)?;
    let declared: HashSet<String> = kconfig.order.iter().cloned().collect();

    let mut values: HashMap<String, bool> = HashMap::new();
    for name in &kconfig.order {
        values.insert(name.clone(), kconfig.options[name].default);
    }

    if let Some((defconfig_name, defconfig_text)) = defconfig {
        parse_defconfig(defconfig_text, &declared, defconfig_name, &mut values)?;
    }

    for spec in overrides {
        if spec.is_empty() {
            continue;
        }
        let (name, value) = parse_override(spec)
            .ok_or_else(|| format!("override '{spec}' is not of the form CONFIG_NAME=y|n"))?;
        assign(&mut values, &declared, &name, &value, "command line")?;
    }

    for name in &kconfig.order {
        if !values[name] {
            continue;
        }
        let Some(expr) = &kconfig.options[name].depends else {
            continue;
        };
        for term in expr.split("&&") {
            let term = term.trim();
            let (neg, term_name) = match term.strip_prefix('!') {
                Some(rest) => (true, rest.trim()),
                None => (false, term),
            };
            if !declared.contains(term_name) {
                return Err(format!(
                    "Kconfig: CONFIG_{name} depends on undeclared option {term_name}"
                ));
            }
            let raw = values[term_name];
            let sat = if neg { !raw } else { raw };
            if !sat {
                let neg_str = if neg { "!" } else { "" };
                let raw_str = if raw { "y" } else { "n" };
                return Err(format!(
                    "CONFIG_{name}=y requires {neg_str}CONFIG_{term_name} (currently CONFIG_{term_name}={raw_str})"
                ));
            }
        }
    }

    Ok(kconfig
        .order
        .iter()
        .map(|n| (n.clone(), values[n]))
        .collect())
}

pub fn render_dot_config(resolved: &[(String, bool)], defconfig_name: &str) -> String {
    let mut out = format!(
        "# Automatically generated by kconfig-tool; do not edit.\n\
         # Layers: Kconfig defaults <- {defconfig_name} <- command line\n"
    );
    for (name, value) in resolved {
        if *value {
            out.push_str(&format!("CONFIG_{name}=y\n"));
        } else {
            out.push_str(&format!("# CONFIG_{name} is not set\n"));
        }
    }
    out
}

pub fn render_config_mk(resolved: &[(String, bool)]) -> String {
    let mut out = "# Automatically generated by kconfig-tool; do not edit.\n".to_string();
    for (name, value) in resolved {
        out.push_str(&format!(
            "CONFIG_{name} := {}\n",
            if *value { "y" } else { "n" }
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// gensystem
// ---------------------------------------------------------------------------

enum Marker {
    If { neg: bool, name: String },
    EndIf,
    None,
}

fn try_if_marker(after_comment_open: &str) -> Option<(bool, String)> {
    let s = trim_ws_prefix(after_comment_open);
    let s = s.strip_prefix("@if")?;
    let s = strip_leading_ws(s)?;
    let (neg, s) = match s.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let s = s.strip_prefix("CONFIG_")?;
    let (name, s) = split_name(s);
    if name.is_empty() {
        return None;
    }
    let s = trim_ws_prefix(s);
    s.strip_prefix("-->")?;
    Some((neg, name.to_string()))
}

fn try_endif_marker(after_comment_open: &str) -> bool {
    let s = trim_ws_prefix(after_comment_open);
    let Some(rest) = s.strip_prefix("@endif") else {
        return false;
    };
    rest.starts_with(' ') || rest.starts_with('\t') || rest.starts_with("-->")
}

fn scan_markers(line: &str) -> Marker {
    let mut search_from = 0usize;
    while let Some(pos) = line[search_from..].find("<!--") {
        let abs = search_from + pos;
        let after = &line[abs + 4..];
        if let Some((neg, name)) = try_if_marker(after) {
            return Marker::If { neg, name };
        }
        if try_endif_marker(after) {
            return Marker::EndIf;
        }
        search_from = abs + 4;
    }
    Marker::None
}

/// Matches `^CONFIG_[A-Za-z0-9_]+=y[ \t]*$`.
fn match_dot_config_yes(line: &str) -> Option<String> {
    let rest = line.strip_prefix("CONFIG_")?;
    let (name, remainder) = split_name(rest);
    if name.is_empty() {
        return None;
    }
    let value = remainder.strip_prefix('=')?;
    let value = value.trim_end_matches([' ', '\t']);
    if value == "y" {
        Some(name.to_string())
    } else {
        None
    }
}

/// Matches `^#[ \t]*CONFIG_[A-Za-z0-9_]+[ \t]+is not set` (no trailing
/// anchor — mirrors the looser regex `gensystem` uses to read `.config`).
fn match_dot_config_no(line: &str) -> Option<String> {
    let rest = line.strip_prefix('#')?;
    let rest = trim_ws_prefix(rest);
    let rest = rest.strip_prefix("CONFIG_")?;
    let (name, remainder) = split_name(rest);
    if name.is_empty() {
        return None;
    }
    let remainder = strip_leading_ws(remainder)?;
    if remainder.starts_with("is not set") {
        Some(name.to_string())
    } else {
        None
    }
}

fn parse_dot_config(text: &str) -> HashMap<String, bool> {
    let mut cfg = HashMap::new();
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(name) = match_dot_config_yes(line) {
            cfg.insert(name, true);
        } else if let Some(name) = match_dot_config_no(line) {
            cfg.insert(name, false);
        }
    }
    cfg
}

/// Preprocesses a `.system` template, keeping/dropping `@if`-guarded blocks
/// per the resolved `.config`. `template_name` is used in error messages
/// (`name:line: ...`), matching the original awk `FILENAME:FNR`.
pub fn gensystem(
    config_text: &str,
    template_text: &str,
    template_name: &str,
) -> Result<String, String> {
    let cfg = parse_dot_config(config_text);

    let mut depth: i32 = 0;
    let mut suppress: i32 = 0;
    let mut out = String::new();

    for (idx, raw_line) in template_text.lines().enumerate() {
        let lineno = idx + 1;
        match scan_markers(raw_line) {
            Marker::If { neg, name } => {
                let Some(&raw_sat) = cfg.get(&name) else {
                    return Err(format!(
                        "{template_name}:{lineno}: @if references CONFIG_{name}, which is not in the .config"
                    ));
                };
                depth += 1;
                if suppress == 0 {
                    let sat = if neg { !raw_sat } else { raw_sat };
                    if !sat {
                        suppress = depth;
                    }
                }
            }
            Marker::EndIf => {
                if depth == 0 {
                    return Err(format!(
                        "{template_name}:{lineno}: @endif without matching @if"
                    ));
                }
                if suppress == depth {
                    suppress = 0;
                }
                depth -= 1;
            }
            Marker::None => {
                if suppress == 0 {
                    out.push_str(raw_line);
                    out.push('\n');
                }
            }
        }
    }

    if depth != 0 {
        return Err(format!(
            "{template_name}: unterminated @if block (missing @endif)"
        ));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KCONFIG: &str = r#"
menu "Test options"

config ALPHA
	bool "Alpha"
	default y
	help
	  First test option.

config BETA
	bool "Beta"
	default n

config GAMMA
	bool "Gamma, needs beta"
	default n
	depends on BETA

config DELTA
	bool "Delta, needs beta and not alpha"
	default n
	depends on BETA && !ALPHA

endmenu
"#;

    const TEST_DEFCONFIG: &str = "\
# comment and blank lines are fine

CONFIG_BETA=y
# CONFIG_ALPHA is not set
";

    fn value(resolved: &[(String, bool)], name: &str) -> bool {
        resolved
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no such option: {name}"))
            .1
    }

    #[test]
    fn defaults_only() {
        let resolved = resolve(TEST_KCONFIG, None, &[]).unwrap();
        assert!(value(&resolved, "ALPHA"));
        assert!(!value(&resolved, "BETA"));
    }

    #[test]
    fn defconfig_overrides_defaults() {
        let resolved = resolve(TEST_KCONFIG, Some(("defconfig", TEST_DEFCONFIG)), &[]).unwrap();
        assert!(value(&resolved, "BETA"));
        assert!(!value(&resolved, "ALPHA"));
    }

    #[test]
    fn command_line_overrides_defconfig() {
        let resolved = resolve(
            TEST_KCONFIG,
            Some(("defconfig", TEST_DEFCONFIG)),
            &["CONFIG_BETA=n".to_string(), "CONFIG_ALPHA=y".to_string()],
        )
        .unwrap();
        assert!(!value(&resolved, "BETA"));
        assert!(value(&resolved, "ALPHA"));
    }

    #[test]
    fn dot_config_and_mk_rendering() {
        let resolved = resolve(
            TEST_KCONFIG,
            Some(("defconfig", TEST_DEFCONFIG)),
            &["CONFIG_BETA=n".to_string(), "CONFIG_ALPHA=y".to_string()],
        )
        .unwrap();
        let mk = render_config_mk(&resolved);
        assert!(mk.contains("CONFIG_ALPHA := y\n"));
        assert!(mk.contains("CONFIG_BETA := n\n"));
        let cfg = render_dot_config(&resolved, "defconfig");
        assert!(cfg.contains("CONFIG_ALPHA=y\n"));
        assert!(cfg.contains("# CONFIG_BETA is not set\n"));
    }

    #[test]
    fn unknown_option_in_set_is_rejected() {
        let err = resolve(TEST_KCONFIG, None, &["CONFIG_NOPE=y".to_string()]).unwrap_err();
        assert!(err.contains("unknown option CONFIG_NOPE"));
    }

    #[test]
    fn unknown_option_in_defconfig_is_rejected() {
        let err = resolve(
            TEST_KCONFIG,
            Some(("bad_defconfig", "CONFIG_NOPE=y\n")),
            &[],
        )
        .unwrap_err();
        assert!(err.contains("unknown option CONFIG_NOPE"));
    }

    #[test]
    fn non_bool_override_value_is_rejected() {
        let err = resolve(TEST_KCONFIG, None, &["CONFIG_ALPHA=maybe".to_string()]).unwrap_err();
        assert!(err.contains("is not of the form CONFIG_NAME=y|n"));
    }

    #[test]
    fn non_bool_defconfig_value_is_rejected() {
        let err = resolve(
            TEST_KCONFIG,
            Some(("bad_defconfig2", "CONFIG_ALPHA=true\n")),
            &[],
        )
        .unwrap_err();
        assert!(err.contains("must be y or n"));
    }

    #[test]
    fn duplicate_declaration_is_rejected() {
        let dup = "config DUP\n\tbool \"a\"\nconfig DUP\n\tbool \"b\"\n";
        let err = resolve(dup, None, &[]).unwrap_err();
        assert!(err.contains("duplicate option DUP"));
    }

    #[test]
    fn unsatisfied_depends_is_rejected() {
        let err = resolve(TEST_KCONFIG, None, &["CONFIG_GAMMA=y".to_string()]).unwrap_err();
        assert!(err.contains("requires"));
    }

    #[test]
    fn satisfied_depends_is_accepted() {
        resolve(
            TEST_KCONFIG,
            None,
            &["CONFIG_BETA=y".to_string(), "CONFIG_GAMMA=y".to_string()],
        )
        .unwrap();
    }

    #[test]
    fn negated_depends_rejected_when_alpha_on() {
        let err = resolve(
            TEST_KCONFIG,
            None,
            &["CONFIG_BETA=y".to_string(), "CONFIG_DELTA=y".to_string()],
        )
        .unwrap_err();
        assert!(err.contains("requires !CONFIG_ALPHA"));
    }

    #[test]
    fn negated_depends_accepted_when_alpha_off() {
        resolve(
            TEST_KCONFIG,
            Some(("defconfig", TEST_DEFCONFIG)),
            &["CONFIG_DELTA=y".to_string()],
        )
        .unwrap();
    }

    const TEMPLATE: &str = "\
<system>
    <always />
    <!-- @if CONFIG_BETA -->
    <beta-only />
    <!-- @if CONFIG_ALPHA -->
    <alpha-and-beta />
    <!-- @endif -->
    <!-- @endif -->
    <!-- @if !CONFIG_ALPHA -->
    <no-alpha />
    <!-- @endif -->
</system>
";

    #[test]
    fn gensystem_keeps_enabled_nested_blocks() {
        let resolved = resolve(
            TEST_KCONFIG,
            None,
            &["CONFIG_BETA=y".to_string(), "CONFIG_ALPHA=y".to_string()],
        )
        .unwrap();
        let cfg = render_dot_config(&resolved, "<none>");
        let out = gensystem(&cfg, TEMPLATE, "template.system").unwrap();
        assert!(out.contains("<beta-only />"));
        assert!(out.contains("<alpha-and-beta />"));
        assert!(!out.contains("<no-alpha />"));
        assert!(!out.contains("@if"));
        assert!(!out.contains("@endif"));
    }

    #[test]
    fn gensystem_strips_disabled_blocks_and_keeps_negated() {
        let resolved = resolve(
            TEST_KCONFIG,
            None,
            &["CONFIG_BETA=n".to_string(), "CONFIG_ALPHA=n".to_string()],
        )
        .unwrap();
        let cfg = render_dot_config(&resolved, "<none>");
        let out = gensystem(&cfg, TEMPLATE, "template.system").unwrap();
        assert!(!out.contains("beta-only"));
        assert!(!out.contains("alpha-and-beta"));
        assert!(out.contains("<no-alpha />"));
        assert!(out.contains("<always />"));
    }

    #[test]
    fn gensystem_rejects_unknown_option_in_marker() {
        let cfg = "CONFIG_ALPHA=y\n";
        let tmpl = "<!-- @if CONFIG_MISSING -->\n<!-- @endif -->\n";
        let err = gensystem(cfg, tmpl, "badref.system").unwrap_err();
        assert!(err.contains("which is not in the .config"));
    }

    #[test]
    fn gensystem_rejects_unterminated_if() {
        let cfg = "CONFIG_ALPHA=y\n";
        let tmpl = "<!-- @if CONFIG_ALPHA -->\n";
        let err = gensystem(cfg, tmpl, "unbalanced.system").unwrap_err();
        assert!(err.contains("unterminated @if block"));
    }

    #[test]
    fn gensystem_rejects_stray_endif() {
        let cfg = "CONFIG_ALPHA=y\n";
        let tmpl = "<!-- @endif -->\n";
        let err = gensystem(cfg, tmpl, "stray.system").unwrap_err();
        assert!(err.contains("@endif without matching @if"));
    }
}
