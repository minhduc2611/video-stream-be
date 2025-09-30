# Use the official Rust image as the base image
FROM rust:1.70-slim as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

# Set the working directory
WORKDIR /app

# Copy the Cargo.toml and Cargo.lock files
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (this layer will be cached if Cargo.toml doesn't change)
RUN cargo build --release

# Remove the dummy main.rs
RUN rm src/main.rs

# Copy the source code
COPY src ./src
COPY migrations ./migrations

# Build the application
RUN cargo build --release

# Create the runtime image
FROM debian:bullseye-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -m -u 1000 appuser

# Set the working directory
WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /app/target/release/video-stream-be /app/video-stream-be

# Copy migrations
COPY --from=builder /app/migrations ./migrations

# Create uploads directory
RUN mkdir -p uploads && chown -R appuser:appuser /app

# Switch to the non-root user
USER appuser

# Expose the port
EXPOSE 8080

# Set environment variables
ENV RUST_LOG=info
ENV PORT=8080

# Run the application
CMD ["./video-stream-be"]
