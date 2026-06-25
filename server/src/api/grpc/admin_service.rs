use super::*;

#[tonic::async_trait]
impl admin_service_server::AdminService for HarpeGrpc {
    async fn list_background_jobs(
        &self,
        request: Request<ListBackgroundJobsRequest>,
    ) -> std::result::Result<Response<pb::ListBackgroundJobsResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "AdminService.ListBackgroundJobs");
        let request = request.into_inner();
        let status = admin_job_status_filter(request.status).map_err(status_from_error)?;
        let jobs = self
            .store
            .list_jobs(status, request_limit(request.limit, request.page.as_ref()))
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(background_job_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(jobs.len()));

        Ok(Response::new(pb::ListBackgroundJobsResponse { jobs, page }))
    }

    async fn retry_background_job(
        &self,
        request: Request<RetryBackgroundJobRequest>,
    ) -> std::result::Result<Response<pb::BackgroundJobDebug>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "AdminService.RetryBackgroundJob");
        let request = request.into_inner();
        validate_job_id(&request.job_id).map_err(status_from_error)?;
        let max_attempts = (request.max_attempts > 0).then_some(request.max_attempts);
        let job = self
            .store
            .retry_failed_job(&request.job_id, max_attempts)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(background_job_to_pb(job)))
    }

    async fn purge_background_job(
        &self,
        request: Request<PurgeBackgroundJobRequest>,
    ) -> std::result::Result<Response<pb::BackgroundJobDebug>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "AdminService.PurgeBackgroundJob");
        let request = request.into_inner();
        validate_job_id(&request.job_id).map_err(status_from_error)?;
        let job = self
            .store
            .purge_failed_job(&request.job_id)
            .await
            .map_err(status_from_error)?;

        Ok(Response::new(background_job_to_pb(job)))
    }

    async fn list_memory_chunks(
        &self,
        request: Request<ListMemoryChunksRequest>,
    ) -> std::result::Result<Response<pb::ListMemoryChunksResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        tracing::debug!(rpc = "AdminService.ListMemoryChunks");
        let request = request.into_inner();
        if request.session_id.trim().is_empty() {
            return Err(status_from_error(HarpeError::Validation(
                "session id is required".to_owned(),
            )));
        }

        let chunks = self
            .store
            .list_memory_chunks(
                &request.session_id,
                request_limit(request.limit, request.page.as_ref()),
            )
            .await
            .map_err(status_from_error)?
            .into_iter()
            .map(memory_chunk_to_pb)
            .collect::<Vec<_>>();
        let page = Some(page_info(chunks.len()));

        Ok(Response::new(pb::ListMemoryChunksResponse { chunks, page }))
    }
}
