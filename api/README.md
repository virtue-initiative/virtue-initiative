# BePure API - Cloudflare Worker

A screenshot accountability system API built on Cloudflare Workers using modern technologies: Hono framework, D1 database, and R2 object storage.

## Features

- 🔐 **JWT Authentication** - Secure user authentication with access and refresh tokens
- 📸 **Image Management** - Upload screenshots to R2 with metadata tracking
- 📊 **Accountability Logs** - Track screenshot events with filtering and pagination
- 🖥️ **Device Management** - Register and manage multiple devices with API keys
- 🤝 **Partner System** - Accountability partner relationships with permissions
- ⚙️ **User Settings** - Configurable preferences and retention policies
- 🔒 **Security** - Argon2 password hashing, httpOnly cookies, device API keys
- ✅ **Type Safety** - Full TypeScript implementation

## Technology Stack

- **Runtime**: [Cloudflare Workers](https://workers.cloudflare.com/)
- **Framework**: [Hono](https://hono.dev/) - Fast, lightweight web framework
- **Language**: TypeScript
- **Database**: [Cloudflare D1](https://developers.cloudflare.com/d1/) - Serverless SQL
- **Storage**: [Cloudflare R2](https://developers.cloudflare.com/r2/) - Object storage
- **Auth**: JWT with Argon2id password hashing
- **Testing**: Vitest with Cloudflare Workers pool

## Project Structure

```
./api/
├── src/
│   ├── index.ts                 # Main worker entry point
│   ├── routes/
│   │   ├── auth.ts             # Authentication routes
│   │   ├── images.ts           # Image management
│   │   ├── logs.ts             # Accountability logs
│   │   ├── devices.ts          # Device management
│   │   ├── partners.ts         # Partner relationships
│   │   └── settings.ts         # User preferences
│   ├── middleware/
│   │   ├── auth.ts             # JWT authentication
│   │   └── device-auth.ts      # Device API key auth
│   ├── lib/
│   │   ├── jwt.ts              # JWT utilities
│   │   ├── password.ts         # Argon2 hashing
│   │   └── r2.ts               # R2 utilities
│   ├── types/
│   │   └── bindings.ts         # TypeScript bindings
│   └── schema/
│       └── migrations/         # D1 migrations
├── test/                        # Test files
├── wrangler.toml               # Cloudflare configuration
├── package.json
├── tsconfig.json
└── vitest.config.ts
```

## Prerequisites

- Node.js 18+ (recommend using [Volta](https://volta.sh/) or [nvm](https://github.com/nvm-sh/nvm))
- [Cloudflare account](https://dash.cloudflare.com/sign-up/workers-and-pages)
- [Wrangler CLI](https://developers.cloudflare.com/workers/wrangler/install-and-update/) (installed via npm)

## Setup Instructions

### 1. Install Dependencies

```bash
cd api
npm install
```

### 2. Create Cloudflare Resources

#### Create D1 Database

```bash
npx wrangler d1 create bepure-db
```

This will output a database ID. Update `wrangler.toml`:

```toml
[[d1_databases]]
binding = "DB"
database_name = "bepure-db"
database_id = "your-database-id-here"
```

#### Create R2 Bucket

```bash
npx wrangler r2 bucket create bepure-images
```

### 3. Run Database Migrations

```bash
npm run db:migrate
```

For local development:

```bash
npm run db:migrate:local
```

### 4. Set Environment Variables

Create `.dev.vars` file for local development:

```bash
JWT_SECRET=your-super-secret-key-change-this-in-production
JWT_ACCESS_EXPIRY=15m
JWT_REFRESH_EXPIRY=7d
```

For production, set secrets via Wrangler:

```bash
npx wrangler secret put JWT_SECRET
# Enter your secret when prompted
```

## Development

Start the local development server:

```bash
npm run dev
```

The API will be available at `http://localhost:8787`

## Testing

Run tests:

```bash
npm test
```

Watch mode:

```bash
npm run test:watch
```

## Deployment

Deploy to Cloudflare Workers:

```bash
npm run deploy
```

The API will be deployed to `https://bepure-api.<your-subdomain>.workers.dev`

## API Endpoints

### Authentication

- `POST /signup` - Create new user account
- `POST /login` - Authenticate user
- `POST /logout` - Clear refresh token
- `POST /token` - Refresh access token

### Images

- `POST /image` - Create image metadata and get upload URL
- `PUT /upload/:imageId` - Upload image binary

### Logs

- `POST /log` - Create accountability log entry
- `GET /log` - Query logs with filters

### Devices

- `POST /device` - Register new device
- `GET /device` - List user's devices
- `PATCH /device/:id` - Update device configuration

### Partners

- `POST /partner` - Send partner invite
- `POST /accept-partner` - Accept partner invite
- `GET /partner` - List partnerships
- `PATCH /partner/:id` - Update partner permissions
- `DELETE /partner/:id` - Revoke partner access

### Settings

- `POST /settings` - Create/update user settings
- `GET /settings` - Get user settings

## Authentication Methods

### User Authentication (JWT)

Most endpoints require JWT authentication. Include the access token in the Authorization header:

```bash
curl -H "Authorization: Bearer <access-token>" \
  https://your-api.workers.dev/device
```

### Device Authentication (API Key)

Devices use API keys for `/image` and `/log` endpoints:

```bash
curl -H "X-API-Key: <device-api-key>" \
  -X POST https://your-api.workers.dev/image \
  -d '{"device_id":"...","sha256":"...","content_type":"image/png",...}'
```

## Database Schema

### Users
- `id` (TEXT, PK)
- `email` (TEXT, UNIQUE)
- `password_hash` (TEXT)
- `name` (TEXT)
- `created_at` (TEXT)

### Devices
- `id` (TEXT, PK)
- `user_id` (TEXT, FK)
- `name` (TEXT)
- `platform` (TEXT)
- `api_key_hash` (TEXT)
- `last_seen_at` (TEXT)
- `last_upload_at` (TEXT)
- `enabled` (INTEGER)
- `created_at` (TEXT)

### Images
- `id` (TEXT, PK)
- `user_id` (TEXT, FK)
- `device_id` (TEXT, FK)
- `r2_key` (TEXT)
- `sha256` (TEXT)
- `content_type` (TEXT)
- `size_bytes` (INTEGER)
- `status` (TEXT)
- `taken_at` (TEXT)
- `created_at` (TEXT)

### Logs
- `id` (TEXT, PK)
- `user_id` (TEXT, FK)
- `device_id` (TEXT, FK)
- `image_id` (TEXT, FK, nullable)
- `type` (TEXT)
- `metadata` (TEXT/JSON)
- `created_at` (TEXT)

### Partners
- `id` (TEXT, PK)
- `user_id` (TEXT, FK)
- `partner_user_id` (TEXT, FK)
- `status` (TEXT)
- `permissions` (TEXT/JSON)
- `created_at` (TEXT)
- `updated_at` (TEXT)

### Settings
- `user_id` (TEXT, PK, FK)
- `name` (TEXT)
- `timezone` (TEXT)
- `retention_days` (INTEGER)
- `updated_at` (TEXT)

## Security Considerations

- ✅ Passwords hashed with Argon2id (recommended parameters)
- ✅ JWT access tokens expire in 15 minutes
- ✅ Refresh tokens stored in httpOnly, secure, sameSite cookies
- ✅ Device API keys hashed before storage
- ✅ Images stored in R2 with user-scoped paths
- ✅ SHA-256 verification for uploaded images
- ✅ CORS configured for production use

## R2 Object Layout

Images are stored in R2 with the following structure:

```
bucket/
  user/{userId}/
    images/{imageId}.{ext}
```

Example: `user/abc-123/images/img-456.webp`

## Configuration

Key configuration in `wrangler.toml`:

```toml
name = "bepure-api"
main = "src/index.ts"
compatibility_date = "2024-12-01"
compatibility_flags = ["nodejs_compat"]

[dev]
port = 8787

[[d1_databases]]
binding = "DB"
database_name = "bepure-db"
database_id = "your-database-id"

[[r2_buckets]]
binding = "BUCKET"
bucket_name = "bepure-images"

[vars]
JWT_ACCESS_EXPIRY = "15m"
JWT_REFRESH_EXPIRY = "7d"
```

## Troubleshooting

### Database migrations not applying

Ensure you're using the correct environment:

```bash
# Local
npm run db:migrate:local

# Remote
npm run db:migrate
```

### JWT_SECRET not set

Create `.dev.vars` for local development or use `wrangler secret put JWT_SECRET` for production.

### CORS errors

Update the CORS configuration in `src/index.ts` to match your client's origin.

## Contributing

1. Make changes in feature branches
2. Write tests for new functionality
3. Run `npm test` to verify tests pass
4. Deploy to a preview environment for testing

## License

ISC

## Support

For issues or questions, please refer to the [API specification](../API.md).
