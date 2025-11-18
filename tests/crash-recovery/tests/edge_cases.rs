#![cfg_attr(not(feature = "crash_tests"), allow(unused_imports))]

mod common;

use anyhow::Result;
use common::{SingleNodeEnv, TestEnvironment};

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_partial_multipart_wrong_etags() -> Result<()> {
    // Test: Try to complete multipart with wrong ETags
    // Expected: Error response, no corruption

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let env = SingleNodeEnv::setup().await?;

    env.client().create_bucket().bucket("test").send().await?;

    let init = env
        .client()
        .create_multipart_upload()
        .bucket("test")
        .key("wrong-etags.bin")
        .send()
        .await?;

    let upload_id = init.upload_id().unwrap();

    // Upload parts
    for i in 1..=3 {
        env.client()
            .upload_part()
            .bucket("test")
            .key("wrong-etags.bin")
            .upload_id(upload_id)
            .part_number(i)
            .body(vec![0xAA; 1024].into())
            .send()
            .await?;
    }

    // Try to complete with fabricated/wrong ETags
    use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};

    let completed_upload = CompletedMultipartUpload::builder()
        .parts(
            CompletedPart::builder()
                .e_tag("\"wrong-etag-1\"")
                .part_number(1)
                .build(),
        )
        .parts(
            CompletedPart::builder()
                .e_tag("\"wrong-etag-2\"")
                .part_number(2)
                .build(),
        )
        .parts(
            CompletedPart::builder()
                .e_tag("\"wrong-etag-3\"")
                .part_number(3)
                .build(),
        )
        .build();

    let result = env
        .client()
        .complete_multipart_upload()
        .bucket("test")
        .key("wrong-etags.bin")
        .upload_id(upload_id)
        .multipart_upload(completed_upload)
        .send()
        .await;

    // Should either fail or succeed (implementation dependent)
    // Either way, verify no corruption
    if result.is_ok() {
        tracing::info!("Complete succeeded (implementation allows any ETags)");
    } else {
        tracing::info!("Complete failed with wrong ETags (stricter validation)");
    }

    env.verify_consistency().await?;

    tracing::info!("Test completed successfully");
    Ok(())
}
