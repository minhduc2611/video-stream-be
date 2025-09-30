use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpMessage,
};
use futures_util::future::LocalBoxFuture;
use std::{
    future::{ready, Ready},
    rc::Rc,
};
use uuid::Uuid;
use std::env;

use crate::services::AuthService;
use crate::utils::response::ApiResponse;

pub struct AuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for AuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = AuthMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddlewareService {
            service: Rc::new(service),
        }))
    }
}

pub struct AuthMiddlewareService<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for AuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();

        Box::pin(async move {
            // Extract token from Authorization header
            let auth_header = req.headers().get("Authorization");
            
            if let Some(auth_header) = auth_header {
                if let Ok(auth_str) = auth_header.to_str() {
                    if auth_str.starts_with("Bearer ") {
                        let token = &auth_str[7..]; // Remove "Bearer " prefix
                        
                        let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
                        let auth_service = AuthService::new(
                            req.app_data::<actix_web::web::Data<sqlx::PgPool>>()
                                .unwrap()
                                .get_ref()
                                .clone(),
                            jwt_secret,
                        );

                        match auth_service.verify_token(token) {
                            Ok(claims) => {
                                if let Ok(user_id) = Uuid::parse_str(&claims.sub) {
                                    req.extensions_mut().insert(user_id);
                                } else {
                                    return Ok(req.into_response(
                                        actix_web::HttpResponse::Unauthorized()
                                            .json(ApiResponse::error("Invalid token", None))
                                    ));
                                }
                            }
                            Err(_) => {
                                return Ok(req.into_response(
                                    actix_web::HttpResponse::Unauthorized()
                                        .json(ApiResponse::error("Invalid token", None))
                                ));
                            }
                        }
                    } else {
                        return Ok(req.into_response(
                            actix_web::HttpResponse::Unauthorized()
                                .json(ApiResponse::error("Invalid authorization header format", None))
                        ));
                    }
                } else {
                    return Ok(req.into_response(
                        actix_web::HttpResponse::Unauthorized()
                            .json(ApiResponse::error("Invalid authorization header", None))
                    ));
                }
            } else {
                return Ok(req.into_response(
                    actix_web::HttpResponse::Unauthorized()
                        .json(ApiResponse::error("Authorization header required", None))
                ));
            }

            let res = service.call(req).await?;
            Ok(res)
        })
    }
}
