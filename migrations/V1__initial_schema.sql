-- Initial database schema for Voice AI Assistant

-- Caregivers (account holders)
CREATE TABLE caregivers (
    id UUID PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    role VARCHAR(50) NOT NULL DEFAULT 'caregiver',
    stripe_customer_id VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_caregivers_email ON caregivers(email);
CREATE INDEX idx_caregivers_stripe_customer_id ON caregivers(stripe_customer_id);

-- Elders (elderly users who call the assistant)
CREATE TABLE elders (
    id UUID PRIMARY KEY,
    caregiver_id UUID NOT NULL REFERENCES caregivers(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    phone_number VARCHAR(50) NOT NULL,
    timezone VARCHAR(100) NOT NULL DEFAULT 'America/New_York',
    language VARCHAR(10) NOT NULL DEFAULT 'en',
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_elders_caregiver_id ON elders(caregiver_id);
CREATE INDEX idx_elders_phone_number ON elders(phone_number);
CREATE INDEX idx_elders_status ON elders(status);

-- Contacts (elder's contacts - family, doctors, etc.)
CREATE TABLE contacts (
    id UUID PRIMARY KEY,
    elder_id UUID NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    relationship VARCHAR(100) NOT NULL,
    phone VARCHAR(50) NOT NULL,
    notes TEXT,
    is_emergency BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_contacts_elder_id ON contacts(elder_id);
CREATE INDEX idx_contacts_is_emergency ON contacts(is_emergency);

-- Locations (saved addresses for rides)
CREATE TABLE locations (
    id UUID PRIMARY KEY,
    elder_id UUID NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    address TEXT NOT NULL,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    extra_instructions TEXT,
    is_home BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_locations_elder_id ON locations(elder_id);
CREATE INDEX idx_locations_is_home ON locations(is_home);

-- Medications
CREATE TABLE medications (
    id UUID PRIMARY KEY,
    elder_id UUID NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    dosage VARCHAR(255) NOT NULL,
    instructions TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_medications_elder_id ON medications(elder_id);

-- Medication schedules (when to take each medication)
CREATE TABLE medication_schedules (
    id UUID PRIMARY KEY,
    medication_id UUID NOT NULL REFERENCES medications(id) ON DELETE CASCADE,
    time_of_day TIME NOT NULL,
    days_pattern VARCHAR(100) NOT NULL DEFAULT 'daily',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_medication_schedules_medication_id ON medication_schedules(medication_id);
CREATE INDEX idx_medication_schedules_time_of_day ON medication_schedules(time_of_day);

-- Reminder logs (history of medication reminders)
CREATE TABLE reminder_logs (
    id UUID PRIMARY KEY,
    schedule_id UUID NOT NULL REFERENCES medication_schedules(id) ON DELETE CASCADE,
    elder_id UUID NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
    medication_name VARCHAR(255) NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    delivery_method VARCHAR(50) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    twilio_call_sid VARCHAR(255),
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_reminder_logs_elder_id ON reminder_logs(elder_id);
CREATE INDEX idx_reminder_logs_schedule_id ON reminder_logs(schedule_id);
CREATE INDEX idx_reminder_logs_timestamp ON reminder_logs(timestamp);
CREATE INDEX idx_reminder_logs_status ON reminder_logs(status);

-- Ride requests (Uber rides)
CREATE TABLE ride_requests (
    id UUID PRIMARY KEY,
    elder_id UUID NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
    location_id UUID NOT NULL REFERENCES locations(id),
    pickup_address TEXT NOT NULL,
    dropoff_address TEXT NOT NULL,
    uber_ride_id VARCHAR(255),
    status VARCHAR(50) NOT NULL DEFAULT 'requested',
    driver_name VARCHAR(255),
    driver_phone VARCHAR(50),
    vehicle_make VARCHAR(100),
    vehicle_model VARCHAR(100),
    vehicle_license VARCHAR(50),
    eta_minutes INTEGER,
    fare_estimate VARCHAR(50),
    fare_actual VARCHAR(50),
    requested_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_ride_requests_elder_id ON ride_requests(elder_id);
CREATE INDEX idx_ride_requests_uber_ride_id ON ride_requests(uber_ride_id);
CREATE INDEX idx_ride_requests_status ON ride_requests(status);
CREATE INDEX idx_ride_requests_requested_at ON ride_requests(requested_at);

-- Call sessions (AI call history)
CREATE TABLE call_sessions (
    id UUID PRIMARY KEY,
    elder_id UUID NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
    twilio_call_sid VARCHAR(255) NOT NULL UNIQUE,
    from_number VARCHAR(50) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    duration_seconds INTEGER,
    summary_text TEXT,
    transcript TEXT,
    tools_used TEXT[] NOT NULL DEFAULT '{}',
    transferred_to VARCHAR(255),
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_call_sessions_elder_id ON call_sessions(elder_id);
CREATE INDEX idx_call_sessions_twilio_call_sid ON call_sessions(twilio_call_sid);
CREATE INDEX idx_call_sessions_started_at ON call_sessions(started_at);
CREATE INDEX idx_call_sessions_status ON call_sessions(status);

-- Subscriptions (Stripe billing)
CREATE TABLE subscriptions (
    id UUID PRIMARY KEY,
    caregiver_id UUID NOT NULL REFERENCES caregivers(id) ON DELETE CASCADE,
    stripe_subscription_id VARCHAR(255) NOT NULL UNIQUE,
    stripe_customer_id VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    plan_name VARCHAR(100) NOT NULL,
    current_period_start TIMESTAMPTZ NOT NULL,
    current_period_end TIMESTAMPTZ NOT NULL,
    cancel_at_period_end BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_subscriptions_caregiver_id ON subscriptions(caregiver_id);
CREATE INDEX idx_subscriptions_stripe_subscription_id ON subscriptions(stripe_subscription_id);
CREATE INDEX idx_subscriptions_status ON subscriptions(status);

