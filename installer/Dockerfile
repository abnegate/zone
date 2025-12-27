FROM python:3.12-slim

# Install htpasswd utility
RUN apt-get update && \
    apt-get install -y --no-install-recommends apache2-utils docker.io && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install Python dependencies
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy application
COPY app.py .
COPY templates/ templates/
COPY static/ static/

# Run with gunicorn
CMD ["gunicorn", "--bind", "0.0.0.0:8000", "--workers", "1", "--timeout", "120", "--access-logfile", "-", "app:app"]
