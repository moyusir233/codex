use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde_json::Value;

use super::FornaxCli;
use super::FornaxCliError;
use super::FornaxSkill;
use super::SkillLookup;

impl FornaxCli {
    /// Reads one or more skills through the pinned JSON commands.
    pub fn get_skills(
        &self,
        lookup: &SkillLookup,
        cancelled: &AtomicBool,
    ) -> Result<Vec<FornaxSkill>, FornaxCliError> {
        match lookup {
            SkillLookup::Id { skill_id, version } => {
                require_value("skill_id", skill_id)?;
                let mut args = vec![
                    OsString::from("skill"),
                    OsString::from("get"),
                    OsString::from("--skill-id"),
                    OsString::from(skill_id),
                ];
                if let Some(version) = version {
                    args.extend([
                        OsString::from("--with-commit"),
                        OsString::from("--commit-version"),
                        OsString::from(version),
                    ]);
                }
                self.run_json(args, cancelled).map(|skill| vec![skill])
            }
            SkillLookup::Ids { ids, version } => {
                batch_lookup_args("batch-get-by-id", "--skill-id", ids, version)
                    .and_then(|args| self.run_json(args, cancelled))
            }
            SkillLookup::Keys { keys, version } => {
                batch_lookup_args("batch-get-by-key", "--skill-key", keys, version)
                    .and_then(|args| self.run_json(args, cancelled))
            }
        }
    }

    /// Installs skills only into an explicit absolute staging directory.
    ///
    /// The adapter never permits the CLI's implicit interactive home selection.
    pub fn stage_skills(
        &self,
        keys: &[String],
        staging_dir: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Value, FornaxCliError> {
        if keys.is_empty() {
            return Err(FornaxCliError::InvalidRequest {
                message: "at least one skill key is required".to_string(),
            });
        }
        if !staging_dir.is_absolute() {
            return Err(FornaxCliError::InvalidRequest {
                message: "skill staging directory must be absolute".to_string(),
            });
        }
        std::fs::create_dir_all(staging_dir).map_err(|error| FornaxCliError::InvalidRequest {
            message: format!("could not create staging directory: {error}"),
        })?;
        let mut args = vec![OsString::from("skill"), OsString::from("install")];
        for key in keys {
            require_value("skill_key", key)?;
            args.push(OsString::from(key));
        }
        args.extend([
            OsString::from("--dir"),
            staging_dir.as_os_str().to_os_string(),
        ]);
        self.run_json(args, cancelled)
    }
}

fn batch_lookup_args(
    command: &'static str,
    flag: &'static str,
    values: &[String],
    version: &Option<String>,
) -> Result<Vec<OsString>, FornaxCliError> {
    if values.is_empty() {
        return Err(FornaxCliError::InvalidRequest {
            message: "at least one skill selector is required".to_string(),
        });
    }
    let mut args = vec![OsString::from("skill"), OsString::from(command)];
    for value in values {
        require_value("skill_selector", value)?;
        args.extend([OsString::from(flag), OsString::from(value)]);
    }
    if let Some(version) = version {
        args.extend([OsString::from("--version"), OsString::from(version)]);
    }
    Ok(args)
}

fn require_value(field: &'static str, value: &str) -> Result<(), FornaxCliError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        Err(FornaxCliError::InvalidRequest {
            message: format!("{field} must be non-empty and contain no control characters"),
        })
    } else {
        Ok(())
    }
}
