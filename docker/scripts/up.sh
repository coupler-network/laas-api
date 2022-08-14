#!/usr/bin/env bash
docker-compose up --build -d
$(dirname $0)/setup.py
