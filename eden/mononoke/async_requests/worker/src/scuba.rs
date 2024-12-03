/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_requests::types::AsynchronousRequestResult;
use async_requests::RequestId;
use async_requests_types_thrift::AsynchronousRequestResult as ThriftAsynchronousRequestResult;
use context::CoreContext;
use futures_stats::FutureStats;
use slog::info;
use source_control::AsyncRequestError;

use crate::worker::AsyncMethodRequestWorker;

impl AsyncMethodRequestWorker {
    pub(crate) fn prepare_ctx(
        &self,
        ctx: &CoreContext,
        req_id: &RequestId,
        target: &str,
    ) -> CoreContext {
        let ctx = ctx.with_mutated_scuba(|mut scuba| {
            // Legacy columns
            scuba.add("request_id", req_id.0.0);
            scuba.add("request_type", req_id.1.0.clone());

            // New column names to match the mononoke_scs_server table
            scuba.add("token", format!("{}", req_id.0.0));
            scuba.add("method", req_id.1.0.clone());
            scuba
        });

        info!(
            ctx.logger(),
            "[{}] new request:  id: {}, type: {}, {}", &req_id.0, &req_id.0, &req_id.1, target,
        );

        ctx
    }
}

pub(crate) fn log_start(ctx: &CoreContext) {
    let mut scuba = ctx.scuba().clone();
    scuba.log_with_msg("Request start", None);
}

pub(crate) fn log_result(
    ctx: CoreContext,
    tag: &'static str,
    stats: &FutureStats,
    result: &Result<AsynchronousRequestResult>,
) {
    let mut scuba = ctx.scuba().clone();

    let (status, error) = match result {
        Ok(response) => match response.thrift() {
            ThriftAsynchronousRequestResult::error(error) => match error {
                AsyncRequestError::request_error(error) => {
                    ("REQUEST_ERROR", Some(format!("{:?}", error)))
                }
                AsyncRequestError::internal_error(error) => {
                    ("INTERNAL_ERROR", Some(format!("{:?}", error)))
                }
                AsyncRequestError::UnknownField(error) => {
                    ("UNKNOWN_ERROR", Some(format!("unknown error: {:?}", error)))
                }
            },
            _ => ("SUCCESS", None),
        },
        Err(err) => ("POLL_ERROR", Some(err.to_string())),
    };

    scuba.add_future_stats(stats);
    scuba.add("status", status);

    if let Some(error) = error {
        scuba.unsampled();
        scuba.add("error", error.as_str());
    }
    scuba.log_with_msg(tag, None);
}
