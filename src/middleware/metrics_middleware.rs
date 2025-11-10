use actix_web::dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform};
use futures_util::future::LocalBoxFuture;
use std::future::{ready, Ready};
use std::sync::Arc;
use std::time::Instant;

use crate::services::MetricsServiceTrait;

pub struct MetricsMiddleware {
    pub metrics_service: Arc<dyn MetricsServiceTrait>,
}

impl<S, B> Transform<S, ServiceRequest> for MetricsMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = actix_web::Error;
    type InitError = ();
    type Transform = MetricsMiddlewareService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(MetricsMiddlewareService {
            service,
            metrics_service: self.metrics_service.clone(),
        }))
    }
}

pub struct MetricsMiddlewareService<S> {
    service: S,
    metrics_service: Arc<dyn MetricsServiceTrait>,
}

impl<S, B> Service<ServiceRequest> for MetricsMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = actix_web::Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let start = Instant::now();
        let method = req.method().to_string();
        let path = req.path().to_string();
        let metrics_service = self.metrics_service.clone();
        let fut = self.service.call(req);

        Box::pin(async move {
            let response = fut.await?;

            let latency_ms = start.elapsed().as_millis() as i64;
            let status_code = response.status().to_string();

            if let Err(err) = metrics_service
                .record_api_latency_metric(
                    None,
                    &path,
                    &method,
                    &status_code,
                    latency_ms,
                    None,
                )
                .await
            {
                log::warn!(
                    "Failed to record API latency metric for {} {}: {}",
                    method,
                    path,
                    err
                );
            }

            Ok(response)
        })
    }
}

