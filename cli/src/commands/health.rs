use super::*;

pub(crate) async fn health<W: Write>(
    channel: Channel,
    args: HealthArgs,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let response = HealthServiceClient::new(channel)
        .check(HealthCheckRequest {
            service: args.service,
        })
        .await?
        .into_inner();

    if as_json {
        return write_json(writer, &health_json(&response));
    }

    writeln!(
        writer,
        "{} status={} database_ok={} pending_jobs={} failed_jobs={} checked_at={}",
        response.service,
        serving_status_name(response.status),
        response.database_ok,
        response.pending_jobs,
        response.failed_jobs,
        response.checked_at
    )?;
    Ok(())
}
