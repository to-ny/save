mod common;

use axum::{body::Body, http::StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

mod complete_flow {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn test_multipart_upload_complete_flow() {
        let (state, _temp_dir) = common::test_setup().await;
        let app = save_api::app(state);

        let init_request = Request::builder()
            .method("POST")
            .uri("/test-bucket/multipart-file.txt?uploads")
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let init_response = app.clone().oneshot(init_request).await.unwrap();
        assert_eq!(init_response.status(), StatusCode::OK);

        let init_body = init_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let init_json: Value = serde_json::from_slice(&init_body).unwrap();
        let upload_id = init_json["upload_id"].as_str().unwrap();

        let part1_content = b"First part of the file.";
        let part1_request = Request::builder()
            .method("PUT")
            .uri(format!(
                "/test-bucket/multipart-file.txt?partNumber=1&uploadId={}",
                upload_id
            ))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::from(part1_content.to_vec()))
            .unwrap();

        let part1_response = app.clone().oneshot(part1_request).await.unwrap();
        assert_eq!(part1_response.status(), StatusCode::OK);

        let part2_content = b" Second part of the file.";
        let part2_request = Request::builder()
            .method("PUT")
            .uri(format!(
                "/test-bucket/multipart-file.txt?partNumber=2&uploadId={}",
                upload_id
            ))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::from(part2_content.to_vec()))
            .unwrap();

        let part2_response = app.clone().oneshot(part2_request).await.unwrap();
        assert_eq!(part2_response.status(), StatusCode::OK);

        let complete_request = Request::builder()
            .method("POST")
            .uri(format!(
                "/test-bucket/multipart-file.txt?uploadId={}",
                upload_id
            ))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let complete_response = app.clone().oneshot(complete_request).await.unwrap();
        assert_eq!(complete_response.status(), StatusCode::OK);

        let complete_body = complete_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let complete_json: Value = serde_json::from_slice(&complete_body).unwrap();
        assert!(complete_json["etag"].is_string());

        let get_request = Request::builder()
            .method("GET")
            .uri("/test-bucket/multipart-file.txt")
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let get_response = app.oneshot(get_request).await.unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);

        let get_body = get_response.into_body().collect().await.unwrap().to_bytes();
        let expected_content = b"First part of the file. Second part of the file.";
        assert_eq!(get_body.as_ref(), expected_content);
    }

    #[tokio::test]
    async fn test_multipart_upload_abort() {
        let (state, _temp_dir) = common::test_setup().await;
        let app = save_api::app(state);

        let init_request = Request::builder()
            .method("POST")
            .uri("/test-bucket/abort-file.txt?uploads")
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let init_response = app.clone().oneshot(init_request).await.unwrap();
        assert_eq!(init_response.status(), StatusCode::OK);

        let init_body = init_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let init_json: Value = serde_json::from_slice(&init_body).unwrap();
        let upload_id = init_json["upload_id"].as_str().unwrap();

        let part_request = Request::builder()
            .method("PUT")
            .uri(format!(
                "/test-bucket/abort-file.txt?partNumber=1&uploadId={}",
                upload_id
            ))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::from("Some content"))
            .unwrap();

        let part_response = app.clone().oneshot(part_request).await.unwrap();
        assert_eq!(part_response.status(), StatusCode::OK);

        let abort_request = Request::builder()
            .method("DELETE")
            .uri(format!(
                "/test-bucket/abort-file.txt?uploadId={}",
                upload_id
            ))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let abort_response = app.clone().oneshot(abort_request).await.unwrap();
        assert_eq!(abort_response.status(), StatusCode::NO_CONTENT);

        let get_request = Request::builder()
            .method("GET")
            .uri("/test-bucket/abort-file.txt")
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let get_response = app.oneshot(get_request).await.unwrap();
        assert_eq!(get_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_multipart_invalid_upload_id() {
        let (state, _temp_dir) = common::test_setup().await;
        let app = save_api::app(state);

        let request = Request::builder()
            .method("PUT")
            .uri("/test-bucket/file.txt?partNumber=1&uploadId=invalid-id")
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::from("Content"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_multipart_bucket_not_found() {
        let (state, _temp_dir) = common::test_setup().await;
        let app = save_api::app(state);

        let request = Request::builder()
            .method("POST")
            .uri("/nonexistent-bucket/file.txt?uploads")
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_multipart_invalid_part_number() {
        let (state, _temp_dir) = common::test_setup().await;
        let app = save_api::app(state);

        let init_request = Request::builder()
            .method("POST")
            .uri("/test-bucket/file.txt?uploads")
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let init_response = app.clone().oneshot(init_request).await.unwrap();
        let init_body = init_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let init_json: Value = serde_json::from_slice(&init_body).unwrap();
        let upload_id = init_json["upload_id"].as_str().unwrap();

        let request = Request::builder()
            .method("PUT")
            .uri(format!(
                "/test-bucket/file.txt?partNumber=0&uploadId={}",
                upload_id
            ))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::from("Content"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_multipart_complete_without_parts() {
        let (state, _temp_dir) = common::test_setup().await;
        let app = save_api::app(state);

        let init_request = Request::builder()
            .method("POST")
            .uri("/test-bucket/file.txt?uploads")
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let init_response = app.clone().oneshot(init_request).await.unwrap();
        let init_body = init_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let init_json: Value = serde_json::from_slice(&init_body).unwrap();
        let upload_id = init_json["upload_id"].as_str().unwrap();

        let complete_request = Request::builder()
            .method("POST")
            .uri(format!(
                "/test-bucket/file.txt?uploadId={}",
                upload_id
            ))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(complete_request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_multipart_missing_auth() {
        let (state, _temp_dir) = common::test_setup().await;
        let app = save_api::app(state);

        let request = Request::builder()
            .method("POST")
            .uri("/test-bucket/file.txt?uploads")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_multipart_large_parts() {
        let (state, _temp_dir) = common::test_setup().await;
        let app = save_api::app(state);

        let init_request = Request::builder()
            .method("POST")
            .uri("/test-bucket/large-file.bin?uploads")
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let init_response = app.clone().oneshot(init_request).await.unwrap();
        let init_body = init_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let init_json: Value = serde_json::from_slice(&init_body).unwrap();
        let upload_id = init_json["upload_id"].as_str().unwrap();

        let part1_content = vec![0xAA; 50_000];
        let part1_request = Request::builder()
            .method("PUT")
            .uri(format!(
                "/test-bucket/large-file.bin?partNumber=1&uploadId={}",
                upload_id
            ))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::from(part1_content.clone()))
            .unwrap();

        app.clone().oneshot(part1_request).await.unwrap();

        let part2_content = vec![0xBB; 50_000];
        let part2_request = Request::builder()
            .method("PUT")
            .uri(format!(
                "/test-bucket/large-file.bin?partNumber=2&uploadId={}",
                upload_id
            ))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::from(part2_content.clone()))
            .unwrap();

        app.clone().oneshot(part2_request).await.unwrap();

        let complete_request = Request::builder()
            .method("POST")
            .uri(format!(
                "/test-bucket/large-file.bin?uploadId={}",
                upload_id
            ))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let complete_response = app.clone().oneshot(complete_request).await.unwrap();
        assert_eq!(complete_response.status(), StatusCode::OK);

        let get_request = Request::builder()
            .method("GET")
            .uri("/test-bucket/large-file.bin")
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let get_response = app.oneshot(get_request).await.unwrap();
        let get_body = get_response.into_body().collect().await.unwrap().to_bytes();

        assert_eq!(get_body.len(), 100_000);
        assert_eq!(&get_body[0..50_000], part1_content.as_slice());
        assert_eq!(&get_body[50_000..100_000], part2_content.as_slice());
    }
}

mod list {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn test_list_multipart_uploads_success() {
        let (state, _temp_dir) = common::setup().await;

        let upload1 = state
            .metadata
            .initiate_multipart_upload("test-bucket", "file1.txt", "upload-id-1", None)
            .await
            .unwrap();
        let upload2 = state
            .metadata
            .initiate_multipart_upload("test-bucket", "file2.txt", "upload-id-2", None)
            .await
            .unwrap();

        let app = save_api::app(state);
        let request = common::request_with_auth("GET", "/test-bucket?uploads", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["bucket"], "test-bucket");
        assert_eq!(json["uploads"].as_array().unwrap().len(), 2);

        let upload_ids: Vec<&str> = json["uploads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["upload_id"].as_str().unwrap())
            .collect();

        assert!(upload_ids.contains(&upload1.upload_id.as_str()));
        assert!(upload_ids.contains(&upload2.upload_id.as_str()));

        for upload in json["uploads"].as_array().unwrap() {
            assert!(upload["key"].is_string());
            assert!(upload["initiated"].is_string());
        }
    }

    #[tokio::test]
    async fn test_list_multipart_uploads_empty() {
        let (state, _temp_dir) = common::setup().await;
        let app = save_api::app(state);

        let request = common::request_with_auth("GET", "/test-bucket?uploads", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["bucket"], "test-bucket");
        assert_eq!(json["uploads"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_list_multipart_uploads_bucket_not_found() {
        let (state, _temp_dir) = common::setup_empty().await;
        let app = save_api::app(state);

        let request =
            common::request_with_auth("GET", "/nonexistent-bucket?uploads", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_multipart_uploads_invalid_bucket_name() {
        let (state, _temp_dir) = common::setup_empty().await;
        let app = save_api::app(state);

        let request = common::request_with_auth("GET", "/Invalid_Bucket?uploads", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_list_multipart_uploads_missing_auth() {
        let (state, _temp_dir) = common::setup().await;
        let app = save_api::app(state);

        let request = Request::builder()
            .method("GET")
            .uri("/test-bucket?uploads")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
