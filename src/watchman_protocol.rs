use serde::Serialize;
use serde_bser::value::Value;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct ErrorResponse {
    pub version: String,
    pub error: String,
}

#[derive(Serialize)]
pub struct GetSockNameResponse {
    pub version: String,
    pub sockname: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct WatchProjectResponse {
    pub version: String,
    pub watch: PathBuf,
    pub watcher: String,
    pub relative_path: Option<PathBuf>,
}

#[derive(Serialize)]
pub struct QueryResultResponse {
    pub version: String,
    pub is_fresh_instance: bool,
    pub files: Option<Vec<Value>>,
    pub clock: String,
}

#[derive(Serialize)]
pub struct GenericResponse {
    pub version: String,
}

#[derive(Serialize)]
pub struct TriggerListResponse {
    pub version: String,
    pub triggers: Vec<Value>, // Just empty for now
}

#[derive(Serialize)]
pub struct TriggerDelResponse {
    pub version: String,
    pub deleted: bool,
    pub trigger: String,
}
