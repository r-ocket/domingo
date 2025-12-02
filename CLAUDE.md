# claude.md

design doc for a rust/axum backend using clean architecture, postgres, and tera templates with htmx + alpine.js.

this is for the “elderly voice assistant” product, but the structure is intentionally generic.

docs/ folder at root for docs, including the spec.md Spec for the platform

---

## 0. stack + goals

**stack**

- language: rust (edition 2021+)
- web framework: `axum`
- templates: `tera`
- frontend sprinkles: `htmx`, `alpine.js`, `tailwindcss`
- db: postgres
- db access: `tokio-postgres` + `deadpool-postgres` (simple sql, no orm)
- http client: `reqwest`
- config: `figment` or `config` crate + `dotenvy`
- middleware: `tower` + `tower-http` (trace, compression, auth, etc.)

**goals**

- clear separation of:
  - `domain` (business rules)
  - `application` (use cases)
  - `infrastructure` (db, external apis)
  - `interfaces` / `presentation` (http / templates)
- minimal magic:
  - raw sql, explicit repos
  - thin controllers, fat services
- explicit context + auth via tower layers
- simple env-driven configuration

---

## 1. crate layout

top-level crate, no workspace unless you really want to split later.

```text
src/
  main.rs
  config.rs

  domain/
    mod.rs
    caregiver.rs
    elder.rs
    contact.rs
    location.rs
    medication.rs
    reminder.rs
    ride.rs
    call_session.rs
    subscription.rs

  application/
    mod.rs
    ports.rs            # traits for repos + external services
    services/
      mod.rs
      caregiver_service.rs
      elder_service.rs
      contacts_service.rs
      locations_service.rs
      medications_service.rs
      rides_service.rs
      calls_service.rs
      billing_service.rs

  infrastructure/
    mod.rs
    db/
      mod.rs
      pool.rs           # deadpool setup
      caregiver_repo.rs
      elder_repo.rs
      contact_repo.rs
      location_repo.rs
      medication_repo.rs
      reminder_repo.rs
      ride_repo.rs
      call_session_repo.rs
      subscription_repo.rs
    external/
      mod.rs
      twilio_client.rs
      openai_client.rs
      uber_client.rs
      stripe_client.rs

  interfaces/
    mod.rs
    http/
      mod.rs
      routes.rs
      middleware.rs
      extractors.rs
      view_models.rs
      controllers/
        mod.rs
        auth_controller.rs
        caregiver_dashboard_controller.rs
        elder_controller.rs
        contacts_controller.rs
        locations_controller.rs
        medications_controller.rs
        logs_controller.rs
        admin_controller.rs
        webhooks/
          mod.rs
          twilio_controller.rs
          stripe_controller.rs
          uber_controller.rs

  templates/
    layout.html.tera
    components/
      nav.html.tera
      flash.html.tera
    caregiver/
      dashboard.html.tera
      contacts.html.tera
      locations.html.tera
      medications.html.tera
      logs_calls.html.tera
    admin/
      dashboard.html.tera

  static/
    css/
      output.css          # tailwind compiled output
    js/
      alpine-init.js
      htmx-init.js
  tailwind.config.js      # tailwind configuration
  input.css               # tailwind input with @tailwind directives

migrations/
  001_init.sql
  002_add_medications.sql
  ...


⸻

2. config + env

2.1 config struct

// src/config.rs
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub env: String,                  // "local", "staging", "prod"
    pub http_addr: String,            // "0.0.0.0:8080"
    pub database_url: String,         // postgres://...
    pub session_secret: String,       // 32+ bytes
    pub twilio_auth_token: String,
    pub twilio_account_sid: String,
    pub twilio_phone_number: String,
    pub openai_api_key: String,
    pub uber_client_id: String,
    pub uber_client_secret: String,
    pub stripe_secret_key: String,
    pub stripe_webhook_secret: String,
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        // use `config` crate + dotenvy
        let mut c = config::Config::builder()
            .add_source(config::Environment::default().separator("__"))
            .build()?;
        Ok(c.try_deserialize()?)
    }

    pub fn is_prod(&self) -> bool {
        self.env == "prod"
    }
}

env variables (example):

APP__ENV=local
APP__HTTP_ADDR=0.0.0.0:8080
APP__DATABASE_URL=postgres://...
APP__SESSION_SECRET=super-long-random-string
APP__TWILIO_AUTH_TOKEN=...
APP__TWILIO_ACCOUNT_SID=...
APP__TWILIO_PHONE_NUMBER=+1...
APP__OPENAI_API_KEY=...
APP__UBER_CLIENT_ID=...
APP__UBER_CLIENT_SECRET=...
APP__STRIPE_SECRET_KEY=...
APP__STRIPE_WEBHOOK_SECRET=...


⸻

3. app state & context

3.1 app state

// src/infrastructure/db/pool.rs
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use tokio_postgres::{Config as PgConfig, NoTls};

pub type DbPool = Pool;

pub fn create_pool(database_url: &str) -> anyhow::Result<DbPool> {
    let pg_config: PgConfig = database_url.parse()?;
    let mgr = Manager::from_config(pg_config, NoTls, ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    Ok(Pool::builder(mgr).max_size(16).build().unwrap())
}

// src/main.rs (core pieces)
use crate::config::AppConfig;
use crate::infrastructure::db::pool::create_pool;
use crate::interfaces::http::routes::build_router;
use tera::Tera;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db_pool: DbPool,
    pub templates: Tera,
    pub http_client: reqwest::Client,
    pub twilio: TwilioClient,
    pub openai: OpenAiClient,
    pub uber: UberClient,
    pub stripe: StripeClient,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let config = AppConfig::from_env()?;

    let db_pool = create_pool(&config.database_url)?;
    let tera = Tera::new("templates/**/*")?;
    let http_client = reqwest::Client::builder()
        .user_agent("elder-voice-assistant/1.0")
        .build()?;

    let twilio = TwilioClient::new(&config.twilio_account_sid, &config.twilio_auth_token)?;
    let openai = OpenAiClient::new(&config.openai_api_key, http_client.clone());
    let uber = UberClient::new(&config.uber_client_id, &config.uber_client_secret, http_client.clone());
    let stripe = StripeClient::new(&config.stripe_secret_key, http_client.clone());

    let state = AppState {
        config,
        db_pool,
        templates: tera,
        http_client,
        twilio,
        openai,
        uber,
        stripe,
    };

    let app = build_router(state.clone());

    axum::Server::bind(&state.config.http_addr.parse()?)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

3.2 request context

we want per-request context: current user, elder, maybe hx flags.

// src/interfaces/http/extractors.rs
use axum::{
    async_trait,
    extract::{FromRequestParts, State},
    http::request::Parts,
};
use crate::AppState;
use crate::domain::caregiver::Caregiver;
use crate::domain::caregiver::Role;

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub caregiver: Option<Caregiver>,
    pub role: Role, // caregiver | admin | anonymous
    pub is_htmx: bool,
}

#[async_trait]
impl<S> FromRequestParts<S> for RequestContext
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = axum::response::Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        // 1. detect htmx
        let is_htmx = parts
            .headers
            .get("HX-Request")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "true")
            .unwrap_or(false);

        // 2. read session cookie, lookup user
        let caregiver = super::middleware::session::load_caregiver_from_cookie(parts, &app_state)
            .await
            .map_err(|e| e.into_response())?;

        let role = caregiver
            .as_ref()
            .map(|c| c.role)
            .unwrap_or(Role::Anonymous);

        Ok(RequestContext {
            caregiver,
            role,
            is_htmx,
        })
    }
}

RequestContext is injected into handlers as ctx: RequestContext.

⸻

4. tower middleware / layers

4.1 global stack

// src/interfaces/http/routes.rs
use axum::{Router, routing::{get, post}};
use tower::ServiceBuilder;
use tower_http::{
    trace::TraceLayer,
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
};

pub fn build_router(state: AppState) -> Router {
    let api = api_router();
    let web = web_router();
    let webhooks = webhook_router();

    let app = Router::new()
        .nest("/", web)
        .nest("/api", api)
        .nest("/webhooks", webhooks)
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(CompressionLayer::new())
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(|req: &axum::http::Request<_>| {
                            tracing::info_span!(
                                "request",
                                method = %req.method(),
                                uri = %req.uri(),
                            )
                        }),
                )
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(PropagateRequestIdLayer::x_request_id())
        );

    app
}

4.2 auth / session middleware

session = signed cookie containing caregiver_id.

// src/interfaces/http/middleware/session.rs
use axum::http::request::Parts;
use crate::{AppState, domain::caregiver::Caregiver};

// read cookie, fetch caregiver from db if present
pub async fn load_caregiver_from_cookie(
    parts: &mut Parts,
    state: &AppState,
) -> anyhow::Result<Option<Caregiver>> {
    let cookies = parts
        .headers
        .get(axum_extra::extract::cookie::COOKIE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let jar = cookie::CookieJar::from_str(cookies).unwrap_or_default();
    if let Some(raw) = jar.get("session") {
        let session = crate::interfaces::http::session::verify_session(raw.value(), &state.config)?;
        let caregiver_id = session.caregiver_id;
        let repo = crate::infrastructure::db::caregiver_repo::PgCaregiverRepo::new(state.db_pool.clone());
        let caregiver = repo.find_by_id(caregiver_id).await?;
        Ok(caregiver)
    } else {
        Ok(None)
    }
}

auth for protected routes is done via a guard/extractor:

// src/interfaces/http/extractors.rs
pub struct AuthenticatedCaregiver(pub Caregiver);

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedCaregiver
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = axum::response::Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ctx = RequestContext::from_request_parts(parts, state).await?;
        if let Some(c) = ctx.caregiver {
            Ok(AuthenticatedCaregiver(c))
        } else {
            Err(axum::response::Redirect::to("/login").into_response())
        }
    }
}

admin guard similar, checking role == Role::Admin.

⸻

5. routing structure

5.1 web (caregiver ui)

fn web_router() -> Router<AppState> {
    Router::new()
        .route("/", get(caregiver_dashboard))
        .route("/login", get(show_login).post(do_login))
        .route("/logout", post(do_logout))
        .route("/elder/profile", get(show_elder_profile).post(update_elder_profile))
        .route("/elder/contacts", get(show_contacts).post(create_contact))
        .route("/elder/contacts/:id", post(update_contact).delete(delete_contact))
        .route("/elder/locations", get(show_locations).post(create_location))
        .route("/elder/locations/:id", post(update_location).delete(delete_location))
        .route("/elder/medications", get(show_meds).post(create_med))
        .route("/elder/medications/:id", post(update_med).delete(delete_med))
        .route("/elder/logs/calls", get(show_call_logs))
        .route("/elder/logs/reminders", get(show_reminder_logs))
        .route("/elder/logs/rides", get(show_ride_logs))
}

handlers accept (State<AppState>, RequestContext, ...) and render tera templates.

5.2 api (json if we ever need pure api)

similar structure under /api, but returning json view models.

5.3 webhooks

fn webhook_router() -> Router<AppState> {
    Router::new()
        .route("/twilio/voice", post(twilio_incoming_call))
        .route("/twilio/status", post(twilio_call_status))
        .route("/twilio/media/:call_sid", get(twilio_media_ws_upgrade))
        .route("/stripe", post(stripe_webhook))
        .route("/uber", post(uber_webhook))
}


⸻

6. templates + htmx + alpine.js + tailwindcss

6.1 tailwind setup

install tailwindcss and configure for tera templates:

```bash
npm install -D tailwindcss
npx tailwindcss init
```

tailwind.config.js:
```js
module.exports = {
  content: ["./src/templates/**/*.html"],
  theme: {
    extend: {},
  },
  plugins: [],
}
```

input.css:
```css
@tailwind base;
@tailwind components;
@tailwind utilities;
```

build with: `npx tailwindcss -i ./input.css -o ./static/css/output.css --watch`

6.2 layout

templates/layout.html.tera:

<!doctype html>
<html lang="en" x-data="{ dark: false }" :class="{ 'dark': dark }">
<head>
  <meta charset="utf-8">
  <title>{% block title %}assistant{% endblock title %}</title>
  <link rel="stylesheet" href="/static/css/output.css">
  <script src="https://unpkg.com/alpinejs" defer></script>
  <script src="https://unpkg.com/htmx.org" defer></script>
</head>
<body class="bg-gray-50 dark:bg-gray-900 min-h-screen">
  {% include "components/nav.html.tera" %}
  <main id="content" class="container mx-auto px-4 py-8">
    {% include "components/flash.html.tera" %}
    {% block content %}{% endblock content %}
  </main>
</body>
</html>

6.2 example caregiver page

templates/caregiver/contacts.html.tera:

{% extends "layout.html.tera" %}
{% block title %}contacts{% endblock title %}

{% block content %}
<h1>contacts</h1>

<table>
  <thead>
    <tr><th>name</th><th>relationship</th><th>phone</th><th></th></tr>
  </thead>
  <tbody id="contacts-table">
    {% for c in contacts %}
    <tr id="contact-{{ c.id }}">
      <td>{{ c.name }}</td>
      <td>{{ c.relationship }}</td>
      <td>{{ c.phone }}</td>
      <td>
        <button
          hx-get="/elder/contacts/{{ c.id }}/edit"
          hx-target="#contact-{{ c.id }}"
          hx-swap="outerHTML"
        >
          edit
        </button>
        <button
          hx-delete="/elder/contacts/{{ c.id }}"
          hx-target="#contact-{{ c.id }}"
          hx-swap="outerHTML"
        >
          delete
        </button>
      </td>
    </tr>
    {% endfor %}
  </tbody>
</table>

<form
  hx-post="/elder/contacts"
  hx-target="#contacts-table"
  hx-swap="beforeend"
>
  <!-- inputs for name / relationship / phone -->
</form>
{% endblock content %}

controllers just render full page or fragments depending on ctx.is_htmx.

⸻

7. domain + application

7.1 domain models (sketch)

// src/domain/caregiver.rs
#[derive(Clone, Debug)]
pub enum Role {
    Caregiver,
    Admin,
    Anonymous,
}

#[derive(Clone, Debug)]
pub struct Caregiver {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub password_hash: String,
    pub role: Role,
}

similar structs for Elder, Contact, Location, Medication, etc. all plain structs, no db or framework dependencies.

7.2 ports (repository + external service traits)

// src/application/ports.rs
use crate::domain::{
    caregiver::Caregiver,
    elder::Elder,
    contact::Contact,
    location::Location,
    medication::{Medication, MedicationSchedule},
    ride::RideRequest,
};

#[async_trait::async_trait]
pub trait CaregiverRepository {
    async fn find_by_id(&self, id: i64) -> anyhow::Result<Option<Caregiver>>;
    async fn find_by_email(&self, email: &str) -> anyhow::Result<Option<Caregiver>>;
    async fn insert(&self, caregiver: &Caregiver) -> anyhow::Result<i64>;
}

#[async_trait::async_trait]
pub trait ElderRepository { /* functions… */ }

#[async_trait::async_trait]
pub trait ContactRepository { /* functions… */ }

#[async_trait::async_trait]
pub trait LocationRepository { /* functions… */ }

#[async_trait::async_trait]
pub trait MedicationRepository { /* functions… */ }

#[async_trait::async_trait]
pub trait RideRepository { /* functions… */ }

#[async_trait::async_trait]
pub trait UberService {
    async fn request_ride(&self, elder: &Elder, location: &Location) -> anyhow::Result<RideRequest>;
}

// etc for Twilio, OpenAI, Stripe

7.3 services

// src/application/services/rides_service.rs
use crate::application::ports::{RideRepository, UberService, ElderRepository, LocationRepository};

pub struct RidesService<R, U, E, L> {
    rides_repo: R,
    uber: U,
    elders: E,
    locations: L,
}

impl<R, U, E, L> RidesService<R, U, E, L>
where
    R: RideRepository + Send + Sync,
    U: UberService + Send + Sync,
    E: ElderRepository + Send + Sync,
    L: LocationRepository + Send + Sync,
{
    pub async fn request_ride_to_location(
        &self,
        elder_id: i64,
        location_id: i64,
    ) -> anyhow::Result<()> {
        let elder = self.elders.find_by_id(elder_id).await?
            .ok_or_else(|| anyhow::anyhow!("elder not found"))?;
        let loc = self.locations.find_by_id(location_id).await?
            .ok_or_else(|| anyhow::anyhow!("location not found"))?;

        let ride = self.uber.request_ride(&elder, &loc).await?;
        self.rides_repo.insert(&ride).await?;
        Ok(())
    }
}

handlers in interfaces talk to these services, not directly to repos or external clients.

⸻

8. db access (deadpool + tokio-postgres)

8.1 example repo

// src/infrastructure/db/caregiver_repo.rs
use deadpool_postgres::Pool;
use tokio_postgres::Row;
use crate::{
    application::ports::CaregiverRepository,
    domain::caregiver::{Caregiver, Role},
};

pub struct PgCaregiverRepo {
    pool: Pool,
}

impl PgCaregiverRepo {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    fn map_row(row: Row) -> Caregiver {
        Caregiver {
            id: row.get("id"),
            name: row.get("name"),
            email: row.get("email"),
            password_hash: row.get("password_hash"),
            role: if row.get::<_, String>("role") == "admin" {
                Role::Admin
            } else {
                Role::Caregiver
            },
        }
    }
}

#[async_trait::async_trait]
impl CaregiverRepository for PgCaregiverRepo {
    async fn find_by_id(&self, id: i64) -> anyhow::Result<Option<Caregiver>> {
        let client = self.pool.get().await?;
        let row = client
            .query_opt("SELECT * FROM caregivers WHERE id = $1", &[&id])
            .await?;
        Ok(row.map(Self::map_row))
    }

    async fn find_by_email(&self, email: &str) -> anyhow::Result<Option<Caregiver>> {
        let client = self.pool.get().await?;
        let row = client
            .query_opt("SELECT * FROM caregivers WHERE email = $1", &[&email])
            .await?;
        Ok(row.map(Self::map_row))
    }

    async fn insert(&self, caregiver: &Caregiver) -> anyhow::Result<i64> {
        let client = self.pool.get().await?;
        let row = client
            .query_one(
                "INSERT INTO caregivers (name, email, password_hash, role)
                 VALUES ($1, $2, $3, $4)
                 RETURNING id",
                &[
                    &caregiver.name,
                    &caregiver.email,
                    &caregiver.password_hash,
                    &match caregiver.role {
                        Role::Admin => "admin",
                        _ => "caregiver",
                    },
                ],
            )
            .await?;
        Ok(row.get(0))
    }
}

no orm, just simple sql. everything else is analogous.

⸻

9. data model (tables)

normalized and boring. keep it that way.

-- migrations/001_init.sql
CREATE TABLE caregivers (
  id              BIGSERIAL PRIMARY KEY,
  name            TEXT NOT NULL,
  email           TEXT NOT NULL UNIQUE,
  password_hash   TEXT NOT NULL,
  role            TEXT NOT NULL DEFAULT 'caregiver', -- 'admin' / 'caregiver'
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE elders (
  id              BIGSERIAL PRIMARY KEY,
  caregiver_id    BIGINT NOT NULL REFERENCES caregivers(id) ON DELETE CASCADE,
  name            TEXT NOT NULL,
  phone_number    TEXT NOT NULL,
  timezone        TEXT NOT NULL DEFAULT 'America/New_York',
  language        TEXT NOT NULL DEFAULT 'en',
  status          TEXT NOT NULL DEFAULT 'active',
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE contacts (
  id              BIGSERIAL PRIMARY KEY,
  elder_id        BIGINT NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
  name            TEXT NOT NULL,
  relationship    TEXT NOT NULL,
  phone           TEXT NOT NULL,
  notes           TEXT,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE locations (
  id              BIGSERIAL PRIMARY KEY,
  elder_id        BIGINT NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
  name            TEXT NOT NULL,
  address         TEXT NOT NULL,
  extra_instructions TEXT,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE medications (
  id              BIGSERIAL PRIMARY KEY,
  elder_id        BIGINT NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
  name            TEXT NOT NULL,
  dosage          TEXT NOT NULL,
  instructions    TEXT,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE medication_schedules (
  id              BIGSERIAL PRIMARY KEY,
  medication_id   BIGINT NOT NULL REFERENCES medications(id) ON DELETE CASCADE,
  time_of_day     TIME NOT NULL,
  days_pattern    TEXT NOT NULL DEFAULT 'daily' -- naive; refine later
);

CREATE TABLE reminder_logs (
  id              BIGSERIAL PRIMARY KEY,
  schedule_id     BIGINT NOT NULL REFERENCES medication_schedules(id) ON DELETE CASCADE,
  ts              TIMESTAMPTZ NOT NULL DEFAULT now(),
  delivery_method TEXT NOT NULL, -- 'call' | 'sms'
  status          TEXT NOT NULL, -- 'delivered' | 'no_answer' | 'confirmed'
  metadata        JSONB
);

CREATE TABLE ride_requests (
  id              BIGSERIAL PRIMARY KEY,
  elder_id        BIGINT NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
  location_id     BIGINT NOT NULL REFERENCES locations(id),
  uber_ride_id    TEXT NOT NULL,
  status          TEXT NOT NULL,
  requested_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  completed_at    TIMESTAMPTZ,
  metadata        JSONB
);

CREATE TABLE call_sessions (
  id              BIGSERIAL PRIMARY KEY,
  elder_id        BIGINT NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
  twilio_call_sid TEXT NOT NULL,
  started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  ended_at        TIMESTAMPTZ,
  summary_text    TEXT,
  transcript      TEXT,
  metadata        JSONB
);

CREATE TABLE subscriptions (
  id                      BIGSERIAL PRIMARY KEY,
  caregiver_id            BIGINT NOT NULL REFERENCES caregivers(id) ON DELETE CASCADE,
  stripe_customer_id      TEXT NOT NULL,
  stripe_subscription_id  TEXT NOT NULL,
  status                  TEXT NOT NULL,
  current_period_end      TIMESTAMPTZ
);


⸻

10. reliability / safety notes
	•	use TraceLayer for logging + attach request ids.
	•	wrap external calls (openai, twilio, uber, stripe) in small client wrappers with:
	•	sensible timeouts
	•	retry where appropriate (idempotent GET/POST only)
	•	central error type → map to StatusCode + error pages.
	•	strict separation:
	•	webhooks (twilio, stripe, uber) under /webhooks
	•	verify signatures before doing anything.
	•	auth boundaries:
	•	caregivers can only access their elder’s resources.
	•	admins can access everything; admin-only routes under /admin.
	•	do not store openai/twilio/stripe secrets in db. only env/config.



