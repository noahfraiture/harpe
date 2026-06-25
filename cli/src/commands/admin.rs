use super::*;

pub(crate) async fn admin<W: Write>(
    channel: Channel,
    args: AdminArgs,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    let mut client = AdminServiceClient::new(channel);
    match args.command {
        AdminCommand::Jobs { status, page } => {
            let response = client
                .list_background_jobs(ListBackgroundJobsRequest {
                    status: admin_status_filter(status),
                    limit: 0,
                    page: Some(page.request()),
                })
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "jobs": response.jobs.iter().map(background_job_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for job in response.jobs {
                    writeln!(
                        writer,
                        "{}\t{}\tstatus={}\tattempts={}/{}\trun_after={}\t{}",
                        job.id,
                        job_kind_name(job.kind),
                        admin_status_name(job.status),
                        job.attempts,
                        job.max_attempts,
                        job.run_after,
                        job.last_error
                    )?;
                }
                Ok(())
            }
        }
        AdminCommand::RetryJob {
            job_id,
            max_attempts,
        } => {
            let response = client
                .retry_background_job(pb::RetryBackgroundJobRequest {
                    job_id,
                    max_attempts: max_attempts.unwrap_or_default(),
                })
                .await?
                .into_inner();
            write_job(writer, as_json, &response)
        }
        AdminCommand::PurgeJob { job_id } => {
            let response = client
                .purge_background_job(pb::PurgeBackgroundJobRequest { job_id })
                .await?
                .into_inner();
            write_job(writer, as_json, &response)
        }
        AdminCommand::MemoryChunks { session_id, page } => {
            let response = client
                .list_memory_chunks(ListMemoryChunksRequest {
                    session_id,
                    limit: 0,
                    page: Some(page.request()),
                })
                .await?
                .into_inner();
            if as_json {
                write_json(
                    writer,
                    &json!({
                        "chunks": response.chunks.iter().map(memory_chunk_json).collect::<Vec<_>>(),
                        "page": page_json(response.page.as_ref()),
                    }),
                )
            } else {
                for chunk in response.chunks {
                    writeln!(
                        writer,
                        "{}\tkind={}\tembedding_dims={}\t{}",
                        chunk.id,
                        chunk.kind,
                        chunk.embedding.len(),
                        chunk.content
                    )?;
                }
                Ok(())
            }
        }
    }
}
