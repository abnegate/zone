#!/bin/bash
set -e

# Create the manager database if it doesn't exist
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" <<-EOSQL
    SELECT 'CREATE DATABASE manager'
    WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'manager')\gexec
EOSQL
