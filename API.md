# Video Stream API Documentation

## Overview
This API handles HLS video streaming with support for uploading multiple HLS files (.m3u8 and .ts segments) and serving them for streaming.

## Base URL
```
http://localhost:8080/api/v1
```

## Authentication
All video endpoints require authentication via JWT token in the Authorization header:
```
Authorization: Bearer <jwt_token>
```

## Endpoints

### Authentication

#### Register User
```http
POST /auth/register
Content-Type: application/json

{
  "email": "user@example.com",
  "username": "username",
  "password": "password123"
}
```

#### Login User
```http
POST /auth/login
Content-Type: application/json

{
  "email": "user@example.com",
  "password": "password123"
}
```

Response:
```json
{
  "success": true,
  "data": {
    "user": {
      "id": "uuid",
      "email": "user@example.com",
      "username": "username",
      "created_at": "2024-01-01T00:00:00Z"
    },
    "token": "jwt_token_here"
  }
}
```

### Video Management

#### Upload HLS Video Files
```http
POST /videos
Content-Type: multipart/form-data
Authorization: Bearer <jwt_token>

Form fields:
- title: Video title
- description: Video description (optional)
- files: Multiple HLS files (.m3u8 and .ts files)
```

**Important**: 
- Must include a master playlist file named `playlist.m3u8` or `master.m3u8`
- All .ts segment files must be included
- Files are stored in organized directory structure: `uploads/hls/{video_id}/`

Response:
```json
{
  "success": true,
  "data": {
    "video_id": "uuid",
    "title": "Video Title",
    "description": "Video description",
    "status": "ready",
    "hls_files_count": 15,
    "total_size": 52428800,
    "created_at": "2024-01-01T00:00:00Z"
  }
}
```

#### List Videos
```http
GET /videos?limit=10&offset=0
Authorization: Bearer <jwt_token>
```

Response:
```json
{
  "success": true,
  "data": {
    "data": [
      {
        "id": "uuid",
        "title": "Video Title",
        "description": "Video description",
        "filename": "playlist.m3u8",
        "file_size": 52428800,
        "duration": 120,
        "thumbnail_path": "thumbnails/uuid.jpg",
        "hls_playlist_path": "hls/uuid/playlist.m3u8",
        "hls_stream_url": "/api/v1/videos/uuid/stream/playlist.m3u8",
        "thumbnail_url": "/api/v1/videos/uuid/thumbnail",
        "status": "ready",
        "user_id": "uuid",
        "created_at": "2024-01-01T00:00:00Z",
        "updated_at": "2024-01-01T00:00:00Z"
      }
    ],
    "pagination": {
      "total": 1,
      "limit": 10,
      "offset": 0,
      "current_page": 1,
      "total_pages": 1,
      "has_next": false,
      "has_previous": false
    }
  }
}
```

#### Get Video Details
```http
GET /videos/{video_id}
Authorization: Bearer <jwt_token>
```

#### Get Video Streaming Info
```http
GET /videos/{video_id}/stream
Authorization: Bearer <jwt_token>
```

Response:
```json
{
  "success": true,
  "data": {
    "video_id": "uuid",
    "hls_url": "/api/v1/videos/uuid/stream/playlist.m3u8",
    "thumbnail_url": "/api/v1/videos/uuid/thumbnail",
    "status": "ready",
    "title": "Video Title",
    "duration": 120
  }
}
```

#### Serve HLS Files
```http
GET /videos/{video_id}/stream/{filename}
Authorization: Bearer <jwt_token>
```

This endpoint serves the actual HLS files (.m3u8 and .ts files) with proper content types:
- `.m3u8` files: `application/vnd.apple.mpegurl`
- `.ts` files: `video/mp2t`

The frontend can use these URLs directly with HLS.js or native HTML5 video elements.

#### Get Video Thumbnail
```http
GET /videos/{video_id}/thumbnail
Authorization: Bearer <jwt_token>
```

#### Delete Video
```http
DELETE /videos/{video_id}
Authorization: Bearer <jwt_token>
```

### Health Check
```http
GET /health
```

## Frontend Integration

### Uploading HLS Files
```javascript
const formData = new FormData();
formData.append('title', 'My Video');
formData.append('description', 'Video description');
formData.append('files', masterPlaylistFile); // playlist.m3u8
segmentFiles.forEach(file => {
  formData.append('files', file); // .ts files
});

const response = await fetch('/api/v1/videos', {
  method: 'POST',
  headers: {
    'Authorization': `Bearer ${token}`
  },
  body: formData
});
```

### Streaming HLS Videos
```javascript
// Get streaming info
const streamResponse = await fetch(`/api/v1/videos/${videoId}/stream`, {
  headers: {
    'Authorization': `Bearer ${token}`
  }
});

const { data } = await streamResponse.json();
const hlsUrl = `http://localhost:8080${data.hls_url}`;

// Use with HLS.js
if (Hls.isSupported()) {
  const video = document.getElementById('video');
  const hls = new Hls();
  hls.loadSource(hlsUrl);
  hls.attachMedia(video);
}
```

## File Structure
```
uploads/
├── hls/
│   └── {video_id}/
│       ├── playlist.m3u8
│       ├── segment_001.ts
│       ├── segment_002.ts
│       └── ...
└── thumbnails/
    └── {video_id}.jpg
```

## Error Responses
All error responses follow this format:
```json
{
  "success": false,
  "error": "Error message",
  "validation_errors": {
    "field": ["validation error message"]
  }
}
```

## Status Codes
- `200` - Success
- `201` - Created
- `400` - Bad Request
- `401` - Unauthorized
- `404` - Not Found
- `422` - Unprocessable Entity
- `429` - Too Many Requests
- `500` - Internal Server Error
