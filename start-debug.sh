#!/bin/bash
set -e

docker compose -f debug-docker-compose.yml up -d --build reverse-proxy 
docker compose up -d nginx
