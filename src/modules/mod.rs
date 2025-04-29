pub mod helper_functions;
pub mod routes;

#[allow(unused_imports)]
use crate::constants::{DYNAMO_TABLE_NAME, GOOGLE_API_KEY, LANDING_PAGE, LOGO, S3_BUCKET};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct MyForm {
    pub address: String,
}

#[derive(Serialize)]
pub struct Center {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Serialize)]
pub struct GeocodeResponse {
    pub center: Center,
    pub areas: Vec<serde_json::Value>,
}
