use std::ffi::OsString;
use std::sync::atomic::AtomicBool;

use serde_json::Value;

use super::FornaxCli;
use super::FornaxCliError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FornaxPageRequest {
    pub page_size: u32,
    pub page_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExperimentEvaluatorOnlyRequest {
    pub name: String,
    pub eval_set_id: String,
    pub eval_set_version: String,
    pub evaluators: Vec<String>,
    pub evaluator_mappings: Vec<String>,
    pub item_retry_num: u32,
}

impl FornaxCli {
    pub fn list_datasets(
        &self,
        name: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
        cancelled: &AtomicBool,
    ) -> Result<Value, FornaxCliError> {
        let mut args: Vec<OsString> = vec![
            "dataset".into(),
            "list".into(),
            "--limit".into(),
            limit.to_string().into(),
        ];
        push_optional(&mut args, "--name", name)?;
        push_optional(&mut args, "--cursor", cursor)?;
        self.run_json(args, cancelled)
    }

    pub fn list_eval_sets(
        &self,
        name: Option<&str>,
        page: &FornaxPageRequest,
        cancelled: &AtomicBool,
    ) -> Result<Value, FornaxCliError> {
        validate_page(page)?;
        let mut args: Vec<OsString> = vec![
            "eval-set".into(),
            "list".into(),
            "--page-size".into(),
            page.page_size.to_string().into(),
        ];
        push_optional(&mut args, "--name", name)?;
        push_optional(&mut args, "--page-token", page.page_token.as_deref())?;
        self.run_json(args, cancelled)
    }

    pub fn list_evaluators(
        &self,
        name: Option<&str>,
        page_size: u32,
        cancelled: &AtomicBool,
    ) -> Result<Value, FornaxCliError> {
        validate_page_size(page_size)?;
        let mut args: Vec<OsString> = vec![
            "evaluator".into(),
            "list".into(),
            "--with-version".into(),
            "--page-size".into(),
            page_size.to_string().into(),
        ];
        push_optional(&mut args, "--name", name)?;
        self.run_json(args, cancelled)
    }

    pub fn experiment_detail(
        &self,
        experiment_id: &str,
        cancelled: &AtomicBool,
    ) -> Result<Value, FornaxCliError> {
        validate_scalar("experiment_id", experiment_id)?;
        self.run_json(
            ["experiment", "detail", "--experiment-id", experiment_id],
            cancelled,
        )
    }

    /// Submits evaluator-only evaluation. This deliberately cannot construct a target run.
    pub fn submit_evaluator_only_experiment(
        &self,
        request: &ExperimentEvaluatorOnlyRequest,
        cancelled: &AtomicBool,
    ) -> Result<Value, FornaxCliError> {
        validate_scalar("name", &request.name)?;
        validate_scalar("eval_set_id", &request.eval_set_id)?;
        validate_semver("eval_set_version", &request.eval_set_version)?;
        if request.evaluators.is_empty() {
            return invalid("at least one evaluator id:version is required");
        }
        if request.item_retry_num > 10 {
            return invalid("item_retry_num must be at most 10");
        }
        let mut args: Vec<OsString> = vec![
            "experiment".into(),
            "submit".into(),
            "--name".into(),
            request.name.clone().into(),
            "--eval-set-id".into(),
            request.eval_set_id.clone().into(),
            "--eval-set-version".into(),
            request.eval_set_version.clone().into(),
            "--skip-target".into(),
            "--item-retry-num".into(),
            request.item_retry_num.to_string().into(),
        ];
        for evaluator in &request.evaluators {
            validate_versioned_id("evaluator", evaluator)?;
            args.extend(["--evaluator".into(), evaluator.clone().into()]);
        }
        for mapping in &request.evaluator_mappings {
            validate_mapping(mapping)?;
            args.extend([
                "--evaluator-map-from-evalset".into(),
                mapping.clone().into(),
            ]);
        }
        self.run_json(args, cancelled)
    }

    pub fn list_synthesis_jobs(
        &self,
        job_ids: &[String],
        cancelled: &AtomicBool,
    ) -> Result<Value, FornaxCliError> {
        let mut args: Vec<OsString> = vec!["synthesis".into(), "list".into()];
        if !job_ids.is_empty() {
            for id in job_ids {
                validate_scalar("job_id", id)?;
            }
            args.extend(["--job-ids".into(), job_ids.join(",").into()]);
        }
        self.run_json(args, cancelled)
    }

    pub fn list_models(
        &self,
        page_size: u32,
        statuses: &[String],
        cancelled: &AtomicBool,
    ) -> Result<Value, FornaxCliError> {
        validate_page_size(page_size)?;
        let mut args: Vec<OsString> = vec![
            "model".into(),
            "list".into(),
            "--page-size".into(),
            page_size.to_string().into(),
        ];
        for status in statuses {
            if !matches!(
                status.as_str(),
                "Available" | "Deploying" | "Unavailable" | "Offlining"
            ) {
                return invalid("unsupported model status");
            }
            args.extend(["--status".into(), status.clone().into()]);
        }
        self.run_json(args, cancelled)
    }

    pub fn get_users_by_external_ids(
        &self,
        external_user_ids: &[String],
        cancelled: &AtomicBool,
    ) -> Result<Value, FornaxCliError> {
        if external_user_ids.is_empty() {
            return invalid("at least one external user id is required");
        }
        for id in external_user_ids {
            validate_scalar("external_user_id", id)?;
        }
        self.run_json(
            [
                OsString::from("user"),
                OsString::from("get"),
                OsString::from("--ext-user-ids"),
                external_user_ids.join(",").into(),
            ],
            cancelled,
        )
    }
}

fn validate_page(page: &FornaxPageRequest) -> Result<(), FornaxCliError> {
    validate_page_size(page.page_size)?;
    if let Some(token) = &page.page_token {
        validate_scalar("page_token", token)?;
    }
    Ok(())
}

fn validate_page_size(value: u32) -> Result<(), FornaxCliError> {
    if !(1..=200).contains(&value) {
        return invalid("page_size must be between 1 and 200");
    }
    Ok(())
}

fn validate_scalar(name: &str, value: &str) -> Result<(), FornaxCliError> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return invalid(&format!("{name} must be a bounded scalar"));
    }
    Ok(())
}

fn validate_semver(name: &str, value: &str) -> Result<(), FornaxCliError> {
    validate_scalar(name, value)?;
    semver::Version::parse(value)
        .map(|_| ())
        .map_err(|_| FornaxCliError::InvalidRequest {
            message: format!("{name} must be semantic version"),
        })
}

fn validate_versioned_id(name: &str, value: &str) -> Result<(), FornaxCliError> {
    let Some((id, version)) = value.split_once(':') else {
        return invalid(&format!("{name} must use id:version"));
    };
    validate_scalar(name, id)?;
    validate_semver("evaluator_version", version)
}

fn validate_mapping(value: &str) -> Result<(), FornaxCliError> {
    let Some((versioned, fields)) = value.rsplit_once(':') else {
        return invalid("evaluator mapping must include evaluator id and version");
    };
    validate_versioned_id("evaluator mapping", versioned)?;
    let Some((target, source)) = fields.split_once('=') else {
        return invalid("evaluator mapping must use field=eval_set_field");
    };
    validate_scalar("evaluator field", target)?;
    validate_scalar("eval_set field", source)
}

fn push_optional(
    args: &mut Vec<OsString>,
    flag: &str,
    value: Option<&str>,
) -> Result<(), FornaxCliError> {
    if let Some(value) = value {
        validate_scalar(flag, value)?;
        args.extend([flag.into(), value.into()]);
    }
    Ok(())
}

fn invalid<T>(message: &str) -> Result<T, FornaxCliError> {
    Err(FornaxCliError::InvalidRequest {
        message: message.to_string(),
    })
}
