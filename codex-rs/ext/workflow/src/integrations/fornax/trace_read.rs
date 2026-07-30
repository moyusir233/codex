use std::ffi::OsString;
use std::sync::atomic::AtomicBool;

use serde::Deserialize;

use super::FornaxCli;
use super::FornaxCliError;
use super::FornaxSpan;
use super::FornaxTrajectory;
use super::SpanListRequest;
use super::SpanPage;
use super::TimeWindow;
use super::TraceGetRequest;
use super::TraceListRequest;
use super::TraceLookup;
use super::TracePage;
use super::TrajectoryLookup;

impl FornaxCli {
    /// Reads one trace by trace ID or log ID.
    pub fn get_trace(
        &self,
        request: &TraceGetRequest,
        cancelled: &AtomicBool,
    ) -> Result<TracePage, FornaxCliError> {
        let mut args = vec![OsString::from("trace"), OsString::from("get")];
        match &request.lookup {
            TraceLookup::TraceId(trace_id) => {
                push_required(&mut args, "--trace-id", trace_id)?;
            }
            TraceLookup::LogId(log_id) => {
                push_required(&mut args, "--log-id", log_id)?;
            }
        }
        if !request.span_ids.is_empty() {
            if request.span_ids.iter().any(|span_id| span_id.contains(',')) {
                return Err(invalid("span IDs must not contain commas"));
            }
            args.extend([
                OsString::from("--span-id"),
                OsString::from(request.span_ids.join(",")),
            ]);
        }
        if let Some(window) = &request.time_window {
            push_time_window(&mut args, window)?;
        }
        if request.tree {
            args.push(OsString::from("--tree"));
        }
        let payload: TracePayload = self.run_json(args, cancelled)?;
        Ok(TracePage {
            spans: payload.into_spans(),
        })
    }

    /// Lists traces within an explicit stable time window.
    pub fn list_traces(
        &self,
        request: &TraceListRequest,
        cancelled: &AtomicBool,
    ) -> Result<TracePage, FornaxCliError> {
        require_page_size(request.page_size)?;
        let mut args = vec![
            OsString::from("trace"),
            OsString::from("list"),
            OsString::from("--page-size"),
            OsString::from(request.page_size.to_string()),
        ];
        push_optional(&mut args, "--trace-filter-expr", &request.trace_filter);
        push_optional(&mut args, "--span-filter-expr", &request.span_filter);
        push_time_window(&mut args, &request.time_window)?;
        let payload: TracePayload = self.run_json(args, cancelled)?;
        Ok(TracePage {
            spans: payload.into_spans(),
        })
    }

    /// Lists one stable page from the span index.
    pub fn list_spans(
        &self,
        request: &SpanListRequest,
        cancelled: &AtomicBool,
    ) -> Result<SpanPage, FornaxCliError> {
        require_page_size(request.page_size)?;
        let mut args = vec![
            OsString::from("span"),
            OsString::from("list"),
            OsString::from("--page-size"),
            OsString::from(request.page_size.to_string()),
        ];
        push_optional(&mut args, "--page-token", &request.page_token);
        push_optional(&mut args, "--span-filter-expr", &request.span_filter);
        push_time_window(&mut args, &request.time_window)?;
        self.run_json(args, cancelled)
    }

    /// Reads trajectories by exactly one supported correlation selector.
    pub fn get_trajectories(
        &self,
        lookup: &TrajectoryLookup,
        time_window: Option<&TimeWindow>,
        cancelled: &AtomicBool,
    ) -> Result<Vec<FornaxTrajectory>, FornaxCliError> {
        let mut args = vec![OsString::from("trajectory")];
        match lookup {
            TrajectoryLookup::TraceIds(trace_ids) => {
                if trace_ids.is_empty() || trace_ids.iter().any(|trace_id| trace_id.contains(',')) {
                    return Err(invalid(
                        "trajectory trace IDs must be non-empty and contain no commas",
                    ));
                }
                args.extend([
                    OsString::from("--trace-id"),
                    OsString::from(trace_ids.join(",")),
                ]);
            }
            TrajectoryLookup::ExperimentId(experiment_id) => {
                push_required(&mut args, "--experiment-id", experiment_id)?;
            }
            TrajectoryLookup::LogId(log_id) => {
                push_required(&mut args, "--log-id", log_id)?;
            }
        }
        if let Some(window) = time_window {
            push_time_window(&mut args, window)?;
        }
        self.run_json(args, cancelled)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TracePayload {
    Page(TracePage),
    Spans(Vec<FornaxSpan>),
}

impl TracePayload {
    fn into_spans(self) -> Vec<FornaxSpan> {
        match self {
            Self::Page(page) => page.spans,
            Self::Spans(spans) => spans,
        }
    }
}

fn push_time_window(args: &mut Vec<OsString>, window: &TimeWindow) -> Result<(), FornaxCliError> {
    match window {
        TimeWindow::LastMinutes(minutes) if *minutes > 0 => args.extend([
            OsString::from("--last-n-minutes"),
            OsString::from(minutes.to_string()),
        ]),
        TimeWindow::LastMinutes(_) => return Err(invalid("time window must be positive")),
        TimeWindow::Iso8601 { since, until } if !since.is_empty() && !until.is_empty() => {
            args.extend([
                OsString::from("--since"),
                OsString::from(since),
                OsString::from("--until"),
                OsString::from(until),
            ]);
        }
        TimeWindow::Iso8601 { .. } => return Err(invalid("ISO-8601 window requires both bounds")),
        TimeWindow::EpochMillis { start_ms, end_ms } if start_ms < end_ms => args.extend([
            OsString::from("--start-ms"),
            OsString::from(start_ms.to_string()),
            OsString::from("--end-ms"),
            OsString::from(end_ms.to_string()),
        ]),
        TimeWindow::EpochMillis { .. } => {
            return Err(invalid("epoch window start must precede end"));
        }
    }
    Ok(())
}

fn push_required(
    args: &mut Vec<OsString>,
    name: &'static str,
    value: &str,
) -> Result<(), FornaxCliError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(invalid(
            "identifier must be non-empty and contain no controls",
        ));
    }
    args.extend([OsString::from(name), OsString::from(value)]);
    Ok(())
}

fn push_optional(args: &mut Vec<OsString>, name: &'static str, value: &Option<String>) {
    if let Some(value) = value {
        args.extend([OsString::from(name), OsString::from(value)]);
    }
}

fn require_page_size(page_size: u32) -> Result<(), FornaxCliError> {
    if page_size == 0 || page_size > 200 {
        Err(invalid("page size must be between 1 and 200"))
    } else {
        Ok(())
    }
}

fn invalid(message: &'static str) -> FornaxCliError {
    FornaxCliError::InvalidRequest {
        message: message.to_string(),
    }
}
