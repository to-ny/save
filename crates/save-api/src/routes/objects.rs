use axum::{Router, response::IntoResponse, routing::get};

use crate::handlers::multipart::{abort_multipart, upload_part};
use crate::handlers::objects::{delete_object, get_object, head_object, put_object};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/{bucket}/{*key}",
        get(get_object)
            .put(put_or_upload_part)
            .delete(delete_or_abort)
            .head(head_object),
    )
}

async fn put_or_upload_part(
    state: axum::extract::State<AppState>,
    path: axum::extract::Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<
        crate::handlers::multipart::MultipartQueryParams,
    >,
    body: axum::body::Body,
) -> axum::response::Response {
    match (params.part_number, params.upload_id) {
        (Some(part_number), Some(upload_id)) => {
            let query = crate::handlers::multipart::UploadPartQuery {
                part_number,
                upload_id,
            };
            upload_part(state, path, axum::extract::Query(query), body)
                .await
                .into_response()
        }
        _ => put_object(state, path, body).await.into_response(),
    }
}

async fn delete_or_abort(
    state: axum::extract::State<AppState>,
    path: axum::extract::Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<
        crate::handlers::multipart::MultipartQueryParams,
    >,
) -> axum::response::Response {
    if let Some(upload_id) = params.upload_id {
        let query = crate::handlers::multipart::CompleteQuery { upload_id };
        abort_multipart(state, path, axum::extract::Query(query))
            .await
            .into_response()
    } else {
        delete_object(state, path).await.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use save_metadata::ObjectMetadata;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_route_put_object() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .method("PUT")
            .uri("/test-bucket/file.txt")
            .body(Body::from("test content"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_route_upload_part() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;

        state
            .metadata
            .initiate_multipart_upload("test-bucket", "file.txt", "test-upload-id", None)
            .await
            .unwrap();

        let parts_dir = std::path::PathBuf::from(&state.config.storage.data_path)
            .join("temp")
            .join("parts")
            .join("test-upload-id");
        tokio::fs::create_dir_all(&parts_dir).await.unwrap();

        let app = routes().with_state(state);

        let request = Request::builder()
            .method("PUT")
            .uri("/test-bucket/file.txt?partNumber=1&uploadId=test-upload-id")
            .body(Body::from("part content"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_route_put_malformed_part_query() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .method("PUT")
            .uri("/test-bucket/file.txt?partNumber=&uploadId=")
            .body(Body::from("content"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_route_delete_object() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;

        let content = b"test content";
        let mut reader = std::io::Cursor::new(content);
        state
            .storage
            .put_object("test-bucket/file.txt", &mut reader)
            .await
            .unwrap();

        let metadata = ObjectMetadata::new(
            "test-bucket".to_string(),
            "file.txt".to_string(),
            content.len() as u64,
            "etag".to_string(),
        );
        state.metadata.put_object_metadata(metadata).await.unwrap();

        let app = routes().with_state(state);

        let request = Request::builder()
            .method("DELETE")
            .uri("/test-bucket/file.txt")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_route_abort_multipart() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;

        state
            .metadata
            .initiate_multipart_upload("test-bucket", "file.txt", "test-upload-id", None)
            .await
            .unwrap();

        let app = routes().with_state(state);

        let request = Request::builder()
            .method("DELETE")
            .uri("/test-bucket/file.txt?uploadId=test-upload-id")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_route_delete_malformed_upload_id() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .method("DELETE")
            .uri("/test-bucket/file.txt?uploadId=invalid")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
