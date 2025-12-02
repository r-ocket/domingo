# Walle 🤖

A voice AI assistant for elderly care. Walle enables seniors to request rides, receive medication reminders, and reach emergency contacts—all through natural phone conversations.

## Overview

Walle bridges the gap between elderly family members and modern technology. Instead of requiring smartphone apps, elders simply call a phone number and speak naturally. The AI assistant handles their requests and keeps caregivers informed.

### For Elders (Phone Interface)

Elders interact entirely via voice calls to a dedicated phone number:

- **🚗 Ride Requests** — "I need a ride to the doctor's office"
- **💊 Medication Info** — "What medications do I take in the morning?"
- **👥 Contact Family** — "Call my daughter" or "I need help"
- **🆘 Emergency Transfer** — Automatic transfer to emergency contacts when needed

No apps, no smartphones, no learning curve—just a phone call.

### For Caregivers (Web Dashboard)

Caregivers manage their elder's profile and monitor activity:

- **Elder Profile** — Set up phone number, timezone, language preferences
- **Contacts** — Add family, doctors, emergency contacts
- **Locations** — Save addresses for ride requests (home, pharmacy, clinic)
- **Medications** — Configure medication schedules and automated reminders
- **Activity Logs** — View call transcripts, ride history, reminder confirmations
- **Billing** — Manage subscription via Stripe

### For Admins (Web Dashboard)

System administrators have full visibility:

- **System Overview** — Total users, calls, active subscriptions
- **User Management** — View all caregivers and elders
- **System Logs** — All calls, rides, and reminders across the platform
- **Service Health** — API connection status

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Elder (Phone)                           │
│                              │                                  │
│                              ▼                                  │
│                     ┌────────────────┐                          │
│                     │     Twilio     │ ◄── Incoming calls       │
│                     │   Voice API    │ ──► Outbound reminders   │
│                     └───────┬────────┘                          │
│                             │ WebSocket (audio stream)          │
│                             ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                      Walle Server                        │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐   │   │
│  │  │  Handlers   │  │  Services   │  │   Repositories  │   │   │
│  │  │ (HTTP/WS)   │──│  (Business  │──│   (Postgres)    │   │   │
│  │  │             │  │   Logic)    │  │                 │   │   │
│  │  └─────────────┘  └─────────────┘  └─────────────────┘   │   │
│  │         │                │                               │   │
│  │         ▼                ▼                               │   │
│  │  ┌─────────────────────────────────────────────────┐     │   │
│  │  │              External Clients                   │     │   │
│  │  │  ┌─────────┐ ┌─────────┐ ┌──────┐ ┌─────────┐   │     │   │
│  │  │  │ OpenAI  │ │ Twilio  │ │ Uber │ │ Stripe  │   │     │   │
│  │  │  │Realtime │ │  REST   │ │ API  │ │   API   │   │     │   │
│  │  │  └─────────┘ └─────────┘ └──────┘ └─────────┘   │     │   │
│  │  └─────────────────────────────────────────────────┘     │   │
│  └──────────────────────────────────────────────────────────┘   │
│                             │                                   │
│                             ▼                                   │
│                     ┌────────────────┐                          │
│                     │   PostgreSQL   │                          │
│                     └────────────────┘                          │
│                                                                 │
│                         Caregiver ◄── Web Dashboard (htmx)      │
└─────────────────────────────────────────────────────────────────┘
```

### Tech Stack

| Layer | Technology |
|-------|------------|
| Language | Rust (2021 edition) |
| Web Framework | Axum |
| Database | PostgreSQL + tokio-postgres + deadpool |
| Migrations | Refinery |
| Templates | Tera + htmx + Alpine.js |
| Styling | Tailwind CSS |
| Voice AI | OpenAI Realtime API (GPT-4o) |
| Telephony | Twilio Voice + Media Streams |
| Rides | Uber API |
| Billing | Stripe |

### Project Structure

```
src/
├── main.rs              # Entry point, server setup
├── config.rs            # Environment configuration
├── migrations.rs        # Database migrations (refinery)
├── domain/              # Domain models (Elder, Contact, etc.)
├── services/            # Business logic
│   ├── call_service.rs
│   ├── ride_service.rs
│   ├── medication_service.rs
│   └── reminder_scheduler.rs
├── repositories/        # Database access
│   └── postgres/
├── clients/             # External API clients
│   ├── twilio.rs
│   ├── openai_realtime.rs
│   ├── uber.rs
│   └── stripe.rs
├── handlers/            # HTTP/WebSocket handlers
│   ├── auth.rs
│   ├── pages.rs
│   ├── twilio_webhooks.rs
│   ├── voice_stream.rs
│   └── ...
└── templates/           # Tera HTML templates
    ├── base.html
    ├── dashboard.html
    └── ...

migrations/              # SQL migration files
static/                  # CSS, JS assets
```

---

## Getting Started

### Prerequisites

- **Rust** (1.75+) — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **PostgreSQL** (14+) — `brew install postgresql` or via Docker
- **Node.js** (18+) — For Tailwind CSS build

### External Services

You'll need accounts and API keys for:

| Service | Purpose | Signup |
|---------|---------|--------|
| Twilio | Phone calls & SMS | https://twilio.com |
| OpenAI | Voice AI (Realtime API) | https://platform.openai.com |
| Uber | Ride requests | https://developer.uber.com |
| Stripe | Subscriptions & billing | https://stripe.com |

### Environment Variables

Create a `.env` file in the project root:

```bash
# Server
HOST=0.0.0.0
PORT=8080
BASE_URL=https://your-domain.com  # Public URL for webhooks

# Database
DATABASE_URL=postgres://user:password@localhost:5432/walle

# Session
SESSION_SECRET=your-32-char-random-secret-here

# Twilio
TWILIO_ACCOUNT_SID=ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
TWILIO_AUTH_TOKEN=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
TWILIO_PHONE_NUMBER=+1234567890

# OpenAI
OPENAI_API_KEY=sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

# Uber
UBER_CLIENT_ID=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
UBER_CLIENT_SECRET=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

# Stripe
STRIPE_SECRET_KEY=sk_test_xxxxxxxxxxxxxxxxxxxxxxxx
STRIPE_WEBHOOK_SECRET=whsec_xxxxxxxxxxxxxxxxxxxxxxxx

# Seed Admin (optional - creates on startup if DB empty)
SEED_ADMIN_EMAIL=admin@example.com
SEED_ADMIN_PASSWORD=your-secure-password
```

### Quick Start (Development)

The fastest way to get running locally:

```bash
# 1. Create database
createdb walle

# 2. Run (uses defaults, no .env needed for basic dev)
cargo run

# 3. Open http://localhost:3000/login
#    Default admin: admin@walle.local / admin123

# 4. Go to Admin > Cuidadores to create users
# 5. Go to Admin > Adultos Mayores to create elder profiles
```

On first run with an empty database, an admin user is automatically created:
- **Default**: `admin@walle.local` / `admin123`
- **Custom**: Set `SEED_ADMIN_EMAIL` and `SEED_ADMIN_PASSWORD` env vars

### Full Setup & Run

```bash
# 1. Clone the repository
git clone https://github.com/yourorg/walle.git
cd walle

# 2. Create the database
createdb walle

# 3. Install Node dependencies (for Tailwind)
npm install

# 4. Build CSS
npm run css:build

# 5. Run the server (migrations run automatically)
cargo run

# The server will start at http://localhost:3000
```

### Development

```bash
# Watch mode for CSS
npm run css:watch

# Run with auto-reload (requires cargo-watch)
cargo watch -x run

# Check for errors without building
cargo check
```

### Webhook Configuration

For local development, use a tunnel like ngrok:

```bash
ngrok http 8080
```

Then configure webhooks in your service dashboards:

| Service | Webhook URL |
|---------|-------------|
| Twilio Voice | `https://your-ngrok-url.ngrok.io/webhooks/twilio/voice` |
| Twilio Status | `https://your-ngrok-url.ngrok.io/webhooks/twilio/status` |
| Stripe | `https://your-ngrok-url.ngrok.io/webhooks/stripe` |
| Uber | `https://your-ngrok-url.ngrok.io/webhooks/uber` |

---

## Database Migrations

Migrations are managed with [refinery](https://github.com/rust-db/refinery) and run automatically on startup.

```bash
# Create a new migration
touch migrations/V2__add_something.sql

# Write your SQL, then rebuild
cargo build

# Migrations run automatically when the app starts
cargo run
```

Migration files must follow the naming pattern: `V{version}__{description}.sql`

---

## API Overview

### Webhook Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/webhooks/twilio/voice` | Incoming call handler |
| POST | `/webhooks/twilio/status` | Call status updates |
| GET | `/webhooks/twilio/media/:call_sid` | WebSocket for audio streaming |
| POST | `/webhooks/stripe` | Stripe events |
| POST | `/webhooks/uber` | Uber ride updates |

### Web Pages

| Path | Description |
|------|-------------|
| `/` | Landing page |
| `/login` | Caregiver login |
| `/register` | Caregiver registration |
| `/dashboard` | Caregiver dashboard |
| `/elder/profile` | Elder profile management |
| `/elder/contacts` | Manage contacts |
| `/elder/locations` | Manage saved locations |
| `/elder/medications` | Manage medications |
| `/elder/logs/calls` | Call history |
| `/elder/logs/rides` | Ride history |
| `/elder/logs/reminders` | Reminder history |
| `/billing` | Subscription management |
| `/admin` | Admin dashboard |
| `/admin/caregivers` | All caregivers |
| `/admin/elders` | All elders |
| `/admin/logs` | System logs |
| `/admin/debug` | Debug tools |

### Debug Endpoints (No Auth)

These endpoints are useful for development and testing:

| Method | Path | Description |
|--------|------|-------------|
| POST | `/debug/caregiver` | Create a caregiver `{name, email, password, role}` |
| POST | `/debug/elder` | Create an elder `{caregiver_id, name, phone_number, timezone}` |
| GET | `/debug/health` | Health check with DB/service status |

---

## Localization

The app is configured for **Spanish (Mexico)**:

- **Voice AI**: GPT-4o responds in Mexican Spanish
- **Twilio Voice**: Uses `Polly.Mia` (Amazon's Mexican Spanish voice)
- **Web UI**: All interfaces in Spanish
- **Timezones**: Mexican timezone options (CDMX, Cancún, Monterrey, Tijuana)

---

## Voice AI Tools

The AI assistant can invoke these tools during calls:

| Tool | Description |
|------|-------------|
| `get_medication_schedule` | Retrieves elder's medication info |
| `request_ride` | Books an Uber to a saved location |
| `get_ride_status` | Checks status of active ride |
| `list_contacts` | Lists available contacts |
| `transfer_to_contact` | Transfers call to a contact |
| `emergency_transfer` | Immediate transfer to emergency contact |

---

## License

MIT

