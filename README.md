# Video Stream Backend

A high-performance video streaming backend built with Rust and Actix Web.

## Features

- User authentication with JWT tokens
- Video upload with validation
- HLS video streaming support
- Video metadata management
- Thumbnail generation
- Rate limiting
- CORS support
- PostgreSQL database integration

## Prerequisites

- Rust 1.70+
- PostgreSQL 13+
- FFmpeg (for video processing)

## Installation

1. Clone the repository
2. Copy `env.example` to `.env` and configure your environment variables
3. Set up PostgreSQL database
4. Install dependencies:

```bash
cargo build
```

## Configuration

Create a `.env` file with the following variables:

```env
DATABASE_URL=postgresql://username:password@localhost:5432/video_stream_db
JWT_SECRET=your-super-secret-jwt-key-here
PORT=8080
UPLOAD_DIR=uploads
```

## Database Setup

1. Create a PostgreSQL database
2. Run migrations:

```bash
cargo sqlx migrate run
cargo sqlx prepare     
```

## Running the Server

```bash
cargo run
```

The server will start on `http://localhost:8080`


gcloud config set billing/quota_project video-streaming-473721

## API Endpoints

### Authentication
- `POST /api/v1/auth/register` - Register a new user
- `POST /api/v1/auth/login` - Login user
- `POST /api/v1/auth/logout` - Logout user
- `GET /api/v1/auth/me` - Get current user info

### Videos
- `GET /api/v1/videos` - List user's videos
- `POST /api/v1/videos` - Upload a new video
- `GET /api/v1/videos/{id}` - Get video details
- `GET /api/v1/videos/{id}/stream` - Get video streaming URL
- `GET /api/v1/videos/{id}/thumbnail` - Get video thumbnail
- `DELETE /api/v1/videos/{id}` - Delete video

### Health
- `GET /api/v1/health` - Health check endpoint

## Development

### Project Structure

```
src/
├── handlers/          # HTTP request handlers
├── middleware/        # Custom middleware
├── models/           # Data models
├── services/         # Business logic
└── utils/            # Utility functions
```

### Adding New Features

1. Create models in `src/models/`
2. Implement business logic in `src/services/`
3. Add HTTP handlers in `src/handlers/`
4. Update routes in `src/main.rs`

## Testing

```bash
cargo test
```

## Deployment
```
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t us-central1-docker.pkg.dev/video-streaming-473721/streaming-backend/video-stream-be:latest \
  --push \
  /Users/ducle/Desktop/streaming/video-stream-be


# Authenticate docker with Google Cloud
gcloud auth login
gcloud config set project video-streaming-473721
gcloud auth configure-docker us-central1-docker.pkg.dev
# Create a repository (skip if it already exists)
gcloud artifacts repositories create streaming-backend --repository-format=docker --location=us-central1 --description="Video stream backend"
# Push
docker push us-central1-docker.pkg.dev/video-streaming-473721/streaming-backend/video-stream-be:latest
# Verify
gcloud artifacts docker images list us-central1-docker.pkg.dev/video-streaming-473721/streaming-backend
```
The application is designed to be deployed with Docker. See the Dockerfile for containerization details.

## License

MIT

