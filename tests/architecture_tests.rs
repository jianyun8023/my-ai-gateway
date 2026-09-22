//! Lightweight source-level guards for explicit backend dependency boundaries.
//! These complement Rust visibility; they are not a complete dependency graph.
use std::{fs, path::Path};

fn check_sources(directory: &Path, forbidden: &[&str]) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            check_sources(&path, forbidden);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("tests.rs")
        {
            let source = fs::read_to_string(&path).unwrap();
            // This repository keeps inline test modules at the end of a file.
            let production = source.split("#[cfg(test)]").next().unwrap();
            for dependency in forbidden {
                assert!(
                    !production.contains(dependency),
                    "{} crosses a backend boundary via {dependency}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn api_does_not_execute_sql() {
    check_sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/api"),
        &[
            "sqlx::query",
            "use sqlx::{query",
            "use sqlx::query",
            ".execute(",
        ],
    );
}

#[test]
fn quota_service_has_no_handler_or_application_state_dependency() {
    check_sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/control_plane/quota"),
        &["axum::", "AppState", "IntoResponse", "http::response"],
    );
}
