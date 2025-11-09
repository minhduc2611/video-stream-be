use actix_web::{web, HttpResponse, Result};
use std::sync::Arc;
use validator::Validate;

use crate::app_state::AppState;
use crate::models::{
    AuthResponse, CreateUserRequest, GoogleAuthRequest, LoginRequest, UserResponse,
};
use crate::services::{AuthServiceTrait, GoogleAuthServiceTrait};
use crate::utils::response::ApiResponse;

pub async fn register(
    app_state: web::Data<AppState>,
    request: web::Json<CreateUserRequest>,
) -> Result<HttpResponse> {
    // Validate request
    if let Err(validation_errors) = request.validate() {
        return Ok(
            HttpResponse::BadRequest().json(ApiResponse::<AuthResponse>::error(
                "Validation failed",
                Some(
                    validation_errors
                        .field_errors()
                        .into_iter()
                        .map(|(k, v)| {
                            (
                                k.to_string(),
                                v.into_iter().map(|e| e.to_string()).collect(),
                            )
                        })
                        .collect(),
                ),
            )),
        );
    }

    let auth_service = Arc::clone(&app_state.auth_service);

    match auth_service.register(request.into_inner()).await {
        Ok(auth_response) => Ok(HttpResponse::Created().json(ApiResponse::success(auth_response))),
        Err(e) => {
            log::error!("Registration error: {}", e);
            Ok(HttpResponse::BadRequest()
                .json(ApiResponse::<AuthResponse>::error(&e.to_string(), None)))
        }
    }
}

pub async fn login(
    app_state: web::Data<AppState>,
    request: web::Json<LoginRequest>,
) -> Result<HttpResponse> {
    // Validate request
    if let Err(validation_errors) = request.validate() {
        return Ok(
            HttpResponse::BadRequest().json(ApiResponse::<UserResponse>::error(
                "Validation failed",
                Some(
                    validation_errors
                        .field_errors()
                        .into_iter()
                        .map(|(k, v)| {
                            (
                                k.to_string(),
                                v.into_iter().map(|e| e.to_string()).collect(),
                            )
                        })
                        .collect(),
                ),
            )),
        );
    }

    let auth_service = Arc::clone(&app_state.auth_service);

    match auth_service.login(request.into_inner()).await {
        Ok(auth_response) => Ok(HttpResponse::Ok().json(ApiResponse::success(auth_response))),
        Err(e) => {
            log::error!("Login error: {}", e);
            Ok(
                HttpResponse::Unauthorized().json(ApiResponse::<UserResponse>::error(
                    "Invalid credentials",
                    None,
                )),
            )
        }
    }
}

pub async fn logout() -> Result<HttpResponse> {
    // For JWT tokens, logout is handled on the client side by removing the token
    // In a more sophisticated setup, you might maintain a blacklist of tokens
    Ok(HttpResponse::Ok().json(ApiResponse::success("Logged out successfully")))
}

pub async fn me(
    app_state: web::Data<AppState>,
    user_id: web::ReqData<uuid::Uuid>,
) -> Result<HttpResponse> {
    log::info!("User ID 1: {:?}", user_id.clone().into_inner());
    let auth_service = Arc::clone(&app_state.auth_service);
    log::info!("User ID 2: {}", user_id.clone().into_inner());
    match auth_service.get_user_by_id(&user_id.into_inner()).await {
        Ok(Some(user)) => {
            log::info!("User: {:?}", user);
            Ok(HttpResponse::Ok().json(ApiResponse::success(UserResponse::from(user))))
        }
        Ok(None) => Ok(HttpResponse::NotFound()
            .json(ApiResponse::<UserResponse>::error("User not found", None))),
        Err(e) => {
            log::error!("Get user error: {}", e);
            Ok(
                HttpResponse::InternalServerError().json(ApiResponse::<UserResponse>::error(
                    "Internal server error",
                    None,
                )),
            )
        }
    }
}

pub async fn google_auth(
    app_state: web::Data<AppState>,
    request: web::Json<GoogleAuthRequest>,
) -> Result<HttpResponse> {
    let google_auth_service = Arc::clone(&app_state.google_auth_service);

    // Verify Google token and get user info
    match google_auth_service
        .verify_google_token(&request.token)
        .await
    {
        Ok(google_user) => {
            // Authenticate user with Google info
            match google_auth_service
                .authenticate_google_user(google_user)
                .await
            {
                Ok(auth_response) => {
                    Ok(HttpResponse::Ok().json(ApiResponse::success(auth_response)))
                }
                Err(e) => {
                    log::error!("Google authentication error: {}", e);
                    Ok(HttpResponse::InternalServerError()
                        .json(ApiResponse::<String>::error("Authentication failed", None)))
                }
            }
        }
        Err(e) => {
            log::error!("Google token verification error: {}", e);
            Ok(HttpResponse::Unauthorized()
                .json(ApiResponse::<String>::error("Invalid Google token", None)))
        }
    }
}
