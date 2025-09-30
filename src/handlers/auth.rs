use actix_web::{web, HttpResponse, Result};
use sqlx::PgPool;
use std::env;
use validator::Validate;

use crate::models::{CreateUserRequest, LoginRequest, UserResponse, GoogleAuthRequest};
use crate::services::{AuthService, GoogleAuthService};
use crate::utils::response::ApiResponse;

pub async fn register(
    pool: web::Data<PgPool>,
    request: web::Json<CreateUserRequest>,
) -> Result<HttpResponse> {
    // Validate request
    if let Err(validation_errors) = request.validate() {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::error(
            "Validation failed",
            Some(validation_errors.field_errors()),
        )));
    }

    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    let auth_service = AuthService::new(pool.get_ref().clone(), jwt_secret);

    match auth_service.register(request.into_inner()).await {
        Ok(auth_response) => Ok(HttpResponse::Created().json(ApiResponse::success(auth_response))),
        Err(e) => {
            log::error!("Registration error: {}", e);
            Ok(HttpResponse::BadRequest().json(ApiResponse::error(
                &e.to_string(),
                None,
            )))
        }
    }
}

pub async fn login(
    pool: web::Data<PgPool>,
    request: web::Json<LoginRequest>,
) -> Result<HttpResponse> {
    // Validate request
    if let Err(validation_errors) = request.validate() {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::error(
            "Validation failed",
            Some(validation_errors.field_errors()),
        )));
    }

    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    let auth_service = AuthService::new(pool.get_ref().clone(), jwt_secret);

    match auth_service.login(request.into_inner()).await {
        Ok(auth_response) => Ok(HttpResponse::Ok().json(ApiResponse::success(auth_response))),
        Err(e) => {
            log::error!("Login error: {}", e);
            Ok(HttpResponse::Unauthorized().json(ApiResponse::error(
                "Invalid credentials",
                None,
            )))
        }
    }
}

pub async fn logout() -> Result<HttpResponse> {
    // For JWT tokens, logout is handled on the client side by removing the token
    // In a more sophisticated setup, you might maintain a blacklist of tokens
    Ok(HttpResponse::Ok().json(ApiResponse::success("Logged out successfully")))
}

pub async fn me(
    pool: web::Data<PgPool>,
    user_id: web::ReqData<uuid::Uuid>,
) -> Result<HttpResponse> {
    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    let auth_service = AuthService::new(pool.get_ref().clone(), jwt_secret);

    match auth_service.get_user_by_id(&user_id.into_inner()).await {
        Ok(Some(user)) => Ok(HttpResponse::Ok().json(ApiResponse::success(UserResponse::from(user)))),
        Ok(None) => Ok(HttpResponse::NotFound().json(ApiResponse::error(
            "User not found",
            None,
        ))),
        Err(e) => {
            log::error!("Get user error: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::error(
                "Internal server error",
                None,
            )))
        }
    }
}

pub async fn google_auth(
    pool: web::Data<PgPool>,
    request: web::Json<GoogleAuthRequest>,
) -> Result<HttpResponse> {
    let google_auth_service = GoogleAuthService::new(pool.get_ref().clone());

    // Verify Google token and get user info
    match google_auth_service.verify_google_token(&request.token).await {
        Ok(google_user) => {
            // Authenticate user with Google info
            match google_auth_service.authenticate_google_user(google_user).await {
                Ok(auth_response) => {
                    Ok(HttpResponse::Ok().json(ApiResponse::success(auth_response)))
                }
                Err(e) => {
                    log::error!("Google authentication error: {}", e);
                    Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                        "Authentication failed",
                        None,
                    )))
                }
            }
        }
        Err(e) => {
            log::error!("Google token verification error: {}", e);
            Ok(HttpResponse::Unauthorized().json(ApiResponse::<String>::error(
                "Invalid Google token",
                None,
            )))
        }
    }
}
