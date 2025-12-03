# Server Setup Guide for Asistente Domingo

## Prerequisites

- Ubuntu 22.04+ server
- PostgreSQL 14+
- Nginx
- Cloudflare (with origin certificate)

## 1. Server Preparation

```bash
# Update system
sudo apt update && sudo apt upgrade -y

# Install required packages
sudo apt install -y nginx postgresql postgresql-contrib

# Create app directory
sudo mkdir -p /opt/domingo
sudo chown ubuntu:ubuntu /opt/domingo
```

## 2. PostgreSQL Setup

```bash
# Create database and user
sudo -u postgres psql << EOF
CREATE USER domingo WITH PASSWORD 'your_secure_password';
CREATE DATABASE asistente_domingo OWNER domingo;
GRANT ALL PRIVILEGES ON DATABASE asistente_domingo TO domingo;
EOF
```

## 3. Cloudflare Origin Certificate

1. Go to Cloudflare Dashboard → SSL/TLS → Origin Server
2. Create Certificate (let Cloudflare generate)
3. Save to server:

```bash
sudo mkdir -p /home/ubuntu/cert
# Paste origin certificate
sudo nano /home/ubuntu/cert/origin_cert.pem
# Paste private key
sudo nano /home/ubuntu/cert/origin_key.pem
sudo chmod 600 /home/ubuntu/cert/*.pem
```

## 4. Nginx Configuration

```bash
# Copy nginx config
sudo cp /opt/domingo/deploy/nginx-dev.conf /etc/nginx/nginx.conf

# Edit domain name
sudo nano /etc/nginx/nginx.conf
# Change: dev.domingo.example.com → your actual domain

# Test and reload
sudo nginx -t
sudo systemctl reload nginx
```

## 5. Systemd Service

```bash
# Copy service file
sudo cp /opt/domingo/deploy/domingo.service /etc/systemd/system/

# Reload systemd
sudo systemctl daemon-reload

# Enable service (start on boot)
sudo systemctl enable domingo
```

## 6. Environment Configuration

Create `/opt/domingo/.env`:

```bash
cat > /opt/domingo/.env << 'EOF'
# Server
HOST=127.0.0.1
PORT=3000
BASE_URL=https://your-domain.com

# Database
DATABASE_URL=postgres://domingo:your_password@localhost:5432/asistente_domingo

# Session (generate with: openssl rand -hex 32)
SESSION_SECRET=your-32-char-random-secret

# Twilio
TWILIO_ACCOUNT_SID=ACxxx
TWILIO_AUTH_TOKEN=xxx
TWILIO_PHONE_NUMBER=+1xxx

# OpenAI
OPENAI_API_KEY=sk-xxx

# Uber (optional)
UBER_CLIENT_ID=
UBER_CLIENT_SECRET=

# Stripe (optional)
STRIPE_SECRET_KEY=
STRIPE_WEBHOOK_SECRET=
STRIPE_PRICE_ID=

# Admin seed
SEED_ADMIN_EMAIL=admin@yourdomain.com
SEED_ADMIN_PASSWORD=secure-password
EOF

chmod 600 /opt/domingo/.env
```

## 7. Start Service

```bash
sudo systemctl start domingo
sudo systemctl status domingo
journalctl -u domingo -f  # Watch logs
```

## 8. Twilio Webhook Configuration

In Twilio Console, set your phone number's webhook to:
- **Voice URL**: `https://your-domain.com/api/twilio/voice` (POST)
- **Status Callback**: `https://your-domain.com/api/twilio/status` (POST)

## 9. GitHub Actions Secrets

In your GitHub repository settings, add:

**Secrets:**
- `EC2_SSH_KEY_DEV` - Your server's SSH private key

**Variables:**
- `EC2_HOST_DEV` - Server IP or hostname
- `EC2_USERNAME_DEV` - SSH username (usually `ubuntu`)

## Troubleshooting

### Check service status
```bash
sudo systemctl status domingo
journalctl -u domingo -n 100
```

### Check nginx
```bash
sudo nginx -t
sudo systemctl status nginx
tail -f /var/log/nginx/error.log
```

### Test database connection
```bash
psql -U domingo -d asistente_domingo -h localhost
```

### Test WebSocket (from server)
```bash
curl -v http://127.0.0.1:3000/debug/health
```

