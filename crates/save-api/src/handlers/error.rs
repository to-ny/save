use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub enum ApiError {
    BucketNotFound(String),
    BucketAlreadyExists(String),
    BucketNotEmpty(String),
    ObjectNotFound(String, String),
    InvalidRequest(String),
    Unauthorized,
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BucketNotFound(bucket) => (
                StatusCode::NOT_FOUND,
                format!("Bucket not found: {}", bucket),
            ),
            ApiError::BucketAlreadyExists(bucket) => (
                StatusCode::CONFLICT,
                format!("Bucket already exists: {}", bucket),
            ),
            ApiError::BucketNotEmpty(bucket) => (
                StatusCode::CONFLICT,
                format!("Bucket not empty: {}", bucket),
            ),
            ApiError::ObjectNotFound(bucket, key) => (
                StatusCode::NOT_FOUND,
                format!("Object not found: {}/{}", bucket, key),
            ),
            ApiError::InvalidRequest(msg) => {
                (StatusCode::BAD_REQUEST, format!("Invalid request: {}", msg))
            }
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            ApiError::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Internal error: {}", msg),
            ),
        };

        (status, message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_error_bucket_not_found() {
        let error = ApiError::BucketNotFound("test-bucket".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_api_error_bucket_already_exists() {
        let error = ApiError::BucketAlreadyExists("test-bucket".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn test_api_error_bucket_not_empty() {
        let error = ApiError::BucketNotEmpty("test-bucket".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn test_api_error_object_not_found() {
        let error = ApiError::ObjectNotFound("test-bucket".to_string(), "test-key.txt".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_api_error_invalid_request() {
        let error = ApiError::InvalidRequest("Invalid bucket name".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_api_error_unauthorized() {
        let error = ApiError::Unauthorized;
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_api_error_internal() {
        let error = ApiError::Internal("something went wrong".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
