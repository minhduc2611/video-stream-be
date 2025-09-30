use crate::models::{GoogleUserInfo, User, CreateUserRequest, AuthResponse, UserResponse};
use sqlx::PgPool;
use anyhow::Result;
use reqwest;
use uuid::Uuid;

pub struct GoogleAuthService {
    pool: PgPool,
    client: reqwest::Client,
}

impl GoogleAuthService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            client: reqwest::Client::new(),
        }
    }

    pub async fn verify_google_token(&self, token: &str) -> Result<GoogleUserInfo> {
        let response = self.client
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

    pub async fn authenticate_google_user(&self, google_user: GoogleUserInfo) -> Result<AuthResponse> {
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
        let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
        let token = self.generate_jwt_token(&user.id, &jwt_secret)?;

        Ok(AuthResponse {
            user: user.into(),
            token,
        })
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

    fn generate_jwt_token(&self, user_id: &Uuid, jwt_secret: &str) -> Result<String> {
        use jsonwebtoken::{encode, Header, Algorithm, EncodingKey};
        use chrono::{Utc, Duration};
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

        let key = EncodingKey::from_secret(jwt_secret.as_ref());
        let token = encode(&Header::default(), &claims, &key)?;
        
        Ok(token)
    }
}
