use axum::{
    http::StatusCode,
    response::{Html, IntoResponse},
};
use lambda_http::tracing;
use std::fs;

use crate::{
    constants::{LANDING_PAGE, LOGO, S3_BUCKET},
    modules::helper_functions,
};

pub async fn landing_page() -> impl IntoResponse {
    if fs::metadata(LANDING_PAGE).is_ok() && fs::metadata(LOGO).is_ok() {
        let html = fs::read_to_string(LANDING_PAGE).unwrap();
        tracing::info!("Found landing page and logo locally.");
        return Ok(Html(html).into_response());
    } else {
        tracing::info!("Landing page or logo not found locally. Fetching from S3...");
        let client = helper_functions::get_client().await;
        match helper_functions::download_object(&client, S3_BUCKET, LANDING_PAGE).await {
            Ok(output) => {
                let body = output.body.collect().await.unwrap();
                let bytes = body.into_bytes();

                if let Err(e) = fs::write(LANDING_PAGE, &bytes) {
                    tracing::error!("Failed to write landing page to disk: {}", e);
                } else {
                    if let Err(e) =
                        helper_functions::download_object(&client, S3_BUCKET, LOGO).await
                    {
                        tracing::error!("Failed to save logo from S3: {}", e);
                    } else {
                        tracing::info!("Landing page and logo saved locally.");
                    }
                }

                let html = String::from_utf8(bytes.to_vec()).unwrap();
                Ok(Html(html).into_response())
            }
            Err(e) => {
                tracing::error!(%e, "Failed to download landing page from S3");
                Err(StatusCode::NOT_FOUND)
            }
        }
    }
}
