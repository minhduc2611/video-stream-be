use actix_utils::future::{ready, Ready};
use actix_web::{
    body::{EitherBody, MessageBody},
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpMessage, HttpResponse,
};

use futures_util::future::LocalBoxFuture;
use std::rc::Rc;
use uuid::Uuid;

use std::sync::Arc;

use crate::app_state::AppState;
use crate::services::AuthServiceTrait;
use crate::utils::response::ApiResponse;

pub struct AuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for AuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
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
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();

        Box::pin(async move {
            let app_state = match req.app_data::<actix_web::web::Data<AppState>>().cloned() {
                Some(state) => state,
                None => {
                    log::error!("Application state missing in request context");
                    let res = HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error("Internal server error", None));
                    return Ok(req.into_response(res).map_into_right_body());
                }
            };

            // Extract token from Authorization header
            let auth_header = req.headers().get("Authorization");
            log::info!("Auth header: {:?}", auth_header);
            if let Some(auth_header) = auth_header {
                if let Ok(auth_str) = auth_header.to_str() {
                    if auth_str.starts_with("Bearer ") {
                        let token = &auth_str[7..]; // Remove "Bearer " prefix

                        let auth_service = Arc::clone(&app_state.auth_service);

                        match auth_service.verify_token(token) {
                            Ok(claims) => {
                                if let Ok(user_id) = Uuid::parse_str(&claims.sub) {
                                    req.extensions_mut().insert(user_id);
                                } else {
                                    let res = HttpResponse::Unauthorized()
                                        .json(ApiResponse::<()>::error("Invalid token", None));
                                    return Ok(req.into_response(res).map_into_right_body());
                                }
                            }
                            Err(_) => {
                                let res = HttpResponse::Unauthorized()
                                    .json(ApiResponse::<()>::error("Invalid token", None));
                                return Ok(req.into_response(res).map_into_right_body());
                            }
                        }
                    } else {
                        let res = HttpResponse::Unauthorized().json(ApiResponse::<()>::error(
                            "Invalid authorization header format",
                            None,
                        ));
                        return Ok(req.into_response(res).map_into_right_body());
                    }
                } else {
                    let res = HttpResponse::Unauthorized().json(ApiResponse::<()>::error(
                        "Invalid authorization header",
                        None,
                    ));
                    return Ok(req.into_response(res).map_into_right_body());
                }
            } else {
                let res = HttpResponse::Unauthorized().json(ApiResponse::<()>::error(
                    "Authorization header missing",
                    None,
                ));
                return Ok(req.into_response(res).map_into_right_body());
            }

            let res = service.call(req).await?;
            Ok(res.map_into_left_body())
        })
    }
}
