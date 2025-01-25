use std::ffi::OsStr;
use std::fmt::Display;

use anyhow::Result;
use std::collections::HashSet;
use tokio::fs::File;
use walkdir::WalkDir;

use crate::{
    cli::{self},
    compression,
    errors::ConfigurationError,
    interactor::TriggerTestRunInteractor,
};
use crate::{errors::InputError, filtering};

#[derive(Debug, clap::ValueEnum, Clone, PartialEq, Eq)]
pub enum IosDevice {
    #[clap(name = "iPhone-11")]
    IPhone11,
    #[clap(name = "iPhone-15")]
    IPhone15,
    #[clap(name = "iPhone-15-Pro")]
    IPhone15Pro,
    #[clap(name = "iPhone-15-Pro-Max")]
    IPhone15ProMax,
    #[clap(name = "iPhone-16")]
    IPhone16,
    #[clap(name = "iPhone-16-Pro")]
    IPhone16Pro,
    #[clap(name = "iPhone-16-Pro-Max")]
    IPhone16ProMax,
}

impl Display for IosDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IosDevice::IPhone11 => f.write_str("com.apple.CoreSimulator.SimDeviceType.iPhone-11"),
            IosDevice::IPhone15 => f.write_str("com.apple.CoreSimulator.SimDeviceType.iPhone-15"),
            IosDevice::IPhone15Pro => {
                f.write_str("com.apple.CoreSimulator.SimDeviceType.iPhone-15-Pro")
            }
            IosDevice::IPhone15ProMax => {
                f.write_str("com.apple.CoreSimulator.SimDeviceType.iPhone-15-Pro-Max")
            }
            IosDevice::IPhone16 => f.write_str("com.apple.CoreSimulator.SimDeviceType.iPhone-16"),
            IosDevice::IPhone16Pro => {
                f.write_str("com.apple.CoreSimulator.SimDeviceType.iPhone-16-Pro")
            }
            IosDevice::IPhone16ProMax => {
                f.write_str("com.apple.CoreSimulator.SimDeviceType.iPhone-16-Pro-Max")
            }
        }
    }
}

#[derive(Debug, clap::ValueEnum, Clone, PartialEq, Eq)]
pub enum OsVersion {
    #[clap(name = "17.5")]
    Ios17_5,
    #[clap(name = "18.2")]
    Ios18_2,
}

impl Display for OsVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OsVersion::Ios17_5 => f.write_str("com.apple.CoreSimulator.SimRuntime.iOS-17-5"),
            OsVersion::Ios18_2 => f.write_str("com.apple.CoreSimulator.SimRuntime.iOS-18-2"),
        }
    }
}

#[derive(Debug, clap::ValueEnum, Clone, PartialEq, Eq)]
pub enum XcodeVersion {
    #[clap(name = "15.4")]
    Xcode15_4,
    #[clap(name = "16.2")]
    Xcode16_2,
}

impl Display for XcodeVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XcodeVersion::Xcode15_4 => f.write_str("15.4"),
            XcodeVersion::Xcode16_2 => f.write_str("16.2"),
        }
    }
}

pub(crate) async fn ensure_format(path: std::path::PathBuf) -> Result<std::path::PathBuf> {
    let supported_extensions_file = vec!["zip", "ipa"];
    let supported_extensions_dir = vec!["app", "xctest"];
    if path.is_file()
        && path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|ext| supported_extensions_file.contains(&ext))
    {
        Ok(path)
    } else if path.is_dir()
        && path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|ext| supported_extensions_dir.contains(&ext))
    {
        let dst = &path.with_extension("zip");
        let dst_file = File::create(dst).await?;

        let walkdir = WalkDir::new(&path);
        let it = walkdir.into_iter();
        let prefix = &path
            .parent()
            .unwrap_or(&path)
            .to_str()
            .ok_or(InputError::NonUTF8Path { path: path.clone() })?;

        compression::zip_dir(&mut it.filter_map(|e| e.ok()), prefix, dst_file).await?;
        Ok(dst.to_owned())
    } else {
        Err(InputError::UnsupportedArtifact {
            path,
            supported_files: "[ipa,zip]".into(),
            supported_folders: "[app,xctest]".into(),
        }
        .into())
    }
}

pub(crate) fn get_supported_configs(
) -> Vec<(Option<IosDevice>, Option<XcodeVersion>, Option<OsVersion>)> {
    vec![
        (
            Some(IosDevice::IPhone15),
            Some(XcodeVersion::Xcode15_4),
            Some(OsVersion::Ios17_5),
        ),
        (
            Some(IosDevice::IPhone15Pro),
            Some(XcodeVersion::Xcode15_4),
            Some(OsVersion::Ios17_5),
        ),
        (
            Some(IosDevice::IPhone15ProMax),
            Some(XcodeVersion::Xcode15_4),
            Some(OsVersion::Ios17_5),
        ),
        (
            Some(IosDevice::IPhone11),
            Some(XcodeVersion::Xcode15_4),
            Some(OsVersion::Ios17_5),
        ),
        (
            Some(IosDevice::IPhone16),
            Some(XcodeVersion::Xcode16_2),
            Some(OsVersion::Ios18_2),
        ),
        (
            Some(IosDevice::IPhone16Pro),
            Some(XcodeVersion::Xcode16_2),
            Some(OsVersion::Ios18_2),
        ),
        (
            Some(IosDevice::IPhone16ProMax),
            Some(XcodeVersion::Xcode16_2),
            Some(OsVersion::Ios18_2),
        ),
        (
            Some(IosDevice::IPhone11),
            Some(XcodeVersion::Xcode16_2),
            Some(OsVersion::Ios18_2),
        ),
    ]
}

pub(crate) async fn infer_parameters(
    device: Option<IosDevice>,
    xcode_version: Option<XcodeVersion>,
    os_version: Option<OsVersion>,
) -> Result<(IosDevice, XcodeVersion, OsVersion)> {
    let supported_configs = get_supported_configs();

    // Filter out configurations that match the provided parameters
    let filtered_configs: Vec<&(Option<IosDevice>, Option<XcodeVersion>, Option<OsVersion>)> =
        supported_configs
            .iter()
            .filter(|(d, x, o)| {
                (device.is_none() || d == &device)
                    && (xcode_version.is_none() || x == &xcode_version)
                    && (os_version.is_none() || o == &os_version)
            })
            .collect();

    // If no valid configuration is found, return an error
    if filtered_configs.is_empty() {
        return Err(anyhow::anyhow!("Invalid parameters"));
    }

    // If only one valid configuration is found, use it
    if filtered_configs.len() == 1 {
        let (final_device, final_xcode, final_os) = filtered_configs[0];
        return Ok((
            final_device.clone().unwrap(),
            final_xcode.clone().unwrap(),
            final_os.clone().unwrap(),
        ));
    }

    // If multiple configurations are still valid, we need more specific parameters
    if filtered_configs.len() > 1 {
        return Err(anyhow::anyhow!(
            "Ambiguous parameters, please provide more specific input."
        ));
    }

    Ok((device.unwrap(), xcode_version.unwrap(), os_version.unwrap()))
}

fn get_allowed_permissions() -> HashSet<&'static str> {
    HashSet::from([
        "calendar",
        "contacts-limited",
        "contacts",
        "location",
        "location-always",
        "photos-add",
        "photos",
        "media-library",
        "microphone",
        "motion",
        "reminders",
        "siri",
    ])
}

pub(crate) async fn run(
    application: std::path::PathBuf,
    test_application: std::path::PathBuf,
    os_version: Option<OsVersion>,
    device: Option<IosDevice>,
    xcode_version: Option<XcodeVersion>,
    common: super::CommonRunArgs,
    api_args: super::ApiArgs,
    xctestrun_env: Option<Vec<String>>,
    xctestrun_test_env: Option<Vec<String>>,
    xctestplan_filter_file: Option<std::path::PathBuf>,
    xctestplan_target_name: Option<String>,
    retry_args: super::RetryArgs,
    analytics_args: super::AnalyticsArgs,
    test_timeout_default: Option<u32>,
    test_timeout_max: Option<u32>,
    granted_permission: Option<Vec<String>>,
) -> Result<bool> {
    let (device, xcode_version, os_version) = if device.is_none()
        && xcode_version.is_none()
        && os_version.is_none()
    {
        (None, None, None)
    } else {
        match infer_parameters(device, xcode_version, os_version).await {
            Ok((dev, xcode, os)) => (Some(dev), Some(xcode), Some(os)),
            Err(_) => {
                return Err(ConfigurationError::UnsupportedRunConfiguration {
                    message: "
Please set --xcode-version, --os-version, and --device correctly.
Supported iOS settings combinations are:
    --xcode-version 15.4 --os-version 17.5 --device iPhone-15 => Default
    --xcode-version 15.4 --os-version 17.5 --device iPhone-15-Pro
    --xcode-version 15.4 --os-version 17.5 --device iPhone-15-Pro-Max
    --xcode-version 15.4 --os-version 17.5 --device iPhone-11
    --xcode-version 16.2 --os-version 18.2 --device iPhone-16
    --xcode-version 16.2 --os-version 18.2 --device iPhone-16-Pro
    --xcode-version 16.2 --os-version 18.2 --device iPhone-16-Pro-Max
    --xcode-version 16.2 --os-version 18.2 --device iPhone-11
First example: If you choose --xcode-version 15.4 --device iPhone-15-Pro then the --os-version will be inferred (17.5).
Second example: If you choose --device iPhone-11 then you will receive an error because --os-version and --xcode-version params are ambiguous."
                        .into(),
                }.into());
            }
        }
    };

    let filtering_configuration = if xctestplan_filter_file.is_some() {
        Some(
            filtering::convert::convert_xctestplan(
                xctestplan_filter_file.unwrap(),
                xctestplan_target_name,
            )
            .await?,
        )
    } else {
        let filter_file = common.filter_file.map(filtering::convert::convert);
        match filter_file {
            Some(future) => Some(future.await?),
            None => None,
        }
    };
    let application = ensure_format(application).await?;
    let test_application = ensure_format(test_application).await?;

    let retry_args = cli::validate::retry_args(retry_args);
    cli::validate::result_file_args(&common.result_file_args)?;

    if let Some(limit) = common.concurrency_limit {
        if limit == 0 {
            return Err(InputError::NonPositiveValue {
                arg: "--concurrency-limit".to_owned(),
            })?;
        }
    }

    if let Some(limit) = test_timeout_default {
        if limit == 0 {
            return Err(InputError::NonPositiveValue {
                arg: "--test-timeout-default".to_owned(),
            })?;
        }
    }

    if let Some(limit) = test_timeout_max {
        if limit == 0 {
            return Err(InputError::NonPositiveValue {
                arg: "--test-timeout-max".to_owned(),
            })?;
        }
    }

    if let Some(granted_permission) = granted_permission.clone() {
        let allowed_permissions = get_allowed_permissions();
        let invalid_permissions: Vec<_> = granted_permission
            .iter()
            .filter(|perm| !allowed_permissions.contains(perm.as_str()))
            .cloned()
            .collect();

        if !invalid_permissions.is_empty() {
            return Err(InputError::IncorrectPermission {
                permissions: invalid_permissions,
            })?;
        }
    }

    let present_wait: bool = match common.wait {
        None => true,
        Some(true) => true,
        Some(false) => false,
    };

    TriggerTestRunInteractor {}
        .execute(
            &api_args.base_url,
            &api_args.api_key,
            common.name,
            common.link,
            common.branch,
            present_wait,
            common.isolated,
            common.ignore_test_failures,
            common.code_coverage,
            retry_args.retry_quota_test_uncompleted,
            retry_args.retry_quota_test_preventive,
            retry_args.retry_quota_test_reactive,
            analytics_args.analytics_read_only,
            false,
            false,
            filtering_configuration,
            &common.output,
            Some(application),
            Some(test_application),
            os_version.map(|x| x.to_string()),
            None,
            device.map(|x| x.to_string()),
            xcode_version.map(|x| x.to_string()),
            None,
            "iOS".to_owned(),
            common.progress_args.no_progress_bars,
            common.result_file_args.result_file,
            xctestrun_env,
            xctestrun_test_env,
            None,
            common.concurrency_limit,
            test_timeout_default,
            test_timeout_max,
            common.project,
            None,
            None,
            granted_permission,
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_infer_parameters_ambiguous_device_should_error() {
        let provided_device = Some(IosDevice::IPhone11);

        let result = infer_parameters(provided_device, None, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_infer_parameters_device_and_xcode_version_provided() -> Result<()> {
        let provided_device = Some(IosDevice::IPhone15);
        let provided_xcode_version = Some(XcodeVersion::Xcode15_4);
        let expected_os_version = OsVersion::Ios17_5;

        let (inferred_device, inferred_xcode_version, inferred_os_version) =
            infer_parameters(provided_device, provided_xcode_version, None).await?;

        assert_eq!(inferred_device, IosDevice::IPhone15);
        assert_eq!(inferred_xcode_version, XcodeVersion::Xcode15_4);
        assert_eq!(inferred_os_version, expected_os_version);

        Ok(())
    }

    #[tokio::test]
    async fn test_infer_parameters_ambiguous_xcode_version_should_error() {
        let provided_xcode_version = Some(XcodeVersion::Xcode15_4);

        let result = infer_parameters(None, provided_xcode_version, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_infer_parameters_complete_input_valid() -> Result<()> {
        let provided_device = Some(IosDevice::IPhone15);
        let provided_xcode_version = Some(XcodeVersion::Xcode15_4);
        let provided_os_version = Some(OsVersion::Ios17_5);

        let (inferred_device, inferred_xcode_version, inferred_os_version) =
            infer_parameters(provided_device, provided_xcode_version, provided_os_version).await?;

        assert_eq!(inferred_device, IosDevice::IPhone15);
        assert_eq!(inferred_xcode_version, XcodeVersion::Xcode15_4);
        assert_eq!(inferred_os_version, OsVersion::Ios17_5);

        Ok(())
    }

    #[tokio::test]
    async fn test_infer_parameters_invalid_device_and_xcode_combination_should_error() {
        let provided_os_version = Some(OsVersion::Ios17_5);
        let provided_xcode_version = Some(XcodeVersion::Xcode15_4);

        let result = infer_parameters(None, provided_xcode_version, provided_os_version).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_infer_parameters_valid_for_iphone_16() -> Result<()> {
        let provided_device = Some(IosDevice::IPhone16);

        let (inferred_device, inferred_xcode_version, inferred_os_version) =
            infer_parameters(provided_device, None, None).await?;

        assert_eq!(inferred_device, IosDevice::IPhone16);
        assert_eq!(inferred_xcode_version, XcodeVersion::Xcode16_2);
        assert_eq!(inferred_os_version, OsVersion::Ios18_2);

        Ok(())
    }
}
