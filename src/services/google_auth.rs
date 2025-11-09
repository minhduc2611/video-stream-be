use crate::models::{AuthResponse, CreateUserRequest, GoogleUserInfo, User};
use anyhow::Result;
use async_trait::async_trait;
use reqwest;
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait GoogleAuthServiceTrait: Send + Sync {
    async fn verify_google_token(&self, token: &str) -> Result<GoogleUserInfo>;
    async fn authenticate_google_user(&self, google_user: GoogleUserInfo) -> Result<AuthResponse>;
}

pub struct GoogleAuthService {
    pool: PgPool,
    client: reqwest::Client,
    jwt_secret: String,
}

impl GoogleAuthService {
    pub fn new(pool: PgPool, jwt_secret: String) -> Self {
        Self {
            pool,
            client: reqwest::Client::new(),
            jwt_secret,
        }
    }

    fn generate_jwt_token(&self, user_id: &Uuid) -> Result<String> {
        use chrono::{Duration, Utc};
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Serialize, Deserialize)]
        struct Claims {
            sub: String, // user id
            exp: usize,
            iat: usize,
        }

        let now = Utc::now();
        let exp = now + Duration::days(7); // Token expires in 7 days

        let claims = Claims {
            sub: user_id.to_string(),
            exp: exp.timestamp() as usize,
            iat: now.timestamp() as usize,
        };

        let key = EncodingKey::from_secret(self.jwt_secret.as_ref());
        let token = encode(&Header::default(), &claims, &key)?;

        Ok(token)
    }

    fn generate_username_from_email(&self, email: &str) -> String {
        let username = email.split('@').next().unwrap_or("user");
        let clean_username = username.replace('.', "_").replace('+', "_");
        format!("{}_google", clean_username)
    }

    fn generate_random_password(&self) -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();

        (0..32)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }
}

#[async_trait]
impl GoogleAuthServiceTrait for GoogleAuthService {
    async fn verify_google_token(&self, token: &str) -> Result<GoogleUserInfo> {
        let response = self
            .client
            .get("https://www.googleapis.com/oauth2/v2/userinfo")
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        if response.status().is_success() {
            let user_info: GoogleUserInfo = response.json().await?;
            Ok(user_info)
        } else {
            Err(anyhow::anyhow!("Invalid Google token"))
        }
    }

    async fn authenticate_google_user(&self, google_user: GoogleUserInfo) -> Result<AuthResponse> {
        // Check if user already exists
        let existing_user = sqlx::query_as!(
            User,
            "SELECT * FROM users WHERE email = $1",
            google_user.email
        )
        .fetch_optional(&self.pool)
        .await?;

        let user = if let Some(user) = existing_user {
            user
        } else {
            // Create new user from Google info
            let create_request = CreateUserRequest {
                email: google_user.email.clone(),
                username: self.generate_username_from_email(&google_user.email),
                password: self.generate_random_password(), // We'll use a random password since Google handles auth
            };

            // Hash the random password
            let password_hash = bcrypt::hash(&create_request.password, bcrypt::DEFAULT_COST)?;

            sqlx::query_as!(
                User,
                "INSERT INTO users (email, username, password_hash) VALUES ($1, $2, $3) RETURNING *",
                create_request.email,
                create_request.username,
                password_hash
            )
            .fetch_one(&self.pool)
            .await?
        };

        // Generate JWT token
        let token = self.generate_jwt_token(&user.id)?;

        Ok(AuthResponse {
            user: user.into(),
            token,
        })
    }
}
