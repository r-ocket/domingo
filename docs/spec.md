Voice AI Assistant for Elderly – System Specification

Overview

This system is a voice-based AI assistant designed to help elderly users via simple phone calls. It allows an elderly person to call a dedicated number and converse with an AI agent (powered by OpenAI’s GPT-Realtime model) that can assist with everyday tasks. Key features include hailing rides (Uber) to pre-saved locations, providing medication reminders, and retrieving important contact information or even connecting calls to those contacts. The solution integrates Twilio’s Voice API for telephony and real-time audio streaming, and it utilizes a Rust backend (Axum framework) with a PostgreSQL database. The architecture follows clean architecture principles to ensure a maintainable and scalable design ￼.

The platform involves multiple user roles: the Elderly User (who interacts via phone calls), the Caregiver (usually a family member or aide who configures the service and pays for it), and an Admin (company staff overseeing the system). A web-based interface (using htmx and Alpine.js for dynamic but lightweight interactivity) is provided for caregivers and admins to manage data, view logs, and handle billing. Payment integration is handled through the Stripe API to manage subscriptions and billing. The following sections detail user stories, architecture, features, and API design.

User Roles & Stories

Elderly User (Voice Caller): The elderly user does not use any app or computer – they simply call a phone number and speak with the AI assistant.
	•	As an elderly user, I want to request a ride (Uber) to a familiar place by talking to the assistant, so I can travel without using a smartphone app.
	•	As an elderly user, I want the assistant to remind me of my medication schedule, so I don’t miss taking my pills on time.
	•	As an elderly user, I want to ask the assistant for a family member’s phone number or ask it to call them, so I can easily stay in touch or get help.
	•	As an elderly user, I want a natural, human-like conversation over the phone, so I feel comfortable and understood when getting assistance.

Caregiver (Web User & Service Subscriber): The caregiver sets up and manages the service on behalf of the elder.
	•	As a caregiver, I want to create an account and enroll my elderly parent into the service, so they can start calling the AI assistant.
	•	As a caregiver, I want to save important locations (like “Home”, “Doctor’s Clinic”) for the elder, so the assistant knows where to book rides.
	•	As a caregiver, I want to input my elder’s medication schedule and dosage times, so the system can remind them reliably.
	•	As a caregiver, I want to store key contact information (family members, doctors, emergency services), so the assistant can provide or dial these contacts for the elder.
	•	As a caregiver, I want to view call logs or transcripts of my elder’s interactions (or at least summaries), so I can monitor their needs and ensure they are using the service safely.
	•	As a caregiver, I want to manage billing (subscription payments) easily through a secure interface, so that the service remains active without hassle.

Admin (Service Provider Staff): The admin has oversight over the entire system.
	•	As an admin, I want to view all user accounts and elder profiles, so I can assist with support or adjust settings if needed.
	•	As an admin, I want to monitor system logs, call statistics, and AI performance, so I can ensure the system is running smoothly and identify any issues.
	•	As an admin, I want an interface to manage subscriptions, refunds, or account status, so I can handle business operations and customer support.
	•	As an admin, I want to update content like FAQ or support contact info on the caregiver interface, so that users get up-to-date information.

These stories drive the requirements for our system’s features and help clarify how each type of user will interact with the application.

Architecture & Technology Stack

High-Level Architecture: We follow a layered clean architecture approach, separating core business logic from external concerns ￼ ￼. The system consists of several major components:
	•	Voice Interaction Layer: Handles phone call integration via Twilio and streams audio to/from the AI. This is the real-time interface for the elderly user’s voice conversations.
	•	AI Brain (LLM) and Tool Layer: Utilizes OpenAI’s GPT-Realtime model for understanding and responding to user queries, with the ability to invoke external tools (functions) for specific tasks like booking a ride or fetching data ￼.
	•	Backend API Layer (Axum in Rust): Implements HTTP endpoints (for web interface, Twilio webhooks, Stripe webhooks, etc.) and houses the business logic (use-cases) in a framework-agnostic manner. Axum provides robust routing and middleware in Rust, fitting well with a clean architecture design ￼.
	•	Database Layer (PostgreSQL): Stores persistent data such as user accounts, elder profiles, contacts, saved locations, schedules, call logs, etc. Accessed via tokio-postgres with Deadpool for connection pooling (ensuring asynchronous, efficient DB operations).
	•	Frontend Interface: A web interface for caregivers and admins. Built using server-rendered HTML templates (e.g., using Askama or Tera templating in Rust) enhanced by htmx (for partial page updates via AJAX), Alpine.js (for client-side interactivity), and Tailwind CSS (for utility-first styling). This combination avoids a heavy single-page app, favoring simplicity and reliability. (For example, using Alpine.js with htmx on the client side is a known approach for a dynamic yet lightweight Rust web stack ￼.)
	•	External Service Integrations:
	•	Twilio API: for telephony – managing incoming calls, streaming audio via WebSockets, and making outbound calls or SMS for reminders.
	•	OpenAI API (GPT-Realtime): for real-time speech-to-speech AI capabilities, including advanced conversation handling and function calling support.
	•	Uber API: for ride requests – the backend will integrate with Uber’s Ride Requests API to hail rides programmatically ￼.
	•	Stripe API: for payment processing – handling subscriptions, billing cycles, and webhooks for payment events.

Clean Architecture in Practice: We structure the Rust backend into distinct layers/modules:
	•	Domain Models: Plain data structures (and maybe business logic methods) representing core entities – Elder, Caregiver, Contact, Location, Reminder, RideRequest, etc. These are independent of any framework or database details.
	•	Use Case / Service Layer: Implements operations like “BookRide(ElderID, LocationID)”, “ScheduleReminder(ElderID, time)”, “GetNextMedication(ElderID)” or “InitiateCallToContact(ElderID, ContactID)”. These orchestrate domain logic and call out to the necessary repositories or external service clients. This layer is where we enforce business rules (e.g., only allow booking rides to saved locations, or only caregivers can modify certain data).
	•	Infrastructure Layer: Contains implementations for outside-world interactions – e.g., database repository implementations (using SQL queries via tokio-postgres), Twilio API client (for making calls or generating TwiML), OpenAI API client, Uber API client, Stripe client, etc. These are injected or referenced by the use case layer when needed.
	•	Interface (Delivery) Layer: Axum HTTP controllers (handlers) live here. They accept HTTP requests (from the web frontend or webhooks), parse input (using Axum extractors or serde for JSON), call the appropriate use case in the service layer, and format the output (HTML templates or JSON responses). This layer also includes the Twilio voice webhook handler and Stripe webhook handler as special endpoints. The UI templates for htmx responses or full page renders also reside here (or in a separate frontend folder, integrated via the build).

This separation ensures that replacing a component (say, switching Twilio for another telephony service or swapping the database) has minimal impact on other parts of the system ￼ ￼. It also makes the system more testable and maintainable in the long run. Rust’s strong type system further complements this architecture by catching many errors at compile time and ensuring safe concurrency.

Scalability and Reliability: Axum and Tokio (Rust’s async runtime) will allow the server to handle many concurrent calls and web requests efficiently. We can scale horizontally by running multiple instances of the Rust service, as long as they share the database and are all configured with the same external API credentials. Twilio’s webhooks can be configured to hit a load balancer or API gateway that distributes to our service instances. Because the AI processing happens via OpenAI’s API, that part scales on OpenAI’s side; our system just needs to handle streaming data to/from their endpoints. We will also implement logging and monitoring (using, e.g., structured logs in JSON and perhaps Prometheus metrics) to observe performance, call volumes, errors, etc.

Voice Interaction: Twilio & GPT-Realtime Integration

At the heart of the system is the real-time voice conversation between the elderly user and the AI. This is enabled by integrating Twilio’s Programmable Voice with OpenAI’s GPT-Realtime model.
	•	Twilio Voice Call Handling: We will obtain a Twilio phone number that elders call. When an incoming call arrives, Twilio will send an HTTP webhook request to our backend (to a predefined endpoint) with details about the call (caller ID, etc.). Our backend will respond with Twilio TwiML instructions to hand off the call audio to our service. Specifically, we use Twilio Media Streams with a <Connect><Stream> directive in the TwiML. This tells Twilio to fork the call’s audio and stream it to our server via a secure WebSocket connection ￼. Twilio’s media stream will send audio data in real-time as the user speaks, and it will expect audio back from our server to play to the user (enabling a bidirectional conversation). According to Twilio’s documentation, using <Connect><Stream> effectively creates a bidirectional audio pipeline between the call and our WebSocket server ￼ ￼. In practice, our backend’s voice handler upgrades the HTTP connection to a WebSocket endpoint that Twilio will connect to for streaming audio.
	•	OpenAI GPT-Realtime Processing: Once the WebSocket connection from Twilio is established, our server will bridge it with OpenAI’s Realtime API. The backend opens a corresponding WebSocket connection to OpenAI’s endpoint (e.g., wss://api.openai.com/v1/realtime?model=gpt-realtime with appropriate auth headers). The server then acts as a relay: audio frames from Twilio (the user’s speech) are forwarded to OpenAI, and audio generated by the GPT model is sent back to Twilio to play to the user. This direct streaming approach allows for low latency, natural back-and-forth conversation ￼. In fact, OpenAI’s Realtime model processes audio input and output in one end-to-end step (without separately converting to text and back), which preserves vocal nuances and reduces delay ￼. The model can understand the user’s speech (even detecting tone or emotion) and respond with a synthesized voice that sounds expressive and lifelike. Twilio’s media stream thus effectively connects the caller with an AI that talks almost like a human in real time. (Twilio’s own sample demonstrates this real-time voice agent by proxying audio between the caller and OpenAI over WebSockets ￼.)
	•	Natural Conversation Flow: GPT-Realtime is specifically designed for voice agent use cases. It supports barge-in and interruptions, meaning the elder can speak over the assistant or vice versa without breaking the interaction – the model can handle overlapping speech gracefully ￼. This is important for elderly users who might start talking while the assistant is still speaking; the conversation will feel more natural and less rigid. Additionally, GPT-Realtime maintains context throughout the call, so the user can ask follow-up questions or use pronouns (“take me there” after mentioning a place) and the model will understand. The voice output is also more natural-sounding thanks to OpenAI’s improved speech synthesis (with voices like Cedar and Marin that were introduced) ￼ ￼. We will likely choose a friendly, clear voice that is easy for seniors to understand.
	•	System Messages & Prompting: When initiating the call session with OpenAI, our backend sends a system prompt to establish the assistant’s persona and tools. For example, a system message might say: “You are a helpful and patient AI assistant for elderly users. You can assist with booking rides, medication reminders, and providing contact information using the provided functions. Speak clearly and empathetically.” This ensures the AI’s tone and behavior are appropriate. We can also instruct it to follow certain safety rules (not to give medical advice beyond reminding, etc.). The model will then receive the user’s speech and our defined functions (see Tools section below).
	•	Functions (Tools) via Function Calling: One powerful feature we leverage is OpenAI’s function calling in the GPT-Realtime model ￼. We define a set of functions in the API (with schemas) that the AI can call to perform actions like: request_ride(destination), get_med_schedule(), get_contact_info(name), or call_contact(name), etc. During the conversation, when the user asks for something that requires external action or data, the model can choose to invoke one of these functions. For example, if the user says, “Could you call me an Uber to the grocery store?”, the assistant (the model) will internally decide to call the request_ride function with the parameter “grocery store” (or an identifier for that location). Our backend receives this function call request, executes the logic (e.g., calls the Uber API, see below), and returns the result (e.g., “Ride confirmed, arriving in 5 minutes”). The GPT model then continues the conversation, incorporating that result into what it says to the user. The GPT-Realtime model improvements mean it’s better at choosing the right function and supplying correct arguments ￼, which is crucial for reliability. Importantly, GPT-Realtime also supports asynchronous function handling – the model conversation doesn’t freeze if an API call takes a few seconds. The model can keep speaking or listening while the function result is pending ￼. For instance, it might say, “Alright, let me book that Uber for you…,” and continue with small talk or confirmation questions while the booking completes, providing a fluid experience even during tool usage.
	•	Error Handling & Fallbacks: If the AI or the streaming fails (e.g., OpenAI API error or network issue), our system should handle it gracefully. The Twilio call can be switched to a backup TwiML that uses standard text-to-speech to apologize and maybe offer to call a human caregiver. We will implement timeout logic: if no response comes from the AI in a certain time, the backend can either retry or end the call with a friendly message. All such incidents would be logged so the admin can review any failures. Also, if the user says something that the AI cannot handle and it doesn’t call a function when it should, we can catch certain keywords on our side as a safety net. For example, if the caller says “operator” or “help”, we might immediately bridge the call to a preset emergency contact or caregiver. These are design considerations to ensure reliability for elderly users who might be in urgent need.

In summary, the voice interaction component leverages Twilio’s robust telephony capabilities to capture and play audio, and OpenAI’s cutting-edge voice AI to process and generate responses in real-time. This combination enables an intuitive phone-based assistant experience: the user just talks and listens as they would in a normal phone call, while under the hood the system handles streaming audio and AI-driven dialogue.

Tool Integrations and Services

Beyond conversation, the assistant’s usefulness comes from the actions it can perform and the information it can provide. Here we detail the key integrated tools and features: ride booking, medication reminders, and contact info services.

Uber Ride Booking Service

One primary function is the ability to hail an Uber ride for the elder to a pre-designated destination. The workflow and design for this feature are as follows:
	•	Pre-Saved Locations: The caregiver can save important addresses for the elder (like Home, Grocery Store, Doctor’s Office, Pharmacy, etc.) through the web interface. Each location has a nickname and an address (plus perhaps any special instructions for pickup). These are stored in the database and associated with the elder’s profile. For simplicity, we assume rides will always originate from the elder’s home or current location (which we might default to “Home” address unless specified) – though if the service is via phone call, the AI might ask “Are you at home now?” to confirm pickup location if needed.
	•	Triggering a Ride Request: When the elder says something like “Get me a ride to the doctor’s” or “I need to go to the grocery store”, the AI will use the request_ride(location_name) function. Our backend implements this by calling the Uber API. Specifically, we use Uber’s Ride Requests REST API to create a ride on behalf of the elder ￼. Under the hood, we likely leverage Uber for Business capabilities (since hailing a ride for someone else typically requires either the rider’s Uber account to be linked or using a business account). In our model, the caregiver could link their own Uber account or a dedicated account for the elder during setup (through an OAuth flow with Uber), or our company might have an Uber Business account to dispatch rides. For now, we assume a backend integration where we have credentials to request rides for the elder’s phone number or name.
	•	Uber API Integration: The backend will call Uber’s endpoint (over HTTPS) with details like pickup location (the elder’s current address), drop-off location (the saved address requested), and rider details (name/phone to share with driver). The Uber API will return information about the ride request – e.g., ETA for pickup, driver details, vehicle info, and a ride status ID. Uber’s Ride Request API allows third-party apps to hail rides and provides status updates via webhooks or polling ￼. We will register a Webhook endpoint with Uber so that when the ride status changes (driver arriving, ride started, ride completed, etc.), our backend gets notified.
	•	Communicating with the Elder: Upon successfully requesting the ride, the assistant will convey the important details to the elder. Thanks to GPT-Realtime’s function handling, once our request_ride function returns (with, say, “Uber confirmed, driver John (in a Toyota Camry) arriving in 5 minutes.”), the AI can immediately say that to the user in a friendly manner. The call might stay open until the car arrives if the user wants (allowing them to ask follow-up questions like the car’s license plate or to cancel if needed). However, to avoid long call charges and because the user will physically transition to the ride, we might design it such that the AI says goodbye after confirming and advising “Your Uber will arrive in about 5 minutes. I’ll hang up now. Safe travels!” For safety, we could also send an SMS (via Twilio) to the elder’s phone with the ride details (car model, license, ETA), since they may want a reminder on hand. (This is optional but could be valuable if the elder knows how to read texts or if a caregiver also wants notification.)
	•	Cost and Payment Considerations: If using the caregiver’s linked Uber account, the ride cost will be charged to that. Our system doesn’t handle Uber payments directly aside from initiating the request. If using a company account (less likely due to liability), we might invoice the caregiver later or include it in subscription. For now, we assume the simplest path: the caregiver’s Uber account is used (they would have authorized us via OAuth to request rides for this elder). This means all rides are paid through Uber by the caregiver, and our service just facilitates it. (In future, we could integrate billing or credits for rides, but it complicates things.)
	•	Uber Webhook Handling: The /api/uber/webhook endpoint (secured with a token) will receive events like “ride.started”, “ride.completed”. We can use these for logging (record ride completion time and fare if provided) and possibly for notifications. For example, we might notify the caregiver when a ride is completed (so they know the elder reached destination), or trigger a follow-up call to the elder to check if they arrived safely (this might be beyond MVP, but is possible). Also if a ride is canceled or fails, the webhook can alert us to inform the elder or caregiver as needed.

By integrating Uber’s API, the system provides a critical independence tool for seniors who may not use smartphones. It essentially gives them the power of app-based ride hailing through a simple voice request. (As noted in industry discussions, partnering with rideshare services can greatly benefit elder care by streamlining transportation ￼ ￼.)

Medication Reminders Service

Medication adherence is a vital feature for many elderly users. Our system addresses this in two ways: scheduled outbound reminders and on-demand queries via the AI assistant.
	•	Caregiver-Configured Schedule: The caregiver can input a schedule of medications for the elder. For each medication, we might store fields like name, dosage, frequency or specific times of day (e.g., “8:00 AM and 8:00 PM”), and perhaps instructions (e.g., “with food”). This data is stored in a medication_schedule table linked to the elder’s profile. The caregiver interface will likely provide a simple form to add meds and set times (with timezone considerations). We ensure the schedule is stored in the elder’s local time.
	•	Reminder Delivery (Outbound Calls/SMS): At each scheduled time, the system will automatically deliver a reminder. We can implement this by scheduling background jobs (for example, using Cron-like scheduling in Rust or leveraging an external scheduler). When a reminder event triggers, our backend uses Twilio to call the elder’s phone and play a reminder message. This can be done either by generating TwiML <Say> instructions (text-to-speech) or even using the GPT-Realtime voice for a more natural reminder call. A simple version: Twilio calls and a recording says, “Hello! This is your reminder to take your 8 PM medication: 2 pills of Lipitor. Please press 1 after you’ve taken it.” (We could even gather input to confirm compliance, logging if they pressed 1 or not – that data could be shown to caregivers later.) If the call is not answered, we might send an SMS fallback: “Reminder: Take your 8 PM Lipitor (2 pills).” Ensuring the elder gets the reminder is crucial, so possibly both call and SMS could be used.
	•	AI-Initiated Reminders: Alternatively, the elder might call the assistant or already be on a call near a reminder time. The AI, thanks to system instructions or checking a function, could proactively say “By the way, it’s 8 PM; this is a good time to take your Lipitor medication.” We could implement a function check_due_medications() that the AI can call at appropriate times or upon user request. But primarily, reminders will be delivered even if the user doesn’t call in, via the proactive Twilio calls.
	•	On-Demand Medication Info: The elder can also ask the assistant questions like “When is my next medicine?” or “What do I take now?”. We provide a function (e.g., get_next_medication() or get_med_list_for_today()) that the AI can invoke. The backend will retrieve from the schedule the upcoming meds or all meds for today. The assistant will then tell the user in understandable terms. For example, “Your next medication is at 8 PM tonight: 2 pills of Lipitor. You have another medication at 9 PM: 1 tablet of Metformin.” The AI can also answer if the user asks if they’ve already taken something (though tracking that requires logging compliance input as mentioned). We might not track actual ingestion in MVP, beyond possibly recording if the user confirmed via phone input.
	•	Safety and Escalation: If an elder consistently misses acknowledging reminders, the system could alert the caregiver (e.g., an email or app notification saying “Your mom missed her 8 PM Lipitor dose”). That’s an added layer not explicitly requested, but worth noting for completeness. In this spec, suffice it to say we will log whether reminders were answered or confirmed, and caregivers can view those logs. The caregiver interface might show something like a checklist of doses and whether they were taken (if we implement the confirmation mechanism).

The medication reminder feature thus uses a combination of scheduled Twilio outbound calls and the AI’s conversational interface to keep the elder on track with their prescriptions, offering both proactive alerts and reactive info on request.

Contact Information & Calling Service

Another valuable feature is giving the elder easy access to important contacts. This includes simply recalling contact info (phone numbers, addresses) and actually placing calls to those contacts via the assistant.
	•	Contact Management: Through the caregiver portal, contacts can be added for the elder. Each contact entry would have at least a name, phone number, and relationship or description (e.g., “Daughter – Jane Doe – 555-1234”, “Dr. Smith – Cardiologist – 555-9876”). Optionally addresses or email if needed, but primarily phone is key here. These contacts are stored in a contacts table linked to the elder’s profile.
	•	Information Lookup: The elder might forget a number or need an address. They can ask the AI things like “What’s my son John’s phone number?” or “Give me Dr. Smith’s office address.” The assistant can call a function like get_contact_info(name) which our backend will implement to search the elder’s contacts by that name (or by relationship keywords like “son”). The function returns the details found. The AI will then speak it out: “John Doe’s number is 555-1234. His address is 123 Maple Street.” If there are multiple contacts with similar names, the AI might clarify by listing them or asking which one.
	•	Placing a Call via AI: More powerfully, the elder can say “Can you call John for me?”. Since they are already on a call (with the AI), we need to bridge that call to the new party. Twilio allows us to do call bridging by issuing new TwiML or using the Programmable Voice API to dial a number and join calls. Here’s how we handle it: We implement a function call_contact(name) that finds the contact’s number. When GPT calls this function, our backend will look up the number, then respond in a way that triggers a call transfer. We have two possible approaches:
	1.	Via TwiML <Dial>: We could respond to Twilio (perhaps by ending the media stream and redirecting Twilio to new TwiML instructions). For example, our function response might signal the server to generate TwiML like <Dial callerId="elder's number">5551234</Dial>. Twilio would then call that number and connect it to the elder, effectively transferring them out of the AI session into a direct call with John. The assistant would say “Alright, connecting you to John now, please wait…” and then the elder will hear ringing and John’s voice if he picks up. Once the call is connected to John, the AI drops off (Twilio streaming ends).
	2.	Via Conference or : Alternatively, we could keep the AI in a conference with both parties, but that’s likely not desired or necessary – the AI’s job is done once the two humans are talking, and staying on the line raises privacy issues. So the first approach (transfer and exit) is preferred.
We will implement the transfer by sending an appropriate command to Twilio. Twilio’s REST API can modify a live call, or we could respond with TwiML if we initiated a  originally. One detail: Twilio’s docs note that when using <Connect><Stream>, after the stream is closed by our server, Twilio can continue with remaining TwiML if provided ￼. That means we could pre-include a <Dial> in the TwiML after the <Connect><Stream> which will execute once our WebSocket closes. Concretely, our initial TwiML might look like:

<Response>
  <Connect>
    <Stream url="wss://our-server/voice-stream/12345" />
  </Connect>
  <Dial><Number>...some placeholder...</Number></Dial>
</Response>

But we don’t know the number at the start. Instead, a cleaner way is: when GPT calls call_contact, our backend can programmatically update the call via Twilio’s API (Calls resource) to redirect to new TwiML that contains <Dial> to the target number. This is known as issuing a TwiML redirect (or using the UpdateCall API). We will do that: the function logic will use Twilio’s REST API with the Call SID to say “play this TwiML now”. The TwiML response will cause the call leg with the elder to be bridged to the contact’s number.

	•	Ending Call Transfers: Once the elder is connected to their contact, Twilio is just acting as a normal phone switch – the AI is out of the loop. We should ensure our system marks the session ended and stops any OpenAI processing. That likely happens automatically when we closed the WS stream. We’ll also log that a call was transferred to Contact X at time Y. The Twilio call itself will terminate when either party hangs up as usual.
	•	Privacy: The assistant will have access to contact info, but we consider that part of the data the caregiver provided. It won’t share that info with anyone besides the elder upon request. All contact data is protected and only accessible within the elder’s context.

Providing these contact services ensures the AI assistant can help the elder stay connected with their support network without having to recall or dial numbers manually, which can be a great comfort and safety feature (for instance, being able to quickly call a family member through voice commands).

Other Features and Considerations
	•	Logging & Transcripts: The system will maintain logs of interactions. For voice calls, we can optionally store a text transcript of the conversation (OpenAI’s model might not output exact transcript, but we could enable a parallel speech-to-text for logging). At least, we will log high-level events: call started/ended times, functions invoked (e.g., ride booked, contact call placed), reminder calls made, etc. These logs are accessible to caregivers (filtered to only their elder’s data) and admins (all data) via the web UI. This serves both as a monitoring tool and for debugging any issues (e.g., seeing what the AI said if there was a miscommunication).
	•	AI Behavior Configurations: The admin interface may allow tweaking certain AI settings or prompts. For example, setting the assistant’s persona or updating the default system prompt to include a company greeting. Also, maybe configuring whether the assistant should automatically verify identity at call start (to ensure it’s the right person calling – though if we rely on caller ID matching the elder’s number, that might suffice).
	•	Security: All communication will use HTTPS (for web) and WSS (secure WebSockets). Sensitive data such as contact info or medical info will be stored encrypted in the database or at least with proper access controls. Authentication for caregiver/admin web access will use secure session cookies or tokens. We will also protect the Twilio and Stripe webhooks with known secrets or verification tokens to ensure those requests are genuinely from Twilio/Stripe. For the OpenAI and Uber integrations, API keys and secrets are stored securely on the server (never exposed to the client). Additionally, we comply with privacy needs – call audio processing via OpenAI is covered under their API data usage policies (we might opt-out of data logging at OpenAI side if possible, since these are personal conversations).

With these integrations and features, the voice assistant provides a comprehensive service: from physical assistance (rides) to health adherence (meds) to social connection (calls/info), all through a simple voice interface tailored to seniors.

Web Application (Caregiver & Admin Interface)

While the elderly user interacts via phone, the caregiver and admin interfaces are delivered as a web application. We emphasize ease of use and clarity, given that caregivers might not be tech experts either. The web app is built with standard HTML/CSS (possibly using a lightweight CSS framework for clean styling) and enhances interactivity through htmx and Alpine.js – avoiding the need for a complex single-page app.

Key UI Components and Pages for Caregivers:
	•	Dashboard: Upon login, the caregiver sees a dashboard for their elderly dependent. This might show a summary (e.g., “Next medication at 8 PM”, “No new alerts”, “Last call with AI was 2 days ago for 5 minutes”, etc.). It provides quick status info.
	•	Elder Profile Management: A section to view/edit the elder’s information – name, phone number (used for caller ID recognition), perhaps some preferences (like time zone, language or voice options for the AI).
	•	Contacts Management: CRUD interface to add new contacts, edit or remove existing ones. Likely a simple list with an “Add Contact” form (name, number, relationship fields).
	•	Locations Management: Similar CRUD interface for saved locations (name and address). We might integrate a map or address autocomplete for convenience, but a simple form is acceptable.
	•	Medication Schedule: An interface to input medications and times. Possibly a table with each med and columns for times (or separate entries for each dose). We’ll include an “Add Medication” form (name, dosage, schedule). The UI should clearly show upcoming doses and allow editing times.
	•	Reminder Logs: A page or section showing past reminders and whether they were acknowledged. For instance, “Aug 1, 8:00 AM – Lipitor – Confirmed by user” or “Aug 1, 8:00 PM – Metformin – No answer, SMS sent.” This helps caregivers track adherence.
	•	Call Logs / Transcripts: A section where the caregiver can see the records of the elder’s calls with the assistant. At minimum, it will list date, time, duration, and summary of what happened (e.g., “Ride booked to Grocery Store” or “Called John Doe”). If transcripts are available, the caregiver could expand a call to read the conversation text. This is useful for oversight and also for the caregiver’s peace of mind (they can see if the elder asked something concerning). We will include disclaimers about privacy here.
	•	Billing & Subscription: The caregiver should be able to view their current subscription plan, billing history, and update payment method. We will integrate Stripe’s Customer Portal for secure management of payment details. A “Billing” page may contain a link/button that redirects to Stripe’s hosted portal or a summary of the plan and a “Update Payment Info” action. If the subscription is near renewal or any issue (failed payment), this page can show that status.

All these are implemented in a responsive way (so caregivers can access on mobile or desktop). The use of htmx means actions like adding/editing a contact or location can happen inline without full page refresh – for example, clicking “Edit” on a contact could trigger an htmx request that returns an HTML form snippet to replace that contact’s display, making it editable. On save, the updated info is shown. This provides a smooth experience without building a heavy frontend. Alpine.js can handle small interactive touches, such as showing/hiding elements (modal dialogs for confirm delete, for instance) and simple form behaviors.

Admin Interface: The admin interface will have some overlapping elements (since an admin might impersonate a caregiver view for support) but also additional capabilities:
	•	Admin Dashboard: showing overall system metrics (number of active elders, number of calls today, etc., maybe graphs).
	•	User Management: a page to list all caregivers and/or all elders. Admin can search and select an account to view details. Possibly allow admin to reset passwords or disable accounts if needed.
	•	Monitoring: access to logs – possibly a live log stream viewer for technical logs, and database of calls/rides across the system. The admin might have a way to filter by date or elder to assist in support queries.
	•	System Settings: some global config accessible to admins, e.g., toggling certain features, updating the AI’s base prompt, managing API keys (though those might just be env config not editable via UI for security).
	•	Billing Management: the admin might not need to handle Stripe directly (that’s mostly automated through Stripe’s dashboard), but maybe they can comp an account or apply a discount by generating a Stripe coupon, etc. This could also be done outside the system if not necessary to implement in the app.

Both caregiver and admin portions will likely be the same web app but with role-based access control deciding what each user sees. We’ll implement authentication (likely with sessions & cookies since it’s server-rendered; or JWT if that fits better). The login system will allow caregivers (and admins) to log in with email/password (and perhaps we’ll add 2FA for security given sensitive data).

Tech specifics: We use Axum to serve these pages. Tera or Askama for HTML templates will produce the initial pages, and subsequent interactions via htmx will hit JSON or HTML endpoints that return fragments. For example, POST /elder/123/contacts could return a rendered <tr> HTML snippet for the new contact to be inserted into a table (htmx can target a div to swap content). This avoids writing a lot of custom JS. (We might include some basic JS for any complex widget not covered by htmx, but likely minimal.)

The stack being Rust+Axum+htmx/Alpine is chosen because it is robust and developer-friendly for iterative development – as one Rust developer noted, this combination leads to a satisfying development experience with little JavaScript needed ￼. It ensures our frontend is lightweight and our backend retains full control over rendering and logic, aligning with clean architecture (UI is a separate layer that could be changed without affecting core logic).

API Design (Endpoints)

The backend exposes a set of HTTP endpoints to support all the above functionality. These include routes for the web frontend (which may serve HTML or JSON), as well as webhook endpoints for Twilio, Stripe, and Uber integrations. Below is a breakdown of major API endpoints grouped by feature/module:

Authentication & User Accounts
	•	POST /api/auth/register – Register a new caregiver account. Expects caregiver details (name, email, password, payment info possibly after registration). Returns success or errors.
	•	POST /api/auth/login – Log in an existing caregiver or admin. On success, establishes a session (cookie) or returns auth token.
	•	POST /api/auth/logout – Log out the current session.
	•	(We may also have email verification or password reset endpoints if needed, e.g., /api/auth/forgot-password.)

Caregiver & Elder Profiles
	•	GET /api/profile – Get the logged-in caregiver’s profile info (and possibly list of elders they manage, though in our model one caregiver -> one elder likely).
	•	POST /api/elder – Create a new elder profile. Called when a caregiver sets up service for an elder. Contains elder’s name, phone number (for caller ID), and perhaps preferences (language or voice). Returns the created elder ID.
	•	GET /api/elder/{elderId} – Get details of a specific elder (only accessible by its caregiver or admin). This includes basic info and maybe aggregated data (like summary of contacts count, etc., or next reminder).
	•	PUT/PATCH /api/elder/{elderId} – Update elder’s profile (e.g., if need to change phone number or name).
	•	GET /api/elder/{elderId}/dashboard – (If using a single-page for caregiver, this could return an overview JSON: upcoming reminders, last call, etc. However, since we have server HTML, this might not be a separate endpoint but rendered server-side. We mention it for completeness.)

Contacts Management
	•	GET /api/elder/{elderId}/contacts – Retrieve list of contacts for the elder. (If the UI is server-rendered, this might not be a pure API call, but for htmx dynamic loading, we could have it return a fragment of HTML.)
	•	POST /api/elder/{elderId}/contacts – Create a new contact. The request carries the contact info (name, phone, etc.). Returns the created contact (JSON or HTML snippet for UI).
	•	PUT/PATCH /api/elder/{elderId}/contacts/{contactId} – Edit an existing contact.
	•	DELETE /api/elder/{elderId}/contacts/{contactId} – Remove a contact from the list. (Likely with a confirmation in UI before calling this.)

Locations Management (Saved Addresses)
	•	GET /api/elder/{elderId}/locations – Get all saved locations for this elder.
	•	POST /api/elder/{elderId}/locations – Add a new saved location (name & address). The backend might geocode or standardize the address if needed, or just store as entered.
	•	PUT/PATCH /api/elder/{elderId}/locations/{locId} – Update address or name of a location.
	•	DELETE /api/elder/{elderId}/locations/{locId} – Delete a saved location.

Medication Schedule
	•	GET /api/elder/{elderId}/medications – Get the list of medications and schedules for the elder. Possibly organized by med or by time.
	•	POST /api/elder/{elderId}/medications – Add a new medication entry. This might include multiple times per day, so either one entry with multiple times or separate entries for each time. We could design it as one record per medication with a schedule pattern, or separate dose records; for simplicity, one med with a times array. Request will contain med name, dosage, and timing info.
	•	PUT /api/elder/{elderId}/medications/{medId} – Update an existing medication schedule (change times or dosage).
	•	DELETE /api/elder/{elderId}/medications/{medId} – Remove a medication from schedule.
	•	GET /api/elder/{elderId}/medications/logs – Retrieve log of reminders (timestamp, and whether confirmed). Alternatively, this could be part of a general logs endpoint.

Logs and History
	•	GET /api/elder/{elderId}/call-logs – Retrieve call records for that elder. Could include transcripts or summaries of each call session with the AI.
	•	GET /api/elder/{elderId}/ride-logs – Retrieve past ride requests made (with statuses: completed, canceled, etc., and details like destination and time). This might be combined in call-logs if each ride corresponds to a call event, but we could separate for clarity.
	•	GET /api/elder/{elderId}/reminder-logs – Retrieve history of medication reminders sent and their outcomes.
(These log endpoints are mostly for the caregiver; an admin would have similar ones without the elder filter, or filtered by all.)
	•	GET /api/admin/logs – (Admin only) perhaps a way to query system-wide logs, maybe with query params for type (calls, rides, etc.) and date range.

Twilio Webhook Endpoints
	•	POST /api/twilio/voice – Incoming voice call webhook from Twilio. Twilio will call this when someone dials our number. Our handler will verify the request (using Twilio’s signature or auth token) and then respond with TwiML. The TwiML will be a <Response><Connect><Stream ... /></Response> that instructs Twilio to initiate the media stream to our WS server ￼ ￼. After Twilio receives this, the call audio is handed over to the WS, and this HTTP request is done. (Note: the actual audio doesn’t come through this endpoint, it goes through the WS; this endpoint just bootstraps the connection.)
	•	WS /api/twilio/media-stream – This is not a traditional HTTP endpoint but a WebSocket route (perhaps something like wss://ourdomain/voice-stream/{callSid} or similar). Twilio connects here for the bidirectional audio stream. Our server will handle messages on this socket. (Axum supports WebSocket upgrades easily.) We’ll map this route to logic that ties into the OpenAI streaming as described earlier.
	•	POST /api/twilio/status – Twilio can optionally send call status callbacks (when a call is completed, or if it fails). We can configure that in TwiML or via Twilio console. This endpoint would log call end times, durations, etc. It might also be useful to trigger post-call processes (like summarization or sending a follow-up SMS of information). For now, its main use is logging.
	•	POST /api/twilio/sms – (If we allow SMS interaction or notifications) Twilio will call this when the number receives an SMS. We might not use inbound SMS for the elder (since service is call-focused), but if we ever allow the elder to text queries, this could handle it by responding via the model (out of scope for now). More importantly, we might use Twilio to send outbound SMS (no webhook, just REST API calls) for things like sending ride details or missed-call reminders. Those don’t need an endpoint except perhaps a status callback if SMS delivery status is needed.
	•	POST /api/twilio/outbound-reminder – This endpoint might be used internally by our scheduler: e.g., when it’s time for a reminder, our code could hit a Twilio API, or we can have Twilio hit us. Actually, a better approach: our system can directly call Twilio’s REST API to initiate a call, so no webhook needed. Alternatively, Twilio has a service where at a scheduled time they call our webhook for instructions. But implementing our own scheduler that directly calls Twilio’s outgoing call API (creating a call to the elder’s number and giving TwiML instructions to read a reminder) is straightforward and doesn’t need an extra public endpoint. So we likely won’t have a separate endpoint for starting outbound calls; the logic is internal.

Uber API & Webhooks
	•	POST /api/uber/request – (Internal use) An endpoint or service call that our function request_ride will trigger. This isn’t exposed to external users, but rather a backend integration function. It will use stored credentials to call Uber’s external API. So not a public HTTP endpoint in our server, just part of service layer.
	•	POST /api/uber/webhook – Uber’s API will send events here (we register this URL with Uber when setting up the API access). We’ll verify the authenticity (Uber likely includes a signature or we use token). The event body will contain ride status updates. On receiving one, we find which elder/ride it pertains to (we might store an internal mapping of Uber’s ride ID to elder and maybe the call session). We then update our records. If needed, we could trigger further actions; e.g., if ride is arriving, potentially send the elder an update (though usually the driver will directly contact the rider if needed). This endpoint just ensures we keep in sync with ride progress for logging and any notifications.

Stripe (Billing) Endpoints
	•	POST /api/billing/subscribe – Create a new subscription (or one-time payment) for a caregiver. Possibly called when they register or in the UI when upgrading plan. This might integrate with Stripe Checkout: our backend calls Stripe’s API to create a Checkout Session and returns the URL for the frontend to redirect the caregiver to. Alternatively, we embed Stripe Elements – but using hosted Checkout is simpler.
	•	GET /api/billing/subscription-status – Return current subscription info (plan name, next payment date, etc.) for the logged in caregiver. This helps show status in the UI.
	•	POST /api/stripe/webhook – Stripe will send events (payment succeeded, failed, subscription renewed, etc.) to this endpoint. We validate the signature using Stripe’s library. On relevant events, we update the user’s subscription status in our DB. For instance, when a payment succeeds, mark subscription active till next period; if failed or canceled, mark as inactive so we can restrict service if needed. This ensures our system knows whether the caregiver’s account is in good standing.
(We rely on Stripe for most billing logic, so our job is mainly to update our records and maybe email receipts or alerts on failures.)

Admin Endpoints
	•	GET /api/admin/caregivers – List all caregiver accounts (with basic info and status).
	•	GET /api/admin/caregivers/{id} – Get details of a specific caregiver (including their elder’s info, subscription status, etc.).
	•	PUT /api/admin/caregivers/{id} – Modify a caregiver (perhaps toggling active status or resetting password).
	•	GET /api/admin/elders – List all elder profiles in system. Possibly with filters (active, etc.).
	•	GET /api/admin/logs/calls, /logs/rides, etc. – endpoints to retrieve system-wide logs or stats.
	•	POST /api/admin/announcement – If we allow sending a message to all caregivers/elders (like system maintenance or new feature info), this could handle it (e.g., schedule an email or a message on their dashboard). This is speculative, not required by core features.

(Admin endpoints will be protected by admin auth and likely on separate URL prefix like /api/admin/... to clearly separate from normal user routes.)

Each endpoint will be secured appropriately: caregivers can only access their own elder’s resources (we will enforce that by checking the authenticated user’s ID against the elder’s caregiver_id). Admin endpoints require admin privileges. We will also implement rate limiting on certain endpoints to prevent abuse (especially the Twilio webhook and login attempts, etc.).

For the voice and AI functions: It’s worth noting that the OpenAI function calling doesn’t directly hit these HTTP endpoints; instead, our backend, when fulfilling a function call, will use internal service logic that in turn may use some of these endpoints or rather the underlying code. For example, the get_contact_info function on AI side will trigger our code to look up DB (similar to what GET /contacts would do), and request_ride calls the Uber integration. So those “functions” are implemented by calling service layer methods, not by making the AI do an HTTP request to these endpoints (that would be slower and unnecessary). The design ensures that all such logic is contained within the backend with proper reuse of the same business logic whether invoked via API call or AI function.

Conclusion

In this specification, we detailed a comprehensive design for a voice AI assistant service for the elderly, integrating a real-time AI model with telephony and various tools. We defined the user stories guiding the features, described a robust architecture using Rust (Axum) aligned with clean architecture principles, and covered key functionalities: from real-time AI-driven calls using Twilio and GPT-Realtime (for natural conversation without high latency or loss of context ￼), to external integrations like Uber for rides (leveraging Uber’s Ride Requests API for third-party ride hailing ￼), medication reminder scheduling, and contact management with the ability to place calls. The API endpoints were outlined to show how each part of the system will be accessed or triggered, ensuring clarity in implementation.

Overall, this design aims to be elder-friendly (accessible via a plain phone call with an empathetic AI voice), caregiver-friendly (easy web setup and oversight), and built on proven technology (Rust for reliability, Twilio for telephony, OpenAI for AI capabilities, Stripe for billing). By following this specification, developers can implement a system that meaningfully improves the independence and safety of elderly users, while maintaining high standards of code organization, security, and scalability.

Sources: The design draws upon current capabilities of OpenAI’s GPT-Realtime (notably its end-to-end speech handling and function calling for tool use ￼ ￼) and Twilio’s media streaming for real-time voice integration ￼, as well as standard web development practices in Rust (as exemplified by similar Rust+htmx stacks ￼). Uber’s API documentation confirms the feasibility of third-party ride requests ￼. Clean architecture principles guided the separation of concerns in the backend design ￼. This ensures the final product is maintainable and robust as a production system.

sure, here’s a cleaner markdown version of the spec.

⸻

voice ai assistant for elderly – system spec

1. overview

a phone-based voice assistant for elderly users:
	•	user calls a twilio number
	•	call audio is streamed to openai gpt-realtime
	•	assistant can:
	•	call ubers to pre-saved locations
	•	remind about meds (scheduled + on-demand)
	•	read out / call contacts

stack:
	•	backend: rust + axum, clean architecture
	•	db: postgres (tokio-postgres + deadpool)
	•	ai: openai gpt-realtime (realtime api, tools)
	•	telephony: twilio voice (incoming + outbound calls, media streams)
	•	web ui: axum server-rendered html + htmx + alpine.js + tailwindcss
	•	billing: stripe (subscriptions)

⸻

2. roles & user stories

2.1 elderly user (phone caller only)
	•	calls a single phone number and talks to “the assistant”
	•	ride booking
	•	“call me an uber to the doctor”
	•	“take me to the grocery store”
	•	meds
	•	“remind me to take my pills”
	•	“what do i have to take tonight?”
	•	contacts
	•	“what’s my son john’s number?”
	•	“call my daughter”

user stories:
	•	i want to request rides to familiar places without apps.
	•	i want to get medication reminders so i don’t forget doses.
	•	i want to hear or dial my contacts with my voice only.
	•	i want the conversation to feel natural and slow-paced, not techy.

2.2 caregiver (web user + payer)
	•	creates account, configures elder’s profile
	•	sets:
	•	elder phone number (caller id)
	•	contacts
	•	locations
	•	med schedule
	•	views:
	•	logs (calls, rides, reminders)
	•	subscription and billing

user stories:
	•	i want to enroll my parent so they can use the assistant via phone.
	•	i want to define safe locations for uber rides.
	•	i want to input their med schedule once and trust the reminders.
	•	i want to see high-level logs / summaries of calls.
	•	i want to manage my subscription and payment info.

2.3 admin (service operator)
	•	sees global metrics, all caregivers/elders
	•	monitors issues
	•	can toggle accounts, support users

user stories:
	•	i want to see system health (call counts, error rates).
	•	i want to inspect particular accounts when there are support tickets.
	•	i want to manage subscription status (comp, disable).

⸻

3. architecture

3.1 high-level components
	•	voice interaction layer
	•	twilio incoming calls → http webhook (twiml)
	•	twilio media stream → websocket with backend
	•	ai layer
	•	backend ↔ openai gpt-realtime via websocket
	•	function calling for tools (rides, meds, contacts)
	•	backend api layer (axum)
	•	http routes for:
	•	web ui (html)
	•	twilio webhooks
	•	stripe webhooks
	•	admin / caregiver apis
	•	domain + use cases
	•	pure rust structs + services
	•	no framework / io logic inside
	•	infra
	•	postgres repositories (tokio-postgres + deadpool)
	•	twilio client
	•	openai client
	•	uber client
	•	stripe client
	•	web ui
	•	templates (tera/askama)
	•	htmx for partial updates
	•	alpine.js for light client behavior

3.2 clean architecture layering
	•	domain
	•	entities: caregiver, elder, contact, location, medication, reminder, ride_request, call_session, subscription
	•	use case / service layer
	•	book_ride(elder_id, location_id)
	•	schedule_medication_reminders(elder_id)
	•	get_next_medication(elder_id, now)
	•	get_contact_by_name(elder_id, name)
	•	initiate_call_to_contact(elder_id, contact_id)
	•	record_call_log(...), record_ride_status(...), etc.
	•	infra
	•	postgres repos
	•	twilio / uber / openai / stripe clients
	•	delivery (axum handlers)
	•	http endpoints
	•	websocket endpoints (twilio media stream, openai realtime)
	•	template rendering

⸻

4. voice interaction: twilio + gpt-realtime

4.1 incoming call flow (happy path)
	1.	elder dials twilio number.
	2.	twilio hits POST /api/twilio/voice with call sid, from, to, etc.
	3.	backend returns twiml:

<Response>
  <Connect>
    <Stream url="wss://our-domain/api/twilio/media-stream/{callSid}" />
  </Connect>
</Response>


	4.	twilio opens ws to /api/twilio/media-stream/{callSid} with audio frames.
	5.	backend opens ws to openai realtime:
	•	wss://api.openai.com/v1/realtime?model=gpt-realtime
	6.	backend relays:
	•	twilio → openai: audio in
	•	openai → twilio: audio out
	7.	gpt-realtime:
	•	reads system prompt (elderly helper persona)
	•	processes streaming audio
	•	generates streaming voice output
	•	calls tools as needed (function calling)
	8.	backend handles tool calls:
	•	executes function (ride booking, db lookup, etc.)
	•	returns function result to model
	9.	conversation continues until call end.

4.2 system prompt (sketch)

you are a kind, patient phone assistant for an elderly user. speak slowly and clearly. help them:
	•	book rides to pre-saved locations
	•	remember medications
	•	reach their contacts
use tools when appropriate. confirm important actions. never give medical advice beyond reading their schedule and reminders.

4.3 tools (openai function calling)

tool definitions exposed to the model (conceptually):
	•	get_saved_locations(elder_id)
	•	request_ride(elder_id, location_id)
	•	get_upcoming_medications(elder_id, now)
	•	get_medication_schedule(elder_id)
	•	get_contact_info(elder_id, name_or_role)
	•	initiate_contact_call(elder_id, contact_id)

backend implements these calls using domain services. model sees them as json functions.

4.4 error handling & fallback
	•	if openai ws fails:
	•	close twilio stream
	•	twiml redirect: play friendly tts apology, optionally offer to route to caregiver
	•	if tool call fails (uber/stripe/db):
	•	assistant explains failure in simple language
	•	offers alternative (“should i try again?”, “call your caregiver?”)
	•	safety fallback:
	•	if user says “emergency”, “help”, etc:
	•	short circuit to initiate_contact_call for configured emergency contact

⸻

5. tool integrations

5.1 uber ride booking

data:
	•	location:
	•	id
	•	elder_id
	•	name (home, doctor, grocery, etc.)
	•	address (+ optional metadata)

flow:
	1.	model decides to call request_ride.
	2.	backend:
	•	resolves elder_id from call (via caller id → elder lookup).
	•	finds location by name/id.
	•	chooses pickup address (e.g. “home” by default or ask user).
	•	calls uber ride request api using:
	•	pickup: elder address
	•	dropoff: location address
	•	rider: elder or caregiver-linked uber account.
	3.	uber returns ride id, eta, driver/car info.
	4.	backend logs ride_request.
	5.	backend returns function result to model.
	6.	assistant tells user: eta, driver car, etc.
	7.	optional: send sms to elder (twilio) with details.

webhook:
	•	uber → POST /api/uber/webhook with ride status updates:
	•	requested, driver_assigned, arriving, in_progress, completed, canceled, etc.
	•	backend:
	•	updates ride record
	•	(optional) notifies caregiver (email/sms) on certain statuses.

5.2 medication reminders

data:
	•	medication:
	•	id
	•	elder_id
	•	name
	•	dosage
	•	instructions (e.g. with food)
	•	medication_schedule:
	•	id
	•	medication_id
	•	time_of_day or cron / schedule pattern
	•	reminder_log:
	•	id
	•	schedule_id
	•	timestamp
	•	delivery_method (call/sms)
	•	status (delivered, answered, confirmed, no_answer)

scheduled reminders:
	•	background scheduler (cron / tokio task):
	•	every minute: find schedules due now for each elder (by timezone)
	•	for each:
	•	initiate twilio outbound call:
	•	POST twilio calls api → /api/twilio/reminder-twiml?reminder_id=X
	•	twilio hits our reminder twiml endpoint to get instructions:
	•	<Say>: “this is your medication reminder…”
	•	optionally gather dtmf to confirm.
	•	log outcome:
	•	answered + confirmed if dtmf pressed
	•	no_answer if not picked up
	•	optionally send sms fallback with text.

on-demand info:
	•	model calls get_upcoming_medications or get_medication_schedule.
	•	backend returns simple list (“8:00 am – lipitor 2 pills; 8:00 pm – metformin 1 tablet”).
	•	assistant reads it out.

5.3 contacts & calling

data:
	•	contact:
	•	id
	•	elder_id
	•	name
	•	relationship (daughter, son, doctor…)
	•	phone
	•	notes (optional)

info lookup:
	•	tool get_contact_info(elder_id, query):
	•	search by name / relationship (fuzzy).
	•	return match(es).
	•	assistant:
	•	reads phone number, optionally address or notes.

placing calls:
	•	tool initiate_contact_call(elder_id, contact_id):
	•	backend fetches number.
	•	using twilio rest api:
	•	update live call (call sid) to redirect to new twiml:

<Response>
  <Say>connecting you now.</Say>
  <Dial><Number>+1xxx...</Number></Dial>
</Response>


	•	close media stream / openai session.
	•	twilio bridges elder ↔ contact directly.

	•	log call_transfer event.

⸻

6. web app (caregiver & admin ui)

6.1 caregiver ui

pages:
	•	/dashboard
	•	summary: next med time, last call, recent rides
	•	/elder/profile
	•	elder name, phone, timezone, language, preferences
	•	/elder/contacts
	•	list contacts
	•	add / edit / delete
	•	/elder/locations
	•	list locations
	•	add / edit / delete
	•	/elder/medications
	•	meds + schedules
	•	create / edit / remove
	•	/elder/logs/calls
	•	call sessions: date, duration, summary, (optional transcript)
	•	/elder/logs/reminders
	•	reminder history and status
	•	/elder/logs/rides
	•	rides history
	•	/billing
	•	plan, status, link to stripe customer portal

ui tech:
	•	server rendered html templates
	•	htmx:
	•	in-place editing (contacts, locations, meds)
	•	partial updates, no full reload
	•	alpine.js:
	•	small interactions (modals, toggles, confirm dialogs)
	•	tailwindcss:
	•	utility-first css framework
	•	responsive design out of the box
	•	dark mode support

6.2 admin ui

pages:
	•	/admin/dashboard
	•	total elders, active subscriptions, call volume, error counts
	•	/admin/caregivers
	•	list + search caregivers
	•	view linked elder
	•	/admin/elders
	•	list elders, filter by status
	•	/admin/logs
	•	call logs, ride logs, error logs (filterable)
	•	/admin/settings
	•	global config knobs (maybe: base prompt, feature toggles)

role-based access control: caregiver vs admin.

⸻

7. api design

7.1 auth

POST   /api/auth/register        create caregiver account
POST   /api/auth/login           login caregiver/admin
POST   /api/auth/logout          logout

payloads typical: email, password, name; responses: success + session cookie / token.

7.2 caregiver / elder profiles

POST   /api/elder                        create elder for caregiver
GET    /api/elder/{elder_id}             get elder profile
PUT    /api/elder/{elder_id}             update elder profile
GET    /api/profile                      get current caregiver profile (+ elder summary)

7.3 contacts

GET    /api/elder/{elder_id}/contacts                    list contacts
POST   /api/elder/{elder_id}/contacts                    create contact
PUT    /api/elder/{elder_id}/contacts/{contact_id}       update contact
DELETE /api/elder/{elder_id}/contacts/{contact_id}       delete contact

7.4 locations

GET    /api/elder/{elder_id}/locations                   list locations
POST   /api/elder/{elder_id}/locations                   create location
PUT    /api/elder/{elder_id}/locations/{loc_id}          update location
DELETE /api/elder/{elder_id}/locations/{loc_id}          delete location

7.5 medications & reminders

GET    /api/elder/{elder_id}/medications                 list meds
POST   /api/elder/{elder_id}/medications                 create med + schedule
PUT    /api/elder/{elder_id}/medications/{med_id}        update med/sched
DELETE /api/elder/{elder_id}/medications/{med_id}        delete med

GET    /api/elder/{elder_id}/reminder-logs               list reminder logs

7.6 logs

GET    /api/elder/{elder_id}/call-logs                   list call logs
GET    /api/elder/{elder_id}/ride-logs                   list ride logs

7.7 twilio

POST   /api/twilio/voice          incoming call webhook → returns twiml <Connect><Stream>
WS     /api/twilio/media-stream   twilio media stream websocket (audio in/out)
POST   /api/twilio/status         call status callback
POST   /api/twilio/sms            (optional) inbound sms webhook

outbound calls (med reminders) use twilio rest api from backend; no extra public endpoint needed aside from a reminder-twiml endpoint if desired.

7.8 uber

POST   /api/uber/webhook          uber ride status webhooks

ride requests go from backend directly to uber http api (internal client, not public endpoint).

7.9 stripe / billing

POST   /api/billing/subscribe      create stripe checkout session / subscription
GET    /api/billing/status         get current subscription state for caregiver
POST   /api/stripe/webhook         stripe webhooks (invoice paid, failed, cancel, etc.)

7.10 admin

GET    /api/admin/caregivers                       list caregivers
GET    /api/admin/caregivers/{id}                  get caregiver details
PUT    /api/admin/caregivers/{id}                  update caregiver (status, etc.)

GET    /api/admin/elders                           list elders
GET    /api/admin/elders/{id}                      get elder details

GET    /api/admin/logs/calls                       system-wide call logs
GET    /api/admin/logs/rides                       system-wide ride logs
GET    /api/admin/logs/errors                      error logs


⸻

8. data model (sketch)

caregivers
  id
  name
  email
  password_hash
  stripe_customer_id
  created_at
  role (caregiver/admin)

elders
  id
  caregiver_id (fk → caregivers)
  name
  phone_number
  timezone
  language
  created_at
  status (active/inactive)

contacts
  id
  elder_id
  name
  relationship
  phone
  notes

locations
  id
  elder_id
  name
  address
  extra_instructions

medications
  id
  elder_id
  name
  dosage
  instructions

medication_schedules
  id
  medication_id
  time_of_day (local)
  days_pattern / cron_pattern

reminder_logs
  id
  schedule_id
  timestamp
  delivery_method
  status
  metadata (json)

ride_requests
  id
  elder_id
  location_id
  uber_ride_id
  status
  requested_at
  completed_at
  metadata (json)

call_sessions
  id
  elder_id
  twilio_call_sid
  started_at
  ended_at
  summary_text
  transcript (optional)
  metadata (json)

subscriptions
  id
  caregiver_id
  stripe_subscription_id
  status
  current_period_end
  plan_name


⸻

9. reliability, privacy, safety
	•	reliability
	•	timeouts and retries for external apis (uber, openai, stripe, twilio rest)
	•	graceful degradation:
	•	if ai fails, simple tts via twiml apologizing + fallback path
	•	structured logging for all external calls and tool calls
	•	privacy
	•	tls everywhere (https / wss)
	•	db access control; no cross-tenant data exposure
	•	only caregiver and admin see elder data; elder only via voice
	•	optional: encrypt sensitive fields at rest (contacts, meds)
	•	safety
	•	assistant never gives medical advice, only reads schedule / reminders
	•	emergency keyword detection (“help”, “emergency”) → call emergency contact
	•	rate limiting for api endpoints / webhooks to avoid abuse

⸻
