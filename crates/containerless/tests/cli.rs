use std::fs;
use std::path::Path;

use containerless::{CliError, run_cli};

#[tokio::test]
async fn builds_an_oci_archive() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("app");
    let config = temporary.path().join("containerless.toml");
    let output = temporary.path().join("app.tar");
    fs::write(&source, b"hello\n").unwrap();
    fs::write(
        &config,
        r#"
            [images.app]
            files = [{ from = "app", to = "/app", mode = "0755" }]
            entrypoint = ["/app"]
        "#,
    )
    .unwrap();

    run_cli([
        "containerless",
        "--file",
        config.to_str().unwrap(),
        "build",
        "app",
        "--output",
        output.to_str().unwrap(),
        "--tag",
        "example.com/app:test",
    ])
    .await
    .unwrap();

    let mut archive = tar::Archive::new(fs::File::open(output).unwrap());
    assert!(
        archive
            .entries()
            .unwrap()
            .any(|entry| { entry.unwrap().path().unwrap() == Path::new("index.json") })
    );
}

#[tokio::test]
async fn rejects_invalid_output_options() {
    assert_arguments_error(run_cli(["containerless", "build", "app"]).await);
    assert_arguments_error(run_cli(["containerless", "build", "app", "--format", "oci-dir"]).await);
    assert_arguments_error(run_cli(["containerless", "build", "app", "--engine", "podman"]).await);
}

#[tokio::test]
async fn rejects_invalid_platform_and_copy_values() {
    assert_arguments_error(
        run_cli([
            "containerless",
            "build",
            "app",
            "--output",
            "app.tar",
            "--platform",
            "linux/amd64/variant/extra",
        ])
        .await,
    );
    assert_arguments_error(
        run_cli([
            "containerless",
            "build",
            "app",
            "--output",
            "app.tar",
            "--copy",
            "platform=linux/arm64,dist/app:/app",
        ])
        .await,
    );
}

fn assert_arguments_error(result: Result<(), CliError>) {
    assert!(matches!(result, Err(CliError::Arguments(_))));
}
