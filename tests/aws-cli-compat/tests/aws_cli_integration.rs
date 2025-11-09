#![cfg(feature = "compat_tests")]

use serde_json::Value;
use std::process::Command;

/// Helper to run AWS CLI commands with proper configuration
struct AwsCli {
    endpoint: String,
    access_key: String,
    secret_key: String,
}

impl AwsCli {
    fn new() -> Self {
        Self {
            endpoint: "http://localhost:9000".to_string(),
            access_key: "test-access-key".to_string(),
            secret_key: "test-secret-key".to_string(),
        }
    }

    /// Execute an AWS CLI S3 command and return the output
    fn run(&self, args: &[&str]) -> CliResult {
        let mut cmd = Command::new("aws");

        // Set AWS credentials via environment variables
        cmd.env("AWS_ACCESS_KEY_ID", &self.access_key);
        cmd.env("AWS_SECRET_ACCESS_KEY", &self.secret_key);
        cmd.env("AWS_REGION", "us-east-1");

        // Add endpoint and output format
        cmd.arg("s3api");
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg("--endpoint-url").arg(&self.endpoint);
        cmd.arg("--output").arg("json");

        let output = cmd.output().expect("Failed to execute aws cli");

        CliResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        }
    }

    /// Execute an AWS CLI S3 command (high-level S3 commands, not s3api)
    fn run_s3(&self, args: &[&str]) -> CliResult {
        let mut cmd = Command::new("aws");

        cmd.env("AWS_ACCESS_KEY_ID", &self.access_key);
        cmd.env("AWS_SECRET_ACCESS_KEY", &self.secret_key);
        cmd.env("AWS_REGION", "us-east-1");

        cmd.arg("s3");
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg("--endpoint-url").arg(&self.endpoint);

        let output = cmd.output().expect("Failed to execute aws cli");

        CliResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        }
    }
}

struct CliResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

impl CliResult {
    fn is_success(&self) -> bool {
        self.exit_code == 0
    }

    fn parse_json(&self) -> Option<Value> {
        serde_json::from_str(&self.stdout).ok()
    }

    fn contains_error(&self, error_code: &str) -> bool {
        self.stderr.contains(error_code) || self.stdout.contains(error_code)
    }
}

fn check_aws_cli_installed() -> bool {
    Command::new("aws")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_cli_create_and_list_buckets() {
    if !check_aws_cli_installed() {
        eprintln!("Skipping: AWS CLI not installed");
        return;
    }

    let cli = AwsCli::new();
    let bucket_name = format!(
        "cli-test-{}",
        uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
    );

    // Create bucket
    let result = cli.run(&["create-bucket", "--bucket", &bucket_name]);
    assert!(
        result.is_success(),
        "Failed to create bucket: {}",
        result.stderr
    );

    // List buckets
    let result = cli.run(&["list-buckets"]);
    assert!(
        result.is_success(),
        "Failed to list buckets: {}",
        result.stderr
    );

    let json = result.parse_json().expect("Failed to parse JSON");
    let buckets = json["Buckets"].as_array().expect("No Buckets array");
    assert!(
        buckets
            .iter()
            .any(|b| b["Name"].as_str() == Some(&bucket_name)),
        "Created bucket not found in list"
    );

    // Cleanup
    cli.run(&["delete-bucket", "--bucket", &bucket_name]);
}

#[test]
fn test_cli_bucket_already_exists_error() {
    if !check_aws_cli_installed() {
        eprintln!("Skipping: AWS CLI not installed");
        return;
    }

    let cli = AwsCli::new();
    let bucket_name = format!(
        "cli-dup-{}",
        uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
    );

    // Create bucket first time
    let result = cli.run(&["create-bucket", "--bucket", &bucket_name]);
    assert!(
        result.is_success(),
        "Failed to create bucket: {}",
        result.stderr
    );

    // Try to create again - should fail with BucketAlreadyExists
    let result = cli.run(&["create-bucket", "--bucket", &bucket_name]);
    assert!(!result.is_success(), "Second create should have failed");

    // Check error message contains the right error code
    assert!(
        result.contains_error("BucketAlreadyExists")
            || result.contains_error("BucketAlreadyOwnedByYou")
            || result.stderr.contains("already exists"),
        "Error should indicate bucket already exists. stderr: {}",
        result.stderr
    );

    // Cleanup
    cli.run(&["delete-bucket", "--bucket", &bucket_name]);
}

#[test]
fn test_cli_no_such_bucket_error() {
    if !check_aws_cli_installed() {
        eprintln!("Skipping: AWS CLI not installed");
        return;
    }

    let cli = AwsCli::new();
    let bucket_name = format!("cli-nonexistent-{}", uuid::Uuid::new_v4().simple());

    // Try to delete non-existent bucket
    let result = cli.run(&["delete-bucket", "--bucket", &bucket_name]);
    assert!(
        !result.is_success(),
        "Delete of non-existent bucket should fail"
    );

    // Check error message
    assert!(
        result.contains_error("NoSuchBucket") || result.stderr.contains("does not exist"),
        "Error should indicate NoSuchBucket. stderr: {}",
        result.stderr
    );
}

#[test]
fn test_cli_put_and_get_object() {
    if !check_aws_cli_installed() {
        eprintln!("Skipping: AWS CLI not installed");
        return;
    }

    let cli = AwsCli::new();
    let bucket_name = format!(
        "cli-obj-{}",
        uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
    );

    // Create bucket
    let result = cli.run(&["create-bucket", "--bucket", &bucket_name]);
    assert!(
        result.is_success(),
        "Failed to create bucket: {}",
        result.stderr
    );

    // Create a temporary file
    let temp_file = std::env::temp_dir().join("test-upload.txt");
    std::fs::write(&temp_file, b"Hello from AWS CLI test!").unwrap();

    // Upload object using s3 cp command
    let result = cli.run_s3(&[
        "cp",
        temp_file.to_str().unwrap(),
        &format!("s3://{}/test.txt", bucket_name),
    ]);
    assert!(
        result.is_success(),
        "Failed to upload object: {}",
        result.stderr
    );

    // Download object
    let download_file = std::env::temp_dir().join("test-download.txt");
    let result = cli.run_s3(&[
        "cp",
        &format!("s3://{}/test.txt", bucket_name),
        download_file.to_str().unwrap(),
    ]);
    assert!(
        result.is_success(),
        "Failed to download object: {}",
        result.stderr
    );

    // Verify content
    let content = std::fs::read_to_string(&download_file).unwrap();
    assert_eq!(content, "Hello from AWS CLI test!");

    // Cleanup
    std::fs::remove_file(temp_file).ok();
    std::fs::remove_file(download_file).ok();
    cli.run(&[
        "delete-object",
        "--bucket",
        &bucket_name,
        "--key",
        "test.txt",
    ]);
    cli.run(&["delete-bucket", "--bucket", &bucket_name]);
}

#[test]
fn test_cli_no_such_key_error() {
    if !check_aws_cli_installed() {
        eprintln!("Skipping: AWS CLI not installed");
        return;
    }

    let cli = AwsCli::new();
    let bucket_name = format!(
        "cli-nokey-{}",
        uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
    );

    // Create bucket
    let result = cli.run(&["create-bucket", "--bucket", &bucket_name]);
    assert!(
        result.is_success(),
        "Failed to create bucket: {}",
        result.stderr
    );

    // Try to get non-existent object
    let result = cli.run(&[
        "get-object",
        "--bucket",
        &bucket_name,
        "--key",
        "nonexistent.txt",
        "/tmp/output.txt",
    ]);
    assert!(
        !result.is_success(),
        "Get of non-existent object should fail"
    );

    // Check error message
    assert!(
        result.contains_error("NoSuchKey")
            || result.stderr.contains("Not Found")
            || result.stderr.contains("does not exist"),
        "Error should indicate NoSuchKey. stderr: {}",
        result.stderr
    );

    // Cleanup
    cli.run(&["delete-bucket", "--bucket", &bucket_name]);
}

#[test]
fn test_cli_bucket_not_empty_error() {
    if !check_aws_cli_installed() {
        eprintln!("Skipping: AWS CLI not installed");
        return;
    }

    let cli = AwsCli::new();
    let bucket_name = format!(
        "cli-notempty-{}",
        uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
    );

    // Create bucket
    let result = cli.run(&["create-bucket", "--bucket", &bucket_name]);
    assert!(
        result.is_success(),
        "Failed to create bucket: {}",
        result.stderr
    );

    // Put an object
    let temp_file = std::env::temp_dir().join("test-file.txt");
    std::fs::write(&temp_file, b"test content").unwrap();

    let result = cli.run(&[
        "put-object",
        "--bucket",
        &bucket_name,
        "--key",
        "test.txt",
        "--body",
        temp_file.to_str().unwrap(),
    ]);
    assert!(
        result.is_success(),
        "Failed to put object: {}",
        result.stderr
    );

    // Try to delete bucket (should fail - not empty)
    let result = cli.run(&["delete-bucket", "--bucket", &bucket_name]);
    assert!(
        !result.is_success(),
        "Delete of non-empty bucket should fail"
    );

    // Check error message
    assert!(
        result.contains_error("BucketNotEmpty") || result.stderr.contains("not empty"),
        "Error should indicate BucketNotEmpty. stderr: {}",
        result.stderr
    );

    // Cleanup
    std::fs::remove_file(temp_file).ok();
    cli.run(&[
        "delete-object",
        "--bucket",
        &bucket_name,
        "--key",
        "test.txt",
    ]);
    cli.run(&["delete-bucket", "--bucket", &bucket_name]);
}

#[test]
fn test_cli_list_objects_v2() {
    if !check_aws_cli_installed() {
        eprintln!("Skipping: AWS CLI not installed");
        return;
    }

    let cli = AwsCli::new();
    let bucket_name = format!(
        "cli-list-{}",
        uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
    );

    // Create bucket
    let result = cli.run(&["create-bucket", "--bucket", &bucket_name]);
    assert!(
        result.is_success(),
        "Failed to create bucket: {}",
        result.stderr
    );

    // Put multiple objects
    let temp_file = std::env::temp_dir().join("test-file.txt");
    std::fs::write(&temp_file, b"test content").unwrap();

    for i in 1..=3 {
        let result = cli.run(&[
            "put-object",
            "--bucket",
            &bucket_name,
            "--key",
            &format!("file-{}.txt", i),
            "--body",
            temp_file.to_str().unwrap(),
        ]);
        assert!(
            result.is_success(),
            "Failed to put object {}: {}",
            i,
            result.stderr
        );
    }

    // List objects v2
    let result = cli.run(&["list-objects-v2", "--bucket", &bucket_name]);
    assert!(
        result.is_success(),
        "Failed to list objects: {}",
        result.stderr
    );

    let json = result.parse_json().expect("Failed to parse JSON");
    let contents = json["Contents"].as_array().expect("No Contents array");
    assert_eq!(contents.len(), 3, "Should have 3 objects");

    // Verify object keys
    let keys: Vec<String> = contents
        .iter()
        .map(|obj| obj["Key"].as_str().unwrap().to_string())
        .collect();
    assert!(keys.contains(&"file-1.txt".to_string()));
    assert!(keys.contains(&"file-2.txt".to_string()));
    assert!(keys.contains(&"file-3.txt".to_string()));

    // Cleanup
    std::fs::remove_file(temp_file).ok();
    for i in 1..=3 {
        cli.run(&[
            "delete-object",
            "--bucket",
            &bucket_name,
            "--key",
            &format!("file-{}.txt", i),
        ]);
    }
    cli.run(&["delete-bucket", "--bucket", &bucket_name]);
}

#[test]
fn test_cli_head_object() {
    if !check_aws_cli_installed() {
        eprintln!("Skipping: AWS CLI not installed");
        return;
    }

    let cli = AwsCli::new();
    let bucket_name = format!(
        "cli-head-{}",
        uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
    );

    // Create bucket
    let result = cli.run(&["create-bucket", "--bucket", &bucket_name]);
    assert!(
        result.is_success(),
        "Failed to create bucket: {}",
        result.stderr
    );

    // Put object
    let temp_file = std::env::temp_dir().join("test-file.txt");
    let content = b"Hello, World!";
    std::fs::write(&temp_file, content).unwrap();

    let result = cli.run(&[
        "put-object",
        "--bucket",
        &bucket_name,
        "--key",
        "test.txt",
        "--body",
        temp_file.to_str().unwrap(),
    ]);
    assert!(
        result.is_success(),
        "Failed to put object: {}",
        result.stderr
    );

    // Head object
    let result = cli.run(&["head-object", "--bucket", &bucket_name, "--key", "test.txt"]);
    assert!(
        result.is_success(),
        "Failed to head object: {}",
        result.stderr
    );

    let json = result.parse_json().expect("Failed to parse JSON");
    assert_eq!(
        json["ContentLength"].as_i64(),
        Some(content.len() as i64),
        "Content length mismatch"
    );
    assert!(json["LastModified"].is_string(), "Should have LastModified");

    // Cleanup
    std::fs::remove_file(temp_file).ok();
    cli.run(&[
        "delete-object",
        "--bucket",
        &bucket_name,
        "--key",
        "test.txt",
    ]);
    cli.run(&["delete-bucket", "--bucket", &bucket_name]);
}

#[test]
fn test_cli_invalid_bucket_name() {
    if !check_aws_cli_installed() {
        eprintln!("Skipping: AWS CLI not installed");
        return;
    }

    let cli = AwsCli::new();

    // Try to create bucket with invalid name (uppercase letters are not allowed)
    let result = cli.run(&["create-bucket", "--bucket", "INVALID-BUCKET-NAME"]);
    assert!(
        !result.is_success(),
        "Create with invalid bucket name should fail"
    );

    // The error might be client-side validation or server-side
    // Just verify it failed appropriately
    assert!(
        result.stderr.contains("Invalid")
            || result.stderr.contains("invalid")
            || result.stderr.contains("validation"),
        "Error should indicate invalid bucket name. stderr: {}",
        result.stderr
    );
}
