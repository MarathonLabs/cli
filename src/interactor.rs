use anyhow::Result;
use globset::Glob;
use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
use std::{path::PathBuf, time::Duration};
use url::{Position, Url};

use console::style;
use log::debug;
use tokio::time::{sleep, Instant};

use crate::{
    api::{Artifact, RapiClient, RapiReqwestClient},
    artifacts::{download_artifacts, fetch_artifact_list},
    cli::Platform,
    filtering::model::SparseMarathonfile,
};

pub struct DownloadArtifactsInteractor {}

impl DownloadArtifactsInteractor {
    pub(crate) async fn execute(
        &self,
        base_url: &str,
        api_key: &str,
        id: &str,
        wait: bool,
        output: &PathBuf,
        glob: Option<String>,
    ) -> Result<()> {
        let started = Instant::now();
        println!("{} Checking test run state...", style("[1/4]").bold().dim());
        let client = RapiReqwestClient::new(base_url, api_key);
        let stat = client.get_run(id).await?;
        if stat.completed.is_none() && wait {
            loop {
                if stat.completed.is_some() {
                    break;
                }
                sleep(Duration::new(5, 0)).await;
            }
        } else {
            debug!("Test run {} finished", &id);
        }
        println!("{} Fetching file list...", style("[2/4]").bold().dim());
        let token = client.get_token().await?;
        let artifacts = fetch_artifact_list(&client, id, &token).await?;
        let test_run_id_prefix = format!("{}/", id);
        let artifacts = filter_artifact_list(artifacts, glob, &test_run_id_prefix)?;
        println!("{} Downloading files...", style("[3/4]").bold().dim());
        download_artifacts(&client, id, artifacts, output, &token, true).await?;
        println!(
            "{} Patching local relative paths...",
            style("[4/4]").bold().dim()
        );

        println!("Done in {}", HumanDuration(started.elapsed()));
        Ok(())
    }
}

fn filter_artifact_list(
    artifacts: Vec<Artifact>,
    glob: Option<String>,
    prefix: &str,
) -> Result<Vec<crate::api::Artifact>> {
    match glob {
        Some(glob) => {
            let matcher = Glob::new(&glob)?.compile_matcher();
            Ok(artifacts
                .into_iter()
                .filter(|x| -> bool {
                    let predicate_result =
                        matcher.is_match(x.id.strip_prefix(prefix).unwrap_or(&x.id));
                    if !predicate_result {
                        debug!("Filtered out download of {}", &x.id);
                    }
                    predicate_result
                })
                .collect())
        }
        None => Ok(artifacts),
    }
}

pub struct TriggerTestRunInteractor {}

impl TriggerTestRunInteractor {
    pub(crate) async fn execute(
        &self,
        base_url: &str,
        api_key: &str,
        name: Option<String>,
        link: Option<String>,
        wait: bool,
        isolated: Option<bool>,
        ignore_test_failures: Option<bool>,
        code_coverage: Option<bool>,
        retry_quota_test_uncompleted: Option<u32>,
        retry_quota_test_preventive: Option<u32>,
        retry_quota_test_reactive: Option<u32>,
        analytics_read_only: Option<bool>,
        filtering_configuration: Option<SparseMarathonfile>,
        output: &Option<PathBuf>,
        application: Option<PathBuf>,
        test_application: PathBuf,
        os_version: Option<String>,
        system_image: Option<String>,
        device: Option<String>,
        xcode_version: Option<String>,
        flavor: Option<String>,
        platform: String,
        progress: bool,
        env_args: Option<Vec<String>>,
        test_env_args: Option<Vec<String>>,
        output_glob: Option<String>,
    ) -> Result<bool> {
        let client = RapiReqwestClient::new(base_url, api_key);
        let steps = match (wait, output) {
            (true, Some(_)) => 5,
            (true, None) => 2,
            _ => 1,
        };

        let token = client.get_token().await?;
        println!(
            "{} Submitting new run...",
            style(format!("[1/{}]", steps)).bold().dim()
        );
        let id = client
            .create_run(
                application,
                test_application,
                name,
                link,
                platform,
                os_version,
                system_image,
                device,
                xcode_version,
                isolated,
                code_coverage,
                retry_quota_test_uncompleted,
                retry_quota_test_preventive,
                retry_quota_test_reactive,
                analytics_read_only,
                filtering_configuration,
                progress,
                flavor,
                env_args,
                test_env_args,
            )
            .await?;

        if wait {
            println!(
                "{} Waiting for test run to finish...",
                style(format!("[2/{}]", steps)).bold().dim()
            );

            let spinner = if progress {
                let pb = ProgressBar::new_spinner();
                pb.enable_steady_tick(Duration::from_millis(120));
                pb.set_style(
                    ProgressStyle::with_template("{spinner}")
                        .unwrap()
                        .tick_strings(&[
                            "( ●    )",
                            "(  ●   )",
                            "(   ●  )",
                            "(    ● )",
                            "(     ●)",
                            "(    ● )",
                            "(   ●  )",
                            "(  ●   )",
                            "( ●    )",
                            "(●     )",
                        ]),
                );
                Some(pb)
            } else {
                None
            };
            loop {
                let stat = client.get_run(&id).await?;
                if stat.completed.is_some() {
                    if let Some(s) = spinner {
                        s.finish_and_clear()
                    }

                    match stat.state.as_ref() {
                        "passed" => println!("Marathon Cloud execution finished"),
                        "failure" => println!("Marathon Cloud execution finished with failures"),
                        _ => println!("Marathon cloud execution crashed"),
                    };
                    println!("\tstate: {}", stat.state);

                    let base_report_url = Url::parse(base_url)?;
                    let base_report_url = &base_report_url[..Position::AfterPort];
                    println!("\treport: {}/report/{}", base_report_url, id);
                    println!(
                        "\tpassed: {}",
                        stat.passed
                            .map(|x| x.to_string())
                            .unwrap_or("missing".to_owned())
                    );
                    println!(
                        "\tfailed: {}",
                        stat.failed
                            .map(|x| x.to_string())
                            .unwrap_or("missing".to_owned())
                    );
                    println!(
                        "\tignored: {}",
                        stat.ignored
                            .map(|x| x.to_string())
                            .unwrap_or("missing".to_owned())
                    );

                    if let Some(output) = output {
                        println!(
                            "{} Fetching file list...",
                            style(format!("[3/{}]", steps)).bold().dim()
                        );
                        let artifacts = fetch_artifact_list(&client, &id, &token).await?;
                        let test_run_id_prefix = format!("{}/", &id);
                        let artifacts =
                            filter_artifact_list(artifacts, output_glob, &test_run_id_prefix)?;
                        println!(
                            "{} Downloading files...",
                            style(format!("[4/{}]", steps)).bold().dim()
                        );
                        download_artifacts(&client, &id, artifacts, output, &token, true).await?;
                        println!(
                            "{} Patching local relative paths...",
                            style(format!("[5/{}]", steps)).bold().dim()
                        );
                    }
                    return match (stat.state.as_str(), ignore_test_failures) {
                        ("failure", Some(false) | None) => Ok(false),
                        (_, _) => Ok(true),
                    };
                }
                sleep(Duration::new(5, 0)).await;
            }
        } else {
            println!("Test run {} started", id);
            Ok(true)
        }
    }
}

pub struct GetDeviceCatalogInteractor {}

impl GetDeviceCatalogInteractor {
    pub(crate) async fn execute(
        &self,
        base_url: &str,
        api_key: &str,
        platform: &Platform,
    ) -> Result<()> {
        println!("Fetching device catalog...");
        let client = RapiReqwestClient::new(base_url, api_key);

        let token = client.get_token().await?;
        let out = match platform {
            Platform::Android => {
                let devices = client.get_devices_android(&token).await?;
                serde_yaml::to_string(&devices)?
            }
            Platform::iOS => todo!(),
        };
        println!("{}", out);
        Ok(())
    }
}
