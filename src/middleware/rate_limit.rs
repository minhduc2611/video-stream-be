use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpMessage,
};
use futures_util::future::LocalBoxFuture;
use std::{
    future::{ready, Ready},
    rc::Rc,
};
use governor::{Quota, RateLimiter};
use governor::state::keyed::DefaultKeyedStateStore;
use std::num::NonZeroU32;
use std::hash::Hash;

pub struct RateLimitMiddleware<K> 
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
{
    limiter: RateLimiter<K, DefaultKeyedStateStore<K>>,
}

impl<K> RateLimitMiddleware<K>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
{
    pub fn new(requests_per_minute: u32) -> Self {
        let quota = Quota::per_minute(NonZeroU32::new(requests_per_minute).unwrap());
        let limiter = RateLimiter::keyed(quota);
        
        Self { limiter }
    }
}

impl<S, B, K> Transform<S, ServiceRequest> for RateLimitMiddleware<K>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
    K: Hash + Eq + Clone + Send + Sync + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = RateLimitService<S, K>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RateLimitService {
            service: Rc::new(service),
            limiter: self.limiter.clone(),
        }))
    }
}

pub struct RateLimitService<S, K> {
    service: Rc<S>,
    limiter: RateLimiter<K, DefaultKeyedStateStore<K>>,
}

impl<S, B, K> Service<ServiceRequest> for RateLimitService<S, K>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
    K: Hash + Eq + Clone + Send + Sync + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let limiter = self.limiter.clone();

        Box::pin(async move {
            // Extract client IP for rate limiting
            let client_ip = req.connection_info().peer_addr()
                .unwrap_or("unknown")
                .to_string();

            // Check rate limit
            if limiter.check_key(&client_ip).is_err() {
                return Ok(req.into_response(
                    actix_web::HttpResponse::TooManyRequests()
                        .json(serde_json::json!({
                            "success": false,
                            "error": "Rate limit exceeded. Please try again later."
                        }))
                ));
            }

            let res = service.call(req).await?;
            Ok(res)
        })
    }
}
