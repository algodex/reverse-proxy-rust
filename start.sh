#!/bin/bash
set -e

if [ ! -d algodex-service ]; then
  git clone git@github.com:algodex/algodex-service.git
fi
docker compose up -d --build reverse-proxy
