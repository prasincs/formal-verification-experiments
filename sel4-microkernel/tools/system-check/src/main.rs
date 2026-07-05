use std::env;
use std::path::{Path, PathBuf};

use system_check::{check_file, property_path};
use walkdir::WalkDir;

fn usage() -> ! {
    eprintln!("usage: system-check <file.system> [props.toml]\n       system-check --all <repository-root>");
    std::process::exit(2);
}

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let result = match args.as_slice() {
        [flag, root] if flag == "--all" => check_all(Path::new(root)),
        [system] => {
            let system = PathBuf::from(system);
            let props = property_path(&system);
            check_one(&system, &props)
        }
        [system, props] => check_one(Path::new(system), Path::new(props)),
        _ => usage(),
    };

    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn check_one(system: &Path, props: &Path) -> Result<(), String> {
    check_file(system, props)
        .map(|graph| {
            println!(
                "OK {}: {} PDs, {} regions, {} channels, {} PP edges",
                system.display(),
                graph.pds.len(),
                graph.regions.len(),
                graph.channels.len(),
                graph.pp_edges.len()
            );
        })
        .map_err(|error| format!("{}: {error}", system.display()))
}

fn check_all(root: &Path) -> Result<(), String> {
    let mut systems: Vec<_> = WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "system"))
        .filter(|path| !path.components().any(|part| part.as_os_str() == "target"))
        .collect();
    systems.sort();

    if systems.is_empty() {
        return Err(format!("no .system files found under {}", root.display()));
    }

    let mut errors = Vec::new();
    for system in systems {
        let props = property_path(&system);
        if !props.exists() {
            errors.push(format!(
                "{}: missing sidecar {}",
                system.display(),
                props.display()
            ));
            continue;
        }
        if let Err(error) = check_one(&system, &props) {
            errors.push(error);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}
