use anyhow::Result;
use serde_bser::value::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use uuid::Uuid;
use watchman_client::expr::Expr;

use crate::debug_log;
use crate::evaluator::CompiledExpr;
use crate::snapshot_manager::SnapshotManager;
use crate::watchman_protocol::*;
const VERSION: &str = "btrfs-watchman-0.1.0";

#[derive(Clone)]
pub struct WatchState {
    pub watch_root: PathBuf,
    pub relative_path: Option<PathBuf>,
}

pub struct ServerState {
    pub manager: SnapshotManager,
    // Maps watch-project path -> (watch root, relative path)
    pub watches: Mutex<HashMap<PathBuf, WatchState>>,
    // We could store known snapshots, or just parse them from the clock.
    // Clock format: "btrfs:<snap_uuid>"
}

#[derive(serde::Deserialize, Default)]
struct WatchmanConfig {
    #[serde(default)]
    ignore_dirs: Vec<String>,
}

fn load_watchman_config(watch_root: &std::path::Path) -> WatchmanConfig {
    let config_path = watch_root.join(".watchmanconfig");
    if let Ok(content) = std::fs::read_to_string(config_path) {
        if let Ok(config) = serde_json::from_str(&content) {
            return config;
        }
    }
    WatchmanConfig::default()
}

impl ServerState {
    pub fn new() -> Result<Self> {
        Ok(Self {
            manager: SnapshotManager::new()?,
            watches: Mutex::new(HashMap::new()),
        })
    }
}

pub async fn handle_client(
    mut socket: tokio::net::UnixStream,
    state: Arc<ServerState>,
) -> Result<()> {
    let mut buf = [0u8; 8192]; // Allow slightly larger buffer just in case
    loop {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }

        let request: Value = serde_bser::from_slice(&buf[..n])?;

        if let Value::Array(ref arr) = request {
            if let Some(Value::Utf8String(cmd_name)) = arr.get(0) {
                #[allow(unused_variables)]
                let t_start_cmd = std::time::Instant::now();
                debug_log!(">>xx> Received request: {:?}", arr);
                let mut handler = ResponseHandler { state: &state };
                let response = handler.handle(cmd_name, arr).await.unwrap_or_else(|e| {
                    debug_log!("Sending error response: {:?}", e);
                    pack(ErrorResponse {
                        version: VERSION.to_string(),
                        error: format!("{}", e),
                    })
                });
                socket.write_all(&response).await?;
            }
        }
    }
}

struct ResponseHandler<'a> {
    state: &'a ServerState,
}

impl<'a> ResponseHandler<'a> {
    async fn handle(&mut self, cmd_name: &str, arr: &[Value]) -> Result<Vec<u8>> {
        let response: Vec<u8> = match cmd_name {
            "version" => pack(GenericResponse {
                version: VERSION.to_string(),
            }),
            "watch-project" => self.watch_project(arr).await,
            "query" => self.query(arr)?,
            "trigger-list" => pack(TriggerListResponse {
                version: VERSION.to_string(),
                triggers: vec![],
            }),
            "trigger-del" => {
                let trigger_name = if let Some(Value::Utf8String(t)) = arr.get(2) {
                    t.clone()
                } else {
                    "unknown".to_string()
                };
                pack(TriggerDelResponse {
                    version: VERSION.to_string(),
                    deleted: true, // Mock success
                    trigger: trigger_name,
                })
            }
            "trigger" => {
                return Err(anyhow::anyhow!(
                    "trigger registration is not implemented by btrfs-watchman. Set fsmonitor.watchman.register-snapshot-trigger = false in your jj config."
                ));
            }
            _ => pack(GenericResponse {
                version: VERSION.to_string(),
            }),
        };
        Ok(response)
    }

    async fn watch_project(&self, arr: &[Value]) -> Vec<u8> {
        let t_start_cmd = std::time::Instant::now();
        let watch_path = if let Some(Value::Utf8String(r)) = arr.get(1) {
            PathBuf::from(r)
        } else {
            PathBuf::from(".")
        };

        let watch_path = std::fs::canonicalize(&watch_path).unwrap_or(watch_path);
        debug_log!("Received watch-project requested path: {:?}", watch_path);

        let WatchState {
            watch_root,
            relative_path,
        } = {
            let watches_lock = self.state.watches.lock().await;
            if let Some(cached) = watches_lock.get(&watch_path) {
                cached.clone()
            } else {
                drop(watches_lock);
                let watch_root = match self.state.manager.get_subvolume_root(&watch_path) {
                    Ok(root) => root,
                    Err(e) => {
                        return pack(ErrorResponse {
                            version: VERSION.to_string(),
                            error: format!("{}", e),
                        });
                    }
                };

                let relative_path = watch_path
                    .strip_prefix(&watch_root)
                    .ok()
                    .map(|p| p.to_path_buf());

                let mut watches_lock_mut = self.state.watches.lock().await;
                let watch_state = WatchState {
                    watch_root: watch_root.clone(),
                    relative_path: relative_path.clone(),
                };
                watches_lock_mut.insert(watch_path.clone(), watch_state.clone());
                watch_state
            }
        };

        let response = WatchProjectResponse {
            version: VERSION.to_string(),
            watch: watch_root,
            watcher: "btrfs".to_string(),
            relative_path,
        };
        debug_log!("watch-project took: {:?}", t_start_cmd.elapsed());
        pack(response)
    }

    fn query(&self, arr: &[Value]) -> Result<Vec<u8>> {
        #[allow(unused_variables)]
        let t_start_query = std::time::Instant::now();
        if let Some(Value::Utf8String(watch_root_str)) = arr.get(1) {
            let watch_root = PathBuf::from(watch_root_str);
            let query_args = arr.get(2);
            let mut since_clock = None;
            let mut relative_root = None;
            let mut expression = None;

            if let Some(Value::Object(opts)) = query_args {
                if let Some(Value::Utf8String(since)) = opts.get("since") {
                    since_clock = Some(since.clone());
                }
                if let Some(Value::Utf8String(rr)) = opts.get("relative_root") {
                    relative_root = Some(rr.clone());
                }
                expression = opts.get("expression").cloned();
            }
            let compiled_expr = expression.as_ref().map(|e| crate::evaluator::parse_expr(e));
            debug_log!(
                "Received query for {:?} with since_clock: {:?}",
                watch_root,
                since_clock
            );

            let watchman_config = load_watchman_config(&watch_root);

            // Generate new snap ID
            #[allow(unused_variables)]
            let t_start_snap = std::time::Instant::now();
            let new_snap_id = format!("snap_{}", Uuid::new_v4().simple());
            let new_snap_path = match self
                .state
                .manager
                .create_snapshot(&watch_root, &new_snap_id)
            {
                Ok(p) => p,
                Err(e) => {
                    debug_log!("Failed to create snapshot: {}", e);
                    todo!()
                }
            };
            debug_log!("Snapshot creation took: {:?}", t_start_snap.elapsed());
            let clock = format!("btrfs:{}", new_snap_id);

            // Compute files changed
            let mut files = Vec::new();

            let mut is_fresh_instance = since_clock.is_none();

            let diff_success = (|| -> Option<()> {
                let old_clock = since_clock.as_ref()?;
                let old_snap_id = old_clock.strip_prefix("btrfs:")?;

                if !old_snap_id.starts_with("snap_") {
                    return None;
                }

                // Sanity check to prevent path traversal
                if old_snap_id.contains('/') || old_snap_id.contains('\\') {
                    return None;
                }

                let old_snap_path = self
                    .state
                    .manager
                    .ensure_snapshot_dir(&watch_root)
                    .ok()?
                    .join(old_snap_id);
                if !old_snap_path.exists() {
                    return None; // Missing snapshot, will trigger fresh instance
                }

                #[allow(unused_variables)]
                let t_start_diff = std::time::Instant::now();
                files = self
                    .diff_paths(
                        &watchman_config,
                        relative_root,
                        compiled_expr,
                        &old_snap_path,
                        &new_snap_path,
                    )
                    .ok()?
                    .iter()
                    .map(|f| Value::Utf8String(f.clone()))
                    .collect();

                // Cleanup old snap
                let manager = self.state.manager.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = manager.delete_snapshot(&old_snap_path) {
                        debug_log!("Failed to delete old snap: {}", e);
                    }
                });

                Some(())
            })();

            if since_clock.is_some() && diff_success.is_none() {
                is_fresh_instance = true;
            }

            if is_fresh_instance {
                debug_log!("Fresh instance query responding with root directory.");
            }

            let response = QueryResultResponse {
                version: VERSION.to_string(),
                is_fresh_instance,
                files: Some(files),
                clock,
            };
            debug_log!("Total query took: {:?}", t_start_query.elapsed());
            Ok(pack(response))
        } else {
            // Missing watch root in query
            Ok(pack(GenericResponse {
                version: VERSION.to_string(),
            }))
        }
    }
    fn diff_paths(
        &self,
        watchman_config: &WatchmanConfig,
        relative_root: Option<String>,
        compiled_expr: Option<CompiledExpr>,
        old_snap_path: &PathBuf,
        new_snap_path: &PathBuf,
    ) -> Result<Vec<String>> {
        let mut files = Vec::new();
        debug_log!(
            "Diffing snapshots: {:?}, {:?}",
            old_snap_path,
            new_snap_path
        );
        let t_start_diff = std::time::Instant::now();
        match self
            .state
            .manager
            .diff_snapshots(&old_snap_path, &new_snap_path)
        {
            Ok(diff_files) => {
                debug_log!(
                    "Diff took: {:?} for {} changed files.",
                    t_start_diff.elapsed(),
                    diff_files.len()
                );
                for file in diff_files {
                    let mut file_to_report = file.clone();

                    let mut ignored = false;
                    for ignored_dir in &watchman_config.ignore_dirs {
                        if file == *ignored_dir || file.starts_with(&format!("{}/", ignored_dir)) {
                            ignored = true;
                            break;
                        }
                    }
                    if ignored {
                        continue;
                    }

                    if let Some(ref rr) = relative_root {
                        let prefix = format!("{}/", rr);
                        if file.starts_with(&prefix) {
                            file_to_report = file[prefix.len()..].to_string();
                        } else if file == *rr {
                            continue;
                        } else {
                            continue;
                        }
                    }

                    if let Some(expr) = &compiled_expr {
                        if !expr.evaluate(&file_to_report) {
                            continue;
                        }
                    }

                    files.push(file_to_report);
                }
                debug_log!(
                    "Returning files: {:?}...",
                    &files[0..std::cmp::min(10, files.len())]
                );
            }
            Err(e) => return Err(e),
        }
        Ok(files)
    }
}

fn pack<T: serde::Serialize>(response: T) -> Vec<u8> {
    let mut data = Vec::new();
    serde_bser::ser::serialize(&mut data, &response).unwrap();
    data
}
