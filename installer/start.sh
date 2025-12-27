#!/bin/bash
set -e

echo "🌐 Starting Voiz Web Installer (Gleam Backend)..."

cd /build

# Use gleam run to properly start the application
exec gleam run
