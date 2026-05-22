FROM golang:1.24-alpine AS builder
ENV CGO_ENABLED=0
WORKDIR /backend
COPY backend/go.* .
RUN go mod download
COPY backend/. .
RUN go build -trimpath -ldflags="-s -w" -o bin/service

FROM --platform=$BUILDPLATFORM node:24-alpine AS client-builder
WORKDIR /ui
COPY ui/package.json ./
RUN npm install --legacy-peer-deps
COPY ui/. .
RUN npm run build

FROM alpine
LABEL org.opencontainers.image.title="MemoBuild - Incremental Build Farm" \
    com.docker.desktop.extension.api.version="0.4.2"

# Stub for memobuild
RUN echo '#!/bin/sh' > /usr/local/bin/memobuild && \
    echo 'echo "MemoBuild CLI (Stub)"' >> /usr/local/bin/memobuild && \
    chmod +x /usr/local/bin/memobuild

COPY --from=builder /backend/bin/service /
COPY docker-compose.yaml .
COPY metadata.json .
COPY docker.svg .
COPY --from=client-builder /ui/build ui
CMD /service -socket /run/guest-services/backend.sock
