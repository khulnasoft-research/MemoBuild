# Build Stage
FROM golang:1.21-alpine AS builder

WORKDIR /app

# Copy the go.mod first to cache dependency downloads
COPY go.mod ./
# RUN go mod download (If we had dependencies)

# Copy the source code
COPY main.go ./

# Build the executable
RUN CGO_ENABLED=0 GOOS=linux go build -o /microservice main.go

# Final Stage (Minimal scratch image)
FROM scratch

# Copy only the compiled binary from the builder stage
COPY --from=builder /microservice /microservice

# Expose the port the app listens on
EXPOSE 8080

# Run the binary
ENTRYPOINT ["/microservice"]
