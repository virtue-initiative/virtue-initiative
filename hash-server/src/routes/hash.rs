use std::collections::HashMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::error::ApiError;
use crate::state::{self, DeviceState};

fn validate_device_id(id: &str) -> Result<(), ApiError> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| ApiError::InvalidQuery(Some(format!("'{id}' is not a valid device id"))))
}

#[derive(Serialize)]
pub struct DeviceInfo {
    hash: String,
    seq: u32,
    last_received: u32,
}

impl From<DeviceState> for DeviceInfo {
    fn from(state: DeviceState) -> Self {
        DeviceInfo {
            hash: hex::encode(state.hash),
            seq: state.seq,
            last_received: state.last_received,
        }
    }
}

/// `POST /hash` — see SPEC.md section 2.1.
pub async fn ingest(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<StatusCode, ApiError> {
    let claims = app.jwt.require(&headers, "device")?;

    if body.len() != 40 {
        return Err(ApiError::InvalidBody(Some(format!(
            "expected a 40 byte body, got {} bytes",
            body.len()
        ))));
    }

    let unix_time = u32::from_le_bytes(body[0..4].try_into().unwrap());
    let seq = u32::from_le_bytes(body[4..8].try_into().unwrap());
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&body[8..40]);

    app.writer.ingest(claims.sub, unix_time, seq, hash).await?;

    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
pub struct DevicesQuery {
    devices: Option<String>,
}

/// `GET /hash?devices=[device_ids]` — see SPEC.md section 2.2.
pub async fn get_many(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DevicesQuery>,
) -> Result<Json<HashMap<String, DeviceInfo>>, ApiError> {
    app.jwt.require(&headers, "server")?;

    let raw = query.devices.filter(|s| !s.is_empty()).ok_or_else(|| {
        ApiError::InvalidQuery(Some("'devices' query parameter is required".into()))
    })?;

    let device_ids: Vec<&str> = raw.split(',').collect();
    for id in &device_ids {
        validate_device_id(id)?;
    }

    let mut result = HashMap::with_capacity(device_ids.len());
    for id in device_ids {
        result.insert(id.to_string(), state::get(&app.devices, id).into());
    }

    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct DeviceQuery {
    device: Option<String>,
}

/// `DELETE /hash?device=device1` — see SPEC.md section 2.3.
pub async fn reset(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DeviceQuery>,
) -> Result<Json<DeviceInfo>, ApiError> {
    app.jwt.require(&headers, "server")?;

    let device_id = query.device.filter(|s| !s.is_empty()).ok_or_else(|| {
        ApiError::InvalidQuery(Some("'device' query parameter is required".into()))
    })?;
    validate_device_id(&device_id)?;

    let prior = app.writer.reset(device_id).await?;

    Ok(Json(prior.into()))
}
