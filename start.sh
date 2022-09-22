#!/bin/bash
set -e

docker compose up -d --build reverse-proxy
docker compose up -d nginx
