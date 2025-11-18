use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

fn save_cmd() -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("save-cli"));
    cmd.env_remove("S3_ENDPOINT");
    cmd.env_remove("AWS_ACCESS_KEY_ID");
    cmd.env_remove("AWS_SECRET_ACCESS_KEY");
    cmd
}

#[test]
fn test_help_command() {
    save_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("CLI tool for save object storage"))
        .stdout(predicate::str::contains("health"))
        .stdout(predicate::str::contains("bucket"))
        .stdout(predicate::str::contains("object"));
}

#[test]
fn test_health_subcommand_help() {
    save_cmd()
        .args(["health", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Check server health"));
}

#[test]
fn test_bucket_subcommand_help() {
    save_cmd()
        .args(["bucket", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Bucket operations"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("create"))
        .stdout(predicate::str::contains("delete"));
}

#[test]
fn test_object_subcommand_help() {
    save_cmd()
        .args(["object", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Object operations"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("get"))
        .stdout(predicate::str::contains("put"))
        .stdout(predicate::str::contains("delete"));
}

#[test]
fn test_access_key_env_var() {
    save_cmd()
        .env("AWS_ACCESS_KEY_ID", "custom-key")
        .args(["--help"])
        .assert()
        .success();
}

#[test]
fn test_missing_required_args() {
    save_cmd()
        .args(["bucket", "create"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required").or(predicate::str::contains("argument")));
}

#[test]
fn test_object_put_args() {
    save_cmd()
        .args(["object", "put", "bucket", "key"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required").or(predicate::str::contains("file")));
}
