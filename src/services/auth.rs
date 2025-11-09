use crate::models::{AuthResponse, CreateUserRequest, LoginRequest, User};
use anyhow::Result;
use async_trait::async_trait;
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user id
    pub exp: usize,
    pub iat: usize,
}

#[async_trait]
pub trait AuthServiceTrait: Send + Sync {
    async fn register(&self, request: CreateUserRequest) -> Result<AuthResponse>;
    async fn login(&self, request: LoginRequest) -> Result<AuthResponse>;
    async fn get_user_by_id(&self, user_id: &Uuid) -> Result<Option<User>>;
    fn verify_token(&self, token: &str) -> Result<Claims>;
}

pub struct AuthService {
    pool: PgPool,
    jwt_secret: String,
}

impl AuthService {
    pub fn new(pool: PgPool, jwt_secret: String) -> Self {
        Self { pool, jwt_secret }
    }

    fn generate_token(&self, user_id: &Uuid) -> Result<String> {
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
}

#[async_trait]
impl AuthServiceTrait for AuthService {
    async fn register(&self, request: CreateUserRequest) -> Result<AuthResponse> {
        // Check if user already exists
        let existing_user = sqlx::query_as!(
            User,
            "SELECT * FROM users WHERE email = $1 OR username = $2",
            request.email,
            request.username
        )
        .fetch_optional(&self.pool)
        .await?;

        if existing_user.is_some() {
            return Err(anyhow::anyhow!(
                "User with this email or username already exists"
            ));
        }

        // Hash password
        let password_hash = hash(&request.password, DEFAULT_COST)?;

        // Create user
        let user = sqlx::query_as!(
            User,
            "INSERT INTO users (email, username, password_hash) VALUES ($1, $2, $3) RETURNING *",
            request.email,
            request.username,
            password_hash
        )
        .fetch_one(&self.pool)
        .await?;

        // Generate JWT token
        let token = self.generate_token(&user.id)?;

        Ok(AuthResponse {
            user: user.into(),
            token,
        })
    }

    async fn login(&self, request: LoginRequest) -> Result<AuthResponse> {
        // Find user by email
        let user = sqlx::query_as!(User, "SELECT * FROM users WHERE email = $1", request.email)
            .fetch_optional(&self.pool)
            .await?;

        let user = user.ok_or_else(|| anyhow::anyhow!("Invalid credentials"))?;

        // Verify password
        if !verify(&request.password, &user.password_hash)? {
            return Err(anyhow::anyhow!("Invalid credentials"));
        }

        // Generate JWT token
        let token = self.generate_token(&user.id)?;

        Ok(AuthResponse {
            user: user.into(),
            token,
        })
    }

    async fn get_user_by_id(&self, user_id: &Uuid) -> Result<Option<User>> {
        let user = sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", user_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(user)
    }

    fn verify_token(&self, token: &str) -> Result<Claims> {
        let key = DecodingKey::from_secret(self.jwt_secret.as_ref());
        let validation = Validation::new(Algorithm::HS256);

        let token_data = decode::<Claims>(token, &key, &validation)?;
        Ok(token_data.claims)
    }
}
