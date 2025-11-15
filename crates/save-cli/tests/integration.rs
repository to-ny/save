use std::process::Command;

fn save_bin() -> Command {
    let bin_path = std::env::var("CARGO_BIN_EXE_save-cli")
        .unwrap_or_else(|_| "target/debug/save-cli".to_string());
    let mut cmd = Command::new(bin_path);
    cmd.env_remove("S3_ENDPOINT");
    cmd.env_remove("AWS_ACCESS_KEY_ID");
    cmd.env_remove("AWS_SECRET_ACCESS_KEY");
    cmd
}

#[test]
fn test_help_command() {
    let output = save_bin().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("CLI tool for save object storage"));
    assert!(stdout.contains("health"));
    assert!(stdout.contains("bucket"));
    assert!(stdout.contains("object"));
}

#[test]
fn test_health_subcommand_help() {
    let output = save_bin().args(["health", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Check server health"));
}

#[test]
fn test_bucket_subcommand_help() {
    let output = save_bin().args(["bucket", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Bucket operations"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("create"));
    assert!(stdout.contains("delete"));
}

#[test]
fn test_object_subcommand_help() {
    let output = save_bin().args(["object", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Object operations"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("get"));
    assert!(stdout.contains("put"));
    assert!(stdout.contains("delete"));
}

#[test]
fn test_default_endpoint() {
    let output = save_bin().args(["health"]).output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn test_custom_endpoint_flag() {
    let output = save_bin()
        .args(["--endpoint", "http://localhost:19000", "health"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn test_endpoint_env_var() {
    let output = save_bin()
        .env("S3_ENDPOINT", "http://localhost:19000")
        .args(["health"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn test_access_key_env_var() {
    let output = save_bin()
        .env("AWS_ACCESS_KEY_ID", "custom-key")
        .args(["--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn test_missing_required_args() {
    let output = save_bin().args(["bucket", "create"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("required") || stderr.contains("argument"));
}

#[test]
fn test_bucket_list_requires_endpoint() {
    let output = save_bin()
        .args(["--endpoint", "http://nonexistent:9999", "bucket", "list"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn test_object_put_args() {
    let output = save_bin()
        .args(["object", "put", "bucket", "key"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("required") || stderr.contains("file"));
}
