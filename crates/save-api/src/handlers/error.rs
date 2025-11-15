use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use save_common::S3Error;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Bucket not found: {0}")]
    BucketNotFound(String),

    #[error("Bucket already exists: {0}")]
    BucketAlreadyExists(String),

    #[error("Bucket not empty: {0}")]
    BucketNotEmpty(String),

    #[error("Object not found: {bucket}/{key}")]
    ObjectNotFound { bucket: String, key: String },

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Request time too skewed")]
    RequestTimeTooSkewed,

    #[error("Signature does not match")]
    SignatureDoesNotMatch,

    #[error("Invalid signature exception: {0}")]
    InvalidSignatureException(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl ApiError {
    pub fn internal(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        tracing::error!(error = %msg, "Internal error occurred");
        Self::Internal(msg)
    }

    fn to_s3_error(&self) -> S3Error {
        match self {
            ApiError::BucketNotFound(bucket) => S3Error::no_such_bucket(bucket),
            ApiError::BucketAlreadyExists(bucket) => S3Error::bucket_already_exists(bucket),
            ApiError::BucketNotEmpty(bucket) => S3Error::bucket_not_empty(bucket),
            ApiError::ObjectNotFound { bucket, key } => S3Error::no_such_key(bucket, key),
            ApiError::InvalidRequest(msg) => S3Error::invalid_request(msg),
            ApiError::Unauthorized => S3Error::access_denied("/"),
            ApiError::RequestTimeTooSkewed => S3Error::request_time_too_skewed(),
            ApiError::SignatureDoesNotMatch => S3Error::signature_does_not_match(),
            ApiError::InvalidSignatureException(msg) => S3Error::invalid_request(msg),
            ApiError::Internal(_) => {
                S3Error::internal_error("We encountered an internal error. Please try again.")
            }
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BucketNotFound(_) => StatusCode::NOT_FOUND,
            ApiError::BucketAlreadyExists(_) => StatusCode::CONFLICT,
            ApiError::BucketNotEmpty(_) => StatusCode::CONFLICT,
            ApiError::ObjectNotFound { .. } => StatusCode::NOT_FOUND,
            ApiError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized => StatusCode::FORBIDDEN,
            ApiError::RequestTimeTooSkewed => StatusCode::FORBIDDEN,
            ApiError::SignatureDoesNotMatch => StatusCode::FORBIDDEN,
            ApiError::InvalidSignatureException(_) => StatusCode::FORBIDDEN,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_type(&self) -> &'static str {
        match self {
            ApiError::BucketNotFound(_)
            | ApiError::BucketAlreadyExists(_)
            | ApiError::BucketNotEmpty(_)
            | ApiError::ObjectNotFound { .. } => "metadata",
            ApiError::Unauthorized
            | ApiError::RequestTimeTooSkewed
            | ApiError::SignatureDoesNotMatch
            | ApiError::InvalidSignatureException(_) => "auth",
            ApiError::InvalidRequest(_) => "validation",
            ApiError::Internal(_) => "internal",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let error_type = self.error_type();
        let s3_error = self.to_s3_error();
        let request_id = s3_error.request_id.clone();
        let xml_body = s3_error.to_xml();

        crate::metrics::errors_total()
            .with_label_values(&[error_type, "api"])
            .inc();

        let mut response = Response::new(xml_body.into());
        *response.status_mut() = status;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/xml"),
        );
        response.headers_mut().insert(
            header::HeaderName::from_static("x-amz-request-id"),
            header::HeaderValue::from_str(&request_id).unwrap(),
        );
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    async fn response_to_string(response: Response) -> String {
        let body = response.into_body();
        let bytes = body.collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn test_api_error_bucket_not_found() {
        let error = ApiError::BucketNotFound("test-bucket".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );

        let body = response_to_string(response).await;
        assert!(body.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(body.contains("<Code>NoSuchBucket</Code>"));
        assert!(body.contains("<Resource>/test-bucket</Resource>"));
        assert!(body.contains("<RequestId>"));
    }

    #[tokio::test]
    async fn test_api_error_bucket_already_exists() {
        let error = ApiError::BucketAlreadyExists("test-bucket".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );

        let body = response_to_string(response).await;
        assert!(body.contains("<Code>BucketAlreadyExists</Code>"));
        assert!(body.contains("<Resource>/test-bucket</Resource>"));
    }

    #[tokio::test]
    async fn test_api_error_bucket_not_empty() {
        let error = ApiError::BucketNotEmpty("test-bucket".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );

        let body = response_to_string(response).await;
        assert!(body.contains("<Code>BucketNotEmpty</Code>"));
        assert!(body.contains("<Resource>/test-bucket</Resource>"));
    }

    #[tokio::test]
    async fn test_api_error_object_not_found() {
        let error = ApiError::ObjectNotFound {
            bucket: "test-bucket".to_string(),
            key: "test-key.txt".to_string(),
        };
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );

        let body = response_to_string(response).await;
        assert!(body.contains("<Code>NoSuchKey</Code>"));
        assert!(body.contains("<Resource>/test-bucket/test-key.txt</Resource>"));
    }

    #[tokio::test]
    async fn test_api_error_invalid_request() {
        let error = ApiError::InvalidRequest("Invalid bucket name".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );

        let body = response_to_string(response).await;
        assert!(body.contains("<Code>InvalidRequest</Code>"));
        assert!(body.contains("<Message>Invalid bucket name</Message>"));
    }

    #[tokio::test]
    async fn test_api_error_unauthorized() {
        let error = ApiError::Unauthorized;
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );

        let body = response_to_string(response).await;
        assert!(body.contains("<Code>AccessDenied</Code>"));
    }

    #[tokio::test]
    async fn test_api_error_internal() {
        let error = ApiError::internal("something went wrong".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml"
        );

        let body = response_to_string(response).await;
        assert!(body.contains("<Code>InternalError</Code>"));
        assert!(
            body.contains("<Message>We encountered an internal error. Please try again.</Message>")
        );
    }
}
