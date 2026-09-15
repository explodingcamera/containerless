use std::fs;
use std::path::Path;

use containerless::config::{Config, Format};

#[test]
fn self_packaging_config_is_valid() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../containerless");
    Config::load(&path).unwrap();
}

#[test]
fn files_are_always_a_list() {
    let error = Config::parse(
        r#"
            [images.app]
            files = { from = "dist/app", to = "/app" }
        "#,
        Format::Toml,
    )
    .unwrap_err();
    assert!(error.to_string().contains("sequence"));
}

#[test]
fn file_modes_are_quoted_octal_strings() {
    let config = Config::parse(
        r#"
            [images.app]
            files = [{ from = "dist/app", to = "/app", mode = "0755" }]
        "#,
        Format::Toml,
    )
    .unwrap();
    assert_eq!(config.images["app"].files.len(), 1);

    let error = Config::parse(
        r#"
            [images.app]
            files = [{ from = "dist/app", to = "/app", mode = 755 }]
        "#,
        Format::Toml,
    )
    .unwrap_err();
    assert!(error.to_string().contains("quoted octal string"));
}

#[test]
fn explicit_platforms_require_matching_file_sources() {
    let error = Config::parse(
        r#"
            [images.app]
            platforms = ["linux/amd64", "linux/arm64"]
            files = [{
                from = { "linux/amd64" = "dist/amd64/app" },
                to = "/app"
            }]
        "#,
        Format::Toml,
    )
    .unwrap_err();
    assert!(error.to_string().contains("linux/arm64"));
}

#[test]
fn infers_platforms_from_file_sources() {
    let config = Config::parse(
        r#"
            [images.app]
            files = [{
                from = {
                    "linux/arm64" = "dist/arm64/app",
                    "linux/amd64" = "dist/amd64/app"
                },
                to = "/app"
            }]
        "#,
        Format::Toml,
    )
    .unwrap();
    assert_eq!(
        config.platforms_for_image("app").unwrap(),
        ["linux/amd64", "linux/arm64"]
    );
}

#[test]
fn validates_child_platforms_against_local_base_files() {
    let error = Config::parse(
        r#"
            [images.base]
            platforms = ["linux/amd64"]
            files = [{ from = "base", to = "/base" }]

            [images.app]
            base = { image = "base" }
            platforms = ["linux/arm64"]
        "#,
        Format::Toml,
    )
    .unwrap_err();
    assert!(error.to_string().contains("linux/arm64"));
}

#[test]
fn defaults_to_linux_on_the_host_architecture() {
    let config = Config::parse("[images.app]", Format::Toml).unwrap();
    let platforms = config.platforms_for_image("app").unwrap();
    assert_eq!(platforms.len(), 1);
    assert!(platforms[0].starts_with("linux/"));
}

#[test]
#[ignore = "writes containerless.schema.json"]
fn generate_config_schema() {
    let mut schema = serde_json::to_value(schemars::schema_for!(Config)).unwrap();
    schema.as_object_mut().unwrap().insert(
        "x-tombi-toml-version".to_owned(),
        serde_json::json!("v1.1.0"),
    );
    let json = serde_json::to_string_pretty(&schema).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../containerless.schema.json");
    fs::write(path, format!("{json}\n")).unwrap();
}
