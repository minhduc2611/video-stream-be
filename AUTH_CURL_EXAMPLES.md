# Authentication CURL Examples

This document provides CURL examples for all authentication endpoints in the Video Stream API.

## Base URL
```
http://localhost:8080/api/v1
```

## Authentication Endpoints

### 1. Register User

**Endpoint:** `POST /auth/register`

**Description:** Create a new user account

**Request:**
```bash
curl -X POST http://localhost:8080/api/v1/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "email": "user@example.com",
    "username": "testuser",
    "password": "password123"
  }'
```

**Response (201 Created):**
```json
{
  "success": true,
  "data": {
    "user": {
      "id": "123e4567-e89b-12d3-a456-426614174000",
      "email": "user@example.com",
      "username": "testuser",
      "created_at": "2024-01-01T12:00:00Z"
    },
    "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
  }
}
```

**Error Response (400 Bad Request):**
```json
{
  "success": false,
  "error": "Validation failed",
  "validation_errors": {
    "email": ["email field must contain a valid email address"],
    "password": ["password must be at least 8 characters long"]
  }
}
```

---

### 2. Login User

**Endpoint:** `POST /auth/login`

**Description:** Authenticate user and get JWT token

**Request:**
```bash
curl -X POST http://localhost:8080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "email": "user@example.com",
    "password": "password123"
  }'
```

**Response (200 OK):**
```json
{
  "success": true,
  "data": {
    "user": {
      "id": "123e4567-e89b-12d3-a456-426614174000",
      "email": "user@example.com",
      "username": "testuser",
      "created_at": "2024-01-01T12:00:00Z"
    },
    "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
  }
}
```

**Error Response (401 Unauthorized):**
```json
{
  "success": false,
  "error": "Invalid credentials"
}
```

---

### 3. Google Authentication

**Endpoint:** `POST /auth/google`

**Description:** Authenticate user using Google OAuth token

**Request:**
```bash
curl -X POST http://localhost:8080/api/v1/auth/google \
  -H "Content-Type: application/json" \
  -d '{
    "token": "ya29.a0AfH6SMC..."
  }'
```

**Response (200 OK):**
```json
{
  "success": true,
  "data": {
    "user": {
      "id": "123e4567-e89b-12d3-a456-426614174000",
      "email": "user@gmail.com",
      "username": "user",
      "created_at": "2024-01-01T12:00:00Z"
    },
    "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
  }
}
```

**Error Response (401 Unauthorized):**
```json
{
  "success": false,
  "error": "Invalid Google token"
}
```

---

### 4. Get Current User

**Endpoint:** `GET /auth/me`

**Description:** Get current authenticated user information

**Request:**
```bash
curl -X GET http://localhost:8080/api/v1/auth/me \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
```

**Response (200 OK):**
```json
{
  "success": true,
  "data": {
    "id": "123e4567-e89b-12d3-a456-426614174000",
    "email": "user@example.com",
    "username": "testuser",
    "created_at": "2024-01-01T12:00:00Z"
  }
}
```

**Error Response (401 Unauthorized):**
```json
{
  "success": false,
  "error": "Invalid token"
}
```

**Error Response (404 Not Found):**
```json
{
  "success": false,
  "error": "User not found"
}
```

---

### 5. Logout

**Endpoint:** `POST /auth/logout`

**Description:** Logout user (client-side token removal)

**Request:**
```bash
curl -X POST http://localhost:8080/api/v1/auth/logout \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
```

**Response (200 OK):**
```json
{
  "success": true,
  "data": "Logged out successfully"
}
```

---

## Authentication Headers

For protected endpoints, include the JWT token in the Authorization header:

```bash
-H "Authorization: Bearer <your_jwt_token>"
```

## Testing with Environment Variables

You can store your JWT token in an environment variable for easier testing:

```bash
# Set the token
export JWT_TOKEN="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."

# Use in requests
curl -X GET http://localhost:8080/api/v1/auth/me \
  -H "Authorization: Bearer $JWT_TOKEN"
```

## Complete Authentication Flow Example

```bash
# 1. Register a new user
curl -X POST http://localhost:8080/api/v1/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "email": "newuser@example.com",
    "username": "newuser",
    "password": "securepassword123"
  }'

# 2. Login to get token (if registration doesn't return token)
curl -X POST http://localhost:8080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "email": "newuser@example.com",
    "password": "securepassword123"
  }'

# 3. Store the token from response
export JWT_TOKEN="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."

# 4. Use the token for protected endpoints
curl -X GET http://localhost:8080/api/v1/auth/me \
  -H "Authorization: Bearer $JWT_TOKEN"

# 5. Logout when done
curl -X POST http://localhost:8080/api/v1/auth/logout \
  -H "Authorization: Bearer $JWT_TOKEN"
```

## Error Handling

All authentication endpoints return consistent error responses:

- **400 Bad Request**: Validation errors or malformed requests
- **401 Unauthorized**: Invalid credentials or expired token
- **404 Not Found**: User not found (for `/auth/me` endpoint)
- **500 Internal Server Error**: Server-side errors

## Validation Rules

### Registration (`/auth/register`):
- `email`: Must be a valid email address
- `username`: Must be 3-50 characters long
- `password`: Must be at least 8 characters long

### Login (`/auth/login`):
- `email`: Must be a valid email address
- `password`: Required field

### Google Auth (`/auth/google`):
- `token`: Must be a valid Google OAuth token

## Notes

1. **JWT Tokens**: All authenticated endpoints require a valid JWT token in the Authorization header
2. **CORS**: The API supports CORS for cross-origin requests
3. **Logout**: Currently handled client-side by removing the token (no server-side token blacklisting)
4. **Google Auth**: Requires valid Google OAuth configuration and client credentials
5. **Password Security**: Passwords are hashed using bcrypt before storage
