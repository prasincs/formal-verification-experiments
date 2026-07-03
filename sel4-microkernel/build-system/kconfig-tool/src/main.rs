//! CLI for the Kconfig-style build configuration tool.
//!
//! Subcommands (see `../README.md` or `../../docs/build-configuration.md`
//! for the full contract):
//!
//!   resolve   --kconfig <Kconfig> --defconfig <file>
//!             [--set CONFIG_NAME=y|n]...
//!             --out-config <.config> --out-mk <config.mk>
//!
//!   gensystem --config <.config> --in <template.system> --out <file>

use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn die(msg: &str) -> ! {
    eprintln!("kconfig: error: {msg}");
    std::process::exit(1);
}

fn usage() -> ExitCode {
    eprintln!(
        "kconfig-tool — minimal Kconfig-style configuration tool for the build system\n\n\
         Subcommands:\n\n\
         \x20 resolve   --kconfig <Kconfig> --defconfig <file>\n\
         \x20           [--set CONFIG_NAME=y|n]...\n\
         \x20           --out-config <.config> --out-mk <config.mk>\n\n\
         \x20 gensystem --config <.config> --in <template.system> --out <file>"
    );
    ExitCode::from(2)
}

/// Writes `content` to `path` only if it differs from the existing content,
/// so make dependencies (mtimes) stay stable across no-op re-resolves.
fn write_if_changed(path: &Path, content: &str) -> std::io::Result<()> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return Ok(());
        }
    }
    fs::write(path, content)
}

fn cmd_resolve(args: &[String]) -> ExitCode {
    let mut kconfig: Option<String> = None;
    let mut defconfig: Option<String> = None;
    let mut out_config: Option<String> = None;
    let mut out_mk: Option<String> = None;
    let mut overrides: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--kconfig" => {
                kconfig = Some(require_value(args, &mut i, "resolve"));
            }
            "--defconfig" => {
                defconfig = Some(require_value(args, &mut i, "resolve"));
            }
            "--set" => {
                overrides.push(require_value(args, &mut i, "resolve"));
            }
            "--out-config" => {
                out_config = Some(require_value(args, &mut i, "resolve"));
            }
            "--out-mk" => {
                out_mk = Some(require_value(args, &mut i, "resolve"));
            }
            other => die(&format!("resolve: unknown argument '{other}'")),
        }
    }

    let kconfig = kconfig.unwrap_or_else(|| die("resolve: --kconfig is required"));
    if !Path::new(&kconfig).is_file() {
        die(&format!("resolve: Kconfig file not found: {kconfig}"));
    }
    let out_config = out_config.unwrap_or_else(|| die("resolve: --out-config is required"));
    let out_mk = out_mk.unwrap_or_else(|| die("resolve: --out-mk is required"));
    if let Some(d) = &defconfig {
        if !Path::new(d).is_file() {
            die(&format!("resolve: defconfig not found: {d}"));
        }
    }

    let kconfig_text = fs::read_to_string(&kconfig)
        .unwrap_or_else(|e| die(&format!("resolve: cannot read {kconfig}: {e}")));

    let defconfig_text;
    let defconfig_ref = match &defconfig {
        Some(path) => {
            defconfig_text = fs::read_to_string(path)
                .unwrap_or_else(|e| die(&format!("resolve: cannot read {path}: {e}")));
            Some((path.as_str(), defconfig_text.as_str()))
        }
        None => None,
    };

    let resolved = match kconfig_tool::resolve(&kconfig_text, defconfig_ref, &overrides) {
        Ok(r) => r,
        Err(e) => die(&e),
    };

    let defconfig_name = defconfig.as_deref().unwrap_or("<none>");
    let dot_config = kconfig_tool::render_dot_config(&resolved, defconfig_name);
    let config_mk = kconfig_tool::render_config_mk(&resolved);

    if let Err(e) = write_if_changed(Path::new(&out_config), &dot_config) {
        die(&format!("resolve: cannot write {out_config}: {e}"));
    }
    if let Err(e) = write_if_changed(Path::new(&out_mk), &config_mk) {
        die(&format!("resolve: cannot write {out_mk}: {e}"));
    }

    ExitCode::SUCCESS
}

fn cmd_gensystem(args: &[String]) -> ExitCode {
    let mut config: Option<String> = None;
    let mut infile: Option<String> = None;
    let mut outfile: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => config = Some(require_value(args, &mut i, "gensystem")),
            "--in" => infile = Some(require_value(args, &mut i, "gensystem")),
            "--out" => outfile = Some(require_value(args, &mut i, "gensystem")),
            other => die(&format!("gensystem: unknown argument '{other}'")),
        }
    }

    match &config {
        Some(c) if Path::new(c).is_file() => {}
        other => die(&format!(
            "gensystem: --config file not found: {}",
            other.as_deref().unwrap_or("<unset>")
        )),
    }
    match &infile {
        Some(f) if Path::new(f).is_file() => {}
        other => die(&format!(
            "gensystem: --in file not found: {}",
            other.as_deref().unwrap_or("<unset>")
        )),
    }
    let outfile = outfile.unwrap_or_else(|| die("gensystem: --out is required"));

    let config = config.unwrap();
    let infile = infile.unwrap();

    let config_text = fs::read_to_string(&config)
        .unwrap_or_else(|e| die(&format!("gensystem: cannot read {config}: {e}")));
    let template_text = fs::read_to_string(&infile)
        .unwrap_or_else(|e| die(&format!("gensystem: cannot read {infile}: {e}")));

    let output = match kconfig_tool::gensystem(&config_text, &template_text, &infile) {
        Ok(o) => o,
        Err(e) => die(&e),
    };

    if let Err(e) = fs::write(&outfile, output) {
        die(&format!("gensystem: cannot write {outfile}: {e}"));
    }

    ExitCode::SUCCESS
}

fn require_value(args: &[String], i: &mut usize, subcmd: &str) -> String {
    let flag = &args[*i];
    let value = args.get(*i + 1).unwrap_or_else(|| {
        die(&format!("{subcmd}: missing value for '{flag}'"));
    });
    *i += 2;
    value.clone()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        return usage();
    };
    match cmd.as_str() {
        "resolve" => cmd_resolve(rest),
        "gensystem" => cmd_gensystem(rest),
        "-h" | "--help" | "help" => usage(),
        other => die(&format!(
            "unknown subcommand '{other}' (expected: resolve, gensystem)"
        )),
    }
}
