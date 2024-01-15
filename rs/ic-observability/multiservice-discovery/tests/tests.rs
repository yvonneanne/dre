use std::collections::BTreeSet;
use std::process::{Command, Child};
use tempdir::TempDir;
use std::time::Duration;
use assert_cmd::cargo::CommandCargoExt;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use multiservice_discovery_shared::builders::prometheus_config_structure::PrometheusStaticConfig;

#[test]
fn mainnet_targets_tests() {

    let rt = Runtime::new().unwrap();
    let temp_dir = TempDir::new("target").expect("Failed to create temporary directory");
    let mut cmd = Command::cargo_bin("multiservice-discovery").expect("Failed to find binary");

    // Add the necessary arguments
    cmd.arg("--targets-dir").arg(temp_dir.path().to_path_buf().to_str().unwrap());

    let mut child_process: Child = cmd.spawn().expect("Failed to run command");

    let handle: JoinHandle<anyhow::Result<BTreeSet<PrometheusStaticConfig>>> = rt.spawn(async {
            let timeout_duration = Duration::from_secs(300);
            let start_time = std::time::Instant::now();

            loop {
                if start_time.elapsed() > timeout_duration {
                    return Err(anyhow::anyhow!("Timeout reached"));
                }
                sleep(Duration::from_secs(5)).await;

                let response = reqwest::get("http://localhost:8000/prom/targets").await?.text().await?;
                let deserialized: Result<BTreeSet<PrometheusStaticConfig>, serde_json::Error> = serde_json::from_str(&response);

                match deserialized {
                    Ok(mainnet_targets) => {
                        if !mainnet_targets.is_empty() {
                            return Ok(mainnet_targets);
                        }
                    }
                    Err(err) => {
                        return Err(anyhow::anyhow!("Failed to deserialize: {}", err));
                    }
                }
            }
    });

    let mainnet_targets = rt.block_on(handle).unwrap().unwrap();

    assert!(mainnet_targets.iter().count() >= 5895);

    child_process.kill().unwrap();
    child_process.wait_with_output().unwrap();
}