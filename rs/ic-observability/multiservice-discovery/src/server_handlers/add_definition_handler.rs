use base64::{engine::general_purpose as b64, Engine as _};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use ic_crypto_utils_threshold_sig_der::parse_threshold_sig_key_from_der;
use service_discovery::registry_sync::nns_reachable;
use slog::Logger;
use tokio::sync::Mutex;
use warp::Reply;

use crate::definition::{wrap, Definition};
use crate::server_handlers::dto::DefinitionDto;
use crate::server_handlers::WebResult;

pub struct AddDefinitionBinding {
    pub definitions: Arc<Mutex<Vec<Definition>>>,
    pub log: Logger,
    pub registry_path: PathBuf,
    pub poll_interval: Duration,
    pub registry_query_timeout: Duration,
    pub rt: tokio::runtime::Handle,
    pub handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

pub async fn add_definition(definition: DefinitionDto, binding: AddDefinitionBinding) -> WebResult<impl Reply> {
    let public_key = match definition.public_key {
        Some(pk) => {
            let decoded = b64::STANDARD.decode(pk).unwrap();

            match parse_threshold_sig_key_from_der(&decoded) {
                Ok(key) => Some(key),
                Err(e) => {
                    return Ok(warp::reply::with_status(
                        e.to_string(),
                        warp::http::StatusCode::BAD_REQUEST,
                    ))
                }
            }
        }
        None => None,
    };

    let mut definitions = binding.definitions.lock().await;

    if definitions.iter().any(|d| d.name == definition.name) {
        return Ok(warp::reply::with_status(
            "Definition with this name already exists".to_string(),
            warp::http::StatusCode::BAD_REQUEST,
        ));
    }

    if !nns_reachable(definition.nns_urls.clone()).await {
        return Ok(warp::reply::with_status(
            "Couldn't ping nns of that definition".to_string(),
            warp::http::StatusCode::BAD_REQUEST,
        ));
    }

    let (stop_signal_sender, stop_signal_rcv) = crossbeam::channel::bounded::<()>(0);
    let definition = Definition::new(
        definition.nns_urls,
        binding.registry_path.clone(),
        definition.name.clone(),
        binding.log,
        public_key,
        binding.poll_interval,
        stop_signal_rcv,
        binding.registry_query_timeout,
        stop_signal_sender,
    );

    definitions.push(definition.clone());

    let ic_handle = std::thread::spawn(wrap(definition, binding.rt));
    let mut handles = binding.handles.lock().await;
    handles.push(ic_handle);

    Ok(warp::reply::with_status(
        "success".to_string(),
        warp::http::StatusCode::OK,
    ))
}
