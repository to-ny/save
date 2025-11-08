use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

mod common;

#[tokio::test]
async fn test_health_endpoint_with_uptime() {
    let (state, _temp_dir) = common::test_setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert!(json["timestamp"].is_string());
    assert!(json["uptime_seconds"].is_number());
}

#[tokio::test]
async fn test_metrics_endpoint_format() {
    let (state, _temp_dir) = common::test_setup().await;
    let app = save_api::app(state);

    let put_request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/test.txt")
        .header("Authorization", "AWS4-HMAC-SHA256 Credential=saveadmin/20240101/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=test")
        .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
        .header("x-amz-date", "20240101T000000Z")
        .body(Body::from("test data"))
        .unwrap();
    let _ = app.clone().oneshot(put_request).await.unwrap();

    let request = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let metrics_text = String::from_utf8(body.to_vec()).unwrap();

    assert!(metrics_text.contains("save_http_requests_total"));
    assert!(metrics_text.contains("save_http_request_duration_seconds"));
    assert!(metrics_text.contains("save_object_size_bytes"));
}

#[tokio::test]
async fn test_metrics_track_put_operation() {
    let (state, _temp_dir) = common::test_setup().await;
    let app = save_api::app(state.clone());

    let metrics_request = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let metrics_response = app.clone().oneshot(metrics_request).await.unwrap();
    let metrics_body = metrics_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let initial_metrics = String::from_utf8(metrics_body.to_vec()).unwrap();

    let put_request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-object.txt")
        .header("Authorization", "AWS4-HMAC-SHA256 Credential=saveadmin/20240101/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=test")
        .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
        .header("x-amz-date", "20240101T000000Z")
        .body(Body::from("Hello, Metrics!"))
        .unwrap();

    let put_response = app.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);

    let metrics_request = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let metrics_response = app.oneshot(metrics_request).await.unwrap();
    let metrics_body = metrics_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let updated_metrics = String::from_utf8(metrics_body.to_vec()).unwrap();

    assert!(updated_metrics.contains("save_http_requests_total"));
    assert!(updated_metrics.contains("save_http_request_duration_seconds"));
    assert!(updated_metrics.contains("save_object_size_bytes"));
    assert!(updated_metrics.len() > initial_metrics.len());
}

#[tokio::test]
async fn test_metrics_track_multipart_upload() {
    let (state, _temp_dir) = common::test_setup().await;
    let app = save_api::app(state.clone());

    let initiate_request = Request::builder()
        .method("POST")
        .uri("/test-bucket/multipart-test.txt?uploads")
        .header("Authorization", "AWS4-HMAC-SHA256 Credential=saveadmin/20240101/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=test")
        .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
        .header("x-amz-date", "20240101T000000Z")
        .body(Body::empty())
        .unwrap();

    let initiate_response = app.clone().oneshot(initiate_request).await.unwrap();
    assert_eq!(initiate_response.status(), StatusCode::OK);

    let body = initiate_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let upload_id = json["upload_id"].as_str().unwrap();

    let metrics_request = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let metrics_response = app.clone().oneshot(metrics_request).await.unwrap();
    let metrics_body = metrics_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let metrics_text = String::from_utf8(metrics_body.to_vec()).unwrap();

    assert!(metrics_text.contains("save_multipart_uploads_in_progress"));

    let part_request = Request::builder()
        .method("PUT")
        .uri(&format!(
            "/test-bucket/multipart-test.txt?partNumber=1&uploadId={}",
            upload_id
        ))
        .header("Authorization", "AWS4-HMAC-SHA256 Credential=saveadmin/20240101/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=test")
        .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
        .header("x-amz-date", "20240101T000000Z")
        .body(Body::from("Part 1 data"))
        .unwrap();

    let part_response = app.clone().oneshot(part_request).await.unwrap();
    assert_eq!(part_response.status(), StatusCode::OK);

    let complete_request = Request::builder()
        .method("POST")
        .uri(&format!(
            "/test-bucket/multipart-test.txt?uploadId={}",
            upload_id
        ))
        .header("Authorization", "AWS4-HMAC-SHA256 Credential=saveadmin/20240101/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=test")
        .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
        .header("x-amz-date", "20240101T000000Z")
        .body(Body::empty())
        .unwrap();

    let complete_response = app.clone().oneshot(complete_request).await.unwrap();
    assert_eq!(complete_response.status(), StatusCode::OK);

    let metrics_request = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let metrics_response = app.oneshot(metrics_request).await.unwrap();
    let metrics_body = metrics_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let final_metrics = String::from_utf8(metrics_body.to_vec()).unwrap();

    assert!(final_metrics.contains("save_object_size_bytes"));
    assert!(final_metrics.contains("multipart_complete"));
}

#[tokio::test]
async fn test_metrics_endpoint_normalization() {
    let (state, _temp_dir) = common::test_setup().await;
    let app = save_api::app(state.clone());

    for object_name in &["obj1.txt", "obj2.txt", "folder/obj3.txt"] {
        let request = Request::builder()
            .method("PUT")
            .uri(&format!("/test-bucket/{}", object_name))
            .header("Authorization", "AWS4-HMAC-SHA256 Credential=saveadmin/20240101/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=test")
            .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
            .header("x-amz-date", "20240101T000000Z")
            .body(Body::from("test data"))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    let metrics_request = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let metrics_response = app.oneshot(metrics_request).await.unwrap();
    let metrics_body = metrics_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let metrics_text = String::from_utf8(metrics_body.to_vec()).unwrap();

    assert!(metrics_text.contains(r#"endpoint="/{bucket}/{key}""#));
    assert!(metrics_text.contains(r#"method="PUT""#));
}
