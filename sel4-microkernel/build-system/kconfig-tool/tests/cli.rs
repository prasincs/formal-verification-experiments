//! End-to-end CLI tests: invokes the compiled `kconfig-tool` binary the
//! same way `config/kconfig.mk` does, and exercises it against the real
//! repository Kconfig/defconfig/.system files. This is the CLI-level
//! counterpart to the unit tests in `src/lib.rs`; together they cover the
//! cases the old `scripts/test-kconfig.sh` shell harness did.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kconfig-tool"))
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// build-system/ is the parent of this crate; sel4-microkernel/ is its parent.
fn build_system_dir() -> PathBuf {
    manifest_dir().parent().unwrap().to_path_buf()
}

fn repo_root_dir() -> PathBuf {
    build_system_dir().parent().unwrap().to_path_buf()
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("failed to run kconfig-tool")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_kconfig_error(output: &Output, label: &str) {
    assert!(
        !output.status.success(),
        "{label}: expected failure, got success"
    );
    assert!(
        stderr(output).contains("kconfig: error:"),
        "{label}: failed without a kconfig error message: {}",
        stderr(output)
    );
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "kconfig-tool-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const TEST_KCONFIG: &str = r#"menu "Test options"

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

fn config_value(config_path: &Path, name: &str) -> String {
    let text = fs::read_to_string(config_path).unwrap();
    if text
        .lines()
        .any(|l| l == format!("CONFIG_{name}=y"))
    {
        "y".to_string()
    } else if text
        .lines()
        .any(|l| l == format!("# CONFIG_{name} is not set"))
    {
        "n".to_string()
    } else {
        "missing".to_string()
    }
}

#[test]
fn resolve_layering_and_config_mk() {
    let tmp = TempDir::new("layering");
    let kconfig = tmp.path("Kconfig");
    let defconfig = tmp.path("defconfig");
    fs::write(&kconfig, TEST_KCONFIG).unwrap();
    fs::write(&defconfig, TEST_DEFCONFIG).unwrap();
    let out_config = tmp.path(".config");
    let out_mk = tmp.path("config.mk");

    // Defaults only.
    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "resolve with defaults only: {}", stderr(&out));
    assert_eq!(config_value(&out_config, "ALPHA"), "y", "default y is applied");
    assert_eq!(config_value(&out_config, "BETA"), "n", "default n is applied");

    // Defconfig layered on top.
    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--defconfig",
        defconfig.to_str().unwrap(),
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "resolve with defconfig: {}", stderr(&out));
    assert_eq!(config_value(&out_config, "BETA"), "y", "defconfig =y overrides default");
    assert_eq!(
        config_value(&out_config, "ALPHA"),
        "n",
        "defconfig 'is not set' overrides default"
    );

    // Command-line overrides on top of defconfig.
    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--defconfig",
        defconfig.to_str().unwrap(),
        "--set",
        "CONFIG_BETA=n",
        "--set",
        "CONFIG_ALPHA=y",
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "resolve with overrides: {}", stderr(&out));
    assert_eq!(config_value(&out_config, "BETA"), "n", "--set overrides defconfig (to n)");
    assert_eq!(config_value(&out_config, "ALPHA"), "y", "--set overrides defconfig (to y)");

    let mk = fs::read_to_string(&out_mk).unwrap();
    assert!(mk.lines().any(|l| l == "CONFIG_ALPHA := y"), "config.mk contains y value");
    assert!(mk.lines().any(|l| l == "CONFIG_BETA := n"), "config.mk contains n value");
}

#[test]
fn resolve_validation_errors() {
    let tmp = TempDir::new("validation");
    let kconfig = tmp.path("Kconfig");
    fs::write(&kconfig, TEST_KCONFIG).unwrap();
    let out_config = tmp.path("x");
    let out_mk = tmp.path("y");

    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--set",
        "CONFIG_NOPE=y",
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert_kconfig_error(&out, "unknown option in --set is rejected");

    let bad_defconfig = tmp.path("bad_defconfig");
    fs::write(&bad_defconfig, "CONFIG_NOPE=y\n").unwrap();
    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--defconfig",
        bad_defconfig.to_str().unwrap(),
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert_kconfig_error(&out, "unknown option in defconfig is rejected");

    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--set",
        "CONFIG_ALPHA=maybe",
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert_kconfig_error(&out, "non-bool override value is rejected");

    let bad_defconfig2 = tmp.path("bad_defconfig2");
    fs::write(&bad_defconfig2, "CONFIG_ALPHA=true\n").unwrap();
    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--defconfig",
        bad_defconfig2.to_str().unwrap(),
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert_kconfig_error(&out, "non-bool defconfig value is rejected");

    let dup_kconfig = tmp.path("dup_kconfig");
    fs::write(&dup_kconfig, "config DUP\n\tbool \"a\"\nconfig DUP\n\tbool \"b\"\n").unwrap();
    let out = run(&[
        "resolve",
        "--kconfig",
        dup_kconfig.to_str().unwrap(),
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert_kconfig_error(&out, "duplicate declaration is rejected");

    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--set",
        "CONFIG_GAMMA=y",
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert_kconfig_error(&out, "unsatisfied depends is rejected");

    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--set",
        "CONFIG_BETA=y",
        "--set",
        "CONFIG_GAMMA=y",
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "satisfied depends is accepted: {}", stderr(&out));

    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--set",
        "CONFIG_BETA=y",
        "--set",
        "CONFIG_DELTA=y",
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert_kconfig_error(&out, "negated depends (!ALPHA with ALPHA=y) is rejected");

    let defconfig = tmp.path("defconfig");
    fs::write(&defconfig, TEST_DEFCONFIG).unwrap();
    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--defconfig",
        defconfig.to_str().unwrap(),
        "--set",
        "CONFIG_DELTA=y",
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "negated depends (!ALPHA with ALPHA=n) is accepted: {}",
        stderr(&out)
    );
}

#[test]
fn resolve_output_mtime_stability() {
    let tmp = TempDir::new("mtime");
    let kconfig = tmp.path("Kconfig");
    let defconfig = tmp.path("defconfig");
    fs::write(&kconfig, TEST_KCONFIG).unwrap();
    fs::write(&defconfig, TEST_DEFCONFIG).unwrap();
    let out_config = tmp.path(".config");
    let out_mk = tmp.path("config.mk");

    let resolve_args = [
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--defconfig",
        defconfig.to_str().unwrap(),
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ];

    let out = run(&resolve_args);
    assert!(out.status.success(), "resolve for mtime test: {}", stderr(&out));
    let before = fs::metadata(&out_config).unwrap().modified().unwrap();

    // Ensure a coarse filesystem clock can't accidentally show "unchanged".
    std::thread::sleep(std::time::Duration::from_millis(20));

    let out = run(&resolve_args);
    assert!(out.status.success(), "re-resolve for mtime test: {}", stderr(&out));
    let after = fs::metadata(&out_config).unwrap().modified().unwrap();

    assert_eq!(before, after, "unchanged .config is not rewritten");
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
fn gensystem_marker_semantics() {
    let tmp = TempDir::new("gensystem");
    let kconfig = tmp.path("Kconfig");
    fs::write(&kconfig, TEST_KCONFIG).unwrap();
    let out_config = tmp.path(".config");
    let out_mk = tmp.path("config.mk");
    let template = tmp.path("template.system");
    fs::write(&template, TEMPLATE).unwrap();

    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--set",
        "CONFIG_BETA=y",
        "--set",
        "CONFIG_ALPHA=y",
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "resolve for gensystem: {}", stderr(&out));

    let out_system = tmp.path("out.system");
    let out = run(&[
        "gensystem",
        "--config",
        out_config.to_str().unwrap(),
        "--in",
        template.to_str().unwrap(),
        "--out",
        out_system.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "gensystem runs: {}", stderr(&out));
    let generated = fs::read_to_string(&out_system).unwrap();
    assert!(generated.contains("<beta-only />"), "enabled block kept");
    assert!(generated.contains("<alpha-and-beta />"), "nested enabled block kept");
    assert!(!generated.contains("<no-alpha />"), "negated block stripped");
    assert!(!generated.contains("@if") && !generated.contains("@endif"), "markers removed");

    let out = run(&[
        "resolve",
        "--kconfig",
        kconfig.to_str().unwrap(),
        "--set",
        "CONFIG_BETA=n",
        "--set",
        "CONFIG_ALPHA=n",
        "--out-config",
        out_config.to_str().unwrap(),
        "--out-mk",
        out_mk.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "resolve for gensystem (off): {}", stderr(&out));

    let out_system2 = tmp.path("out2.system");
    let out = run(&[
        "gensystem",
        "--config",
        out_config.to_str().unwrap(),
        "--in",
        template.to_str().unwrap(),
        "--out",
        out_system2.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "gensystem runs (off): {}", stderr(&out));
    let generated2 = fs::read_to_string(&out_system2).unwrap();
    assert!(
        !generated2.contains("beta-only") && !generated2.contains("alpha-and-beta"),
        "disabled blocks stripped"
    );
    assert!(generated2.contains("<no-alpha />"), "negated block kept when option off");
    assert!(generated2.contains("<always />"), "unguarded content kept");

    let badref = tmp.path("badref.system");
    fs::write(&badref, "<!-- @if CONFIG_MISSING -->\n<!-- @endif -->\n").unwrap();
    let out = run(&[
        "gensystem",
        "--config",
        out_config.to_str().unwrap(),
        "--in",
        badref.to_str().unwrap(),
        "--out",
        tmp.path("x.system").to_str().unwrap(),
    ]);
    assert_kconfig_error(&out, "gensystem rejects unknown option in marker");

    let unbalanced = tmp.path("unbalanced.system");
    fs::write(&unbalanced, "<!-- @if CONFIG_ALPHA -->\n").unwrap();
    let out = run(&[
        "gensystem",
        "--config",
        out_config.to_str().unwrap(),
        "--in",
        unbalanced.to_str().unwrap(),
        "--out",
        tmp.path("x.system").to_str().unwrap(),
    ]);
    assert_kconfig_error(&out, "gensystem rejects unterminated @if");

    let stray = tmp.path("stray.system");
    fs::write(&stray, "<!-- @endif -->\n").unwrap();
    let out = run(&[
        "gensystem",
        "--config",
        out_config.to_str().unwrap(),
        "--in",
        stray.to_str().unwrap(),
        "--out",
        tmp.path("x.system").to_str().unwrap(),
    ]);
    assert_kconfig_error(&out, "gensystem rejects stray @endif");
}

#[test]
fn repo_defconfigs_resolve() {
    let bs = build_system_dir();
    let kconfig = bs.join("Kconfig");
    let tmp = TempDir::new("repo-defconfigs");
    let out_config = tmp.path("real.config");
    let out_mk = tmp.path("real.mk");

    let configs_dir = bs.join("configs");
    let mut found_any = false;
    for entry in fs::read_dir(&configs_dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if !name.ends_with("_defconfig") {
            continue;
        }
        found_any = true;
        let out = run(&[
            "resolve",
            "--kconfig",
            kconfig.to_str().unwrap(),
            "--defconfig",
            entry.path().to_str().unwrap(),
            "--out-config",
            out_config.to_str().unwrap(),
            "--out-mk",
            out_mk.to_str().unwrap(),
        ]);
        assert!(
            out.status.success(),
            "repo defconfig resolves: {name} ({})",
            stderr(&out)
        );
    }
    assert!(found_any, "expected at least one *_defconfig under {configs_dir:?}");
}

#[test]
fn repo_system_templates_gensystem() {
    let bs = build_system_dir();
    let root = repo_root_dir();
    let kconfig = bs.join("Kconfig");
    let tmp = TempDir::new("repo-systems");
    let out_config = tmp.path("real.config");
    let out_mk = tmp.path("real.mk");

    let systems = [
        root.join("rpi4-photoframe/photoframe.system"),
        root.join("rpi4-graphics/tvdemo-input.system"),
        root.join("rpi4-graphics/tvdemo-network.system"),
    ];

    for sys in &systems {
        assert!(sys.is_file(), "expected .system file at {sys:?}");
        for usb in ["y", "n"] {
            let out = run(&[
                "resolve",
                "--kconfig",
                kconfig.to_str().unwrap(),
                "--set",
                &format!("CONFIG_INPUT_USB_KEYBOARD={usb}"),
                "--out-config",
                out_config.to_str().unwrap(),
                "--out-mk",
                out_mk.to_str().unwrap(),
            ]);
            assert!(out.status.success(), "resolve usb={usb}: {}", stderr(&out));

            let out_system = tmp.path("real.system");
            let out = run(&[
                "gensystem",
                "--config",
                out_config.to_str().unwrap(),
                "--in",
                sys.to_str().unwrap(),
                "--out",
                out_system.to_str().unwrap(),
            ]);
            assert!(
                out.status.success(),
                "gensystem processes {:?} (usb={usb}): {}",
                sys.file_name().unwrap(),
                stderr(&out)
            );

            let generated = fs::read_to_string(&out_system).unwrap();
            let usb_count = generated.matches("mr=\"usb_").count();
            if usb == "y" {
                assert!(usb_count > 0, "{:?} maps USB when enabled", sys.file_name().unwrap());
            } else {
                assert_eq!(usb_count, 0, "{:?} omits USB when disabled", sys.file_name().unwrap());
            }
            assert!(
                !generated.contains("@if") && !generated.contains("@endif"),
                "{:?} markers removed (usb={usb})",
                sys.file_name().unwrap()
            );
        }
    }
}
