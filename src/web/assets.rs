use super::boundary::{esc, page};
use super::{WebState, error_page, not_found};
use axum::{
    Router,
    body::Body,
    extract::{Multipart, Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use tokio_util::io::ReaderStream;

use crate::{
    repository::NewJobAttachment,
    storage::{StorageError, validate_original_filename},
};

pub(super) fn upload_routes() -> Router<WebState> {
    Router::new()
        .route("/jobs/{id}/attachments", post(upload_attachment))
        .route("/settings/logo", post(upload_logo))
}

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route(
            "/jobs/{job_id}/attachments/{attachment_id}/download",
            get(download_attachment),
        )
        .route(
            "/jobs/{job_id}/attachments/{attachment_id}/delete",
            post(delete_attachment),
        )
        .route("/settings/logo/current", get(current_logo))
}

pub(super) fn upload_error(error: &StorageError) -> Response {
    let status = match error {
        StorageError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        StorageError::UnsafeFilename | StorageError::MissingFile | StorageError::InvalidLogo => {
            StatusCode::BAD_REQUEST
        }
        StorageError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        page(
            "Upload failed",
            &format!(
                "<h1>Upload failed</h1><p role=status>{}</p>",
                esc(&error.to_string())
            ),
        ),
    )
        .into_response()
}

pub(super) async fn upload_attachment(
    State(state): State<WebState>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Response {
    let job = match state.repository.job(&id).await {
        Ok(Some(job)) if job.completed_at.is_none() => job,
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                "Completed jobs cannot accept uploads.",
            )
                .into_response();
        }
        Ok(None) => return not_found(),
        Err(error) => return error_page(&error.to_string()),
    };
    let _ = job;
    loop {
        let mut field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => return upload_error(&StorageError::MissingFile),
            Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
        };
        if field.name() != Some("file") {
            continue;
        }
        let original_name = match field.file_name().map(str::to_owned) {
            Some(name) if validate_original_filename(&name).is_ok() => name,
            _ => return upload_error(&StorageError::UnsafeFilename),
        };
        let media_type = field
            .content_type()
            .filter(|value| value.len() <= 255)
            .unwrap_or("application/octet-stream")
            .to_owned();
        let mut staged = match state.storage.begin_attachment().await {
            Ok(staged) => staged,
            Err(error) => return upload_error(&error),
        };
        while let Some(chunk) = match field.chunk().await {
            Ok(chunk) => chunk,
            Err(error) => {
                state.storage.discard(staged).await;
                return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
            }
        } {
            if let Err(error) = staged.write(&chunk).await {
                state.storage.discard(staged).await;
                return upload_error(&error);
            }
        }
        let stored = match state.storage.commit_attachment(staged).await {
            Ok(stored) => stored,
            Err(error) => return upload_error(&error),
        };
        let Ok(byte_size) = i64::try_from(stored.byte_size) else {
            let _ = state.storage.remove_attachment(&stored.stored_name).await;
            return upload_error(&StorageError::TooLarge {
                limit: state.storage.max_attachment_bytes(),
            });
        };
        let metadata = NewJobAttachment {
            original_name: &original_name,
            stored_name: &stored.stored_name,
            media_type: &media_type,
            byte_size,
            sha256: &stored.sha256,
        };
        return match state.repository.create_attachment(&id, &metadata).await {
            Ok(Some(_)) => Redirect::to(&format!("/jobs/{id}?uploaded=1")).into_response(),
            Ok(None) => {
                let _ = state.storage.remove_attachment(&stored.stored_name).await;
                (StatusCode::BAD_REQUEST, "Invalid attachment metadata.").into_response()
            }
            Err(error) => {
                let _ = state.storage.remove_attachment(&stored.stored_name).await;
                error_page(&error.to_string())
            }
        };
    }
}

pub(super) async fn download_attachment(
    State(state): State<WebState>,
    Path((job_id, attachment_id)): Path<(String, String)>,
) -> Response {
    let attachment = match state.repository.attachment(&attachment_id).await {
        Ok(Some(attachment)) if attachment.job_id == job_id => attachment,
        Ok(_) => return not_found(),
        Err(error) => return error_page(&error.to_string()),
    };
    let path = match state.storage.attachment_path(&attachment.stored_name) {
        Ok(path) => path,
        Err(error) => return error_page(&error.to_string()),
    };
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return not_found(),
        Err(error) => return error_page(&error.to_string()),
    };
    let disposition = content_disposition(&attachment.original_name);
    let content_type = HeaderValue::from_str(&attachment.media_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let mut response = Body::from_stream(ReaderStream::new(file)).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    response
        .headers_mut()
        .insert(header::CONTENT_DISPOSITION, disposition);
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

pub(super) async fn delete_attachment(
    State(state): State<WebState>,
    Path((job_id, attachment_id)): Path<(String, String)>,
) -> Response {
    match state.repository.job(&job_id).await {
        Ok(Some(job)) if job.completed_at.is_none() => {}
        Ok(Some(_)) => {
            return (StatusCode::CONFLICT, "Completed jobs cannot delete files.").into_response();
        }
        Ok(None) => return not_found(),
        Err(error) => return error_page(&error.to_string()),
    }
    let attachment = match state.repository.attachment(&attachment_id).await {
        Ok(Some(attachment)) if attachment.job_id == job_id => attachment,
        Ok(_) => return not_found(),
        Err(error) => return error_page(&error.to_string()),
    };
    let removed = match state
        .storage
        .stage_attachment_removal(&attachment.stored_name)
        .await
    {
        Ok(removed) => removed,
        Err(error) => return error_page(&error.to_string()),
    };
    match state.repository.delete_attachment(&attachment_id).await {
        Ok(true) => {
            if let Err(error) = state.storage.finish_attachment_removal(removed).await {
                tracing::warn!(%error, attachment_id, "Deleted attachment metadata but could not remove staged bytes");
            }
            Redirect::to(&format!("/jobs/{job_id}?deleted=1")).into_response()
        }
        Ok(false) => {
            let _ = state.storage.restore_attachment(removed).await;
            not_found()
        }
        Err(error) => {
            let _ = state.storage.restore_attachment(removed).await;
            error_page(&error.to_string())
        }
    }
}

pub(super) async fn upload_logo(
    State(state): State<WebState>,
    mut multipart: Multipart,
) -> Response {
    loop {
        let mut field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => return upload_error(&StorageError::MissingFile),
            Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
        };
        if field.name() != Some("file") {
            continue;
        }
        if let Some(name) = field.file_name()
            && validate_original_filename(name).is_err()
        {
            return upload_error(&StorageError::UnsafeFilename);
        }
        let mut staged = match state.storage.begin_logo().await {
            Ok(staged) => staged,
            Err(error) => return upload_error(&error),
        };
        while let Some(chunk) = match field.chunk().await {
            Ok(chunk) => chunk,
            Err(error) => {
                state.storage.discard(staged).await;
                return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
            }
        } {
            if let Err(error) = staged.write(&chunk).await {
                state.storage.discard(staged).await;
                return upload_error(&error);
            }
        }
        let stored = match state.storage.commit_logo(staged).await {
            Ok(stored) => stored,
            Err(error) => return upload_error(&error),
        };
        if let Err(error) = state.storage.validate_logo(&stored).await {
            let _ = tokio::fs::remove_file(&stored.path).await;
            return upload_error(&error);
        }
        return match state.repository.save_logo_path(&stored.stored_name).await {
            Ok(()) => Redirect::to("/settings?saved=1").into_response(),
            Err(error) => {
                let _ = tokio::fs::remove_file(&stored.path).await;
                error_page(&error.to_string())
            }
        };
    }
}

pub(super) async fn current_logo(State(state): State<WebState>) -> Response {
    let settings = match state.repository.settings().await {
        Ok(settings) => settings,
        Err(error) => return error_page(&error.to_string()),
    };
    let Some(identifier) = settings.logo_path.filter(|path| !path.is_empty()) else {
        return not_found();
    };
    let Ok(path) = state.storage.managed_logo_path(&identifier) else {
        return not_found();
    };
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return not_found(),
        Err(error) => return error_page(&error.to_string()),
    };
    let content_type = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Png) => HeaderValue::from_static("image/png"),
        Ok(image::ImageFormat::Jpeg) => HeaderValue::from_static("image/jpeg"),
        _ => return not_found(),
    };
    let mut response = Body::from(bytes).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

pub(super) fn content_disposition(filename: &str) -> HeaderValue {
    let fallback: String = filename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | ' ') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let encoded = filename
        .as_bytes()
        .iter()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'-' | b'_') {
                char::from(*byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect::<String>();
    HeaderValue::from_str(&format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment; filename=\"download\""))
}
