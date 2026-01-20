# OAuth Setup for ndl

ndl supports two OAuth methods for authenticating with the Threads API.

## Default: Hosted OAuth

The simplest option - no setup required. ndl uses a hosted OAuth server at `ndl.pgray.dev`:

```bash
ndl login
```

### How it works

1. `ndl login` sends a request to `https://ndl.pgray.dev/auth/start`
2. The server creates a session and returns an authorization URL
3. Your browser opens to Threads authorization
4. After you authorize, Threads redirects to the server's `/auth/callback`
5. The server exchanges the code for a token and stores it in the session
6. ndl polls `/auth/poll/{session_id}` until the token is ready
7. Token is saved to `~/.config/ndl/config.toml`

This keeps the client_secret secure on the server - your local ndl installation never sees it.

### Using a custom auth server

To use a different ndld server:

```bash
# Via environment variable
export NDL_OAUTH_ENDPOINT=https://your-ndld-server.com
ndl login

# Or in ~/.config/ndl/config.toml:
# auth_server = "https://your-ndld-server.com"
```

## Alternative: Local OAuth

If you have your own Threads API credentials and prefer to run OAuth locally:

```bash
export NDL_OAUTH_ENDPOINT=""
export NDL_CLIENT_ID=your_client_id
export NDL_CLIENT_SECRET=your_client_secret
ndl login
```

### Meta App Dashboard Setup

1. Go to [developers.facebook.com](https://developers.facebook.com) and create/open your app
2. Navigate to **Use cases** → **Settings** under "Access the Threads API"
   - Direct URL: `https://developers.facebook.com/apps/YOUR_APP_ID/use_cases/customize/?use_case_enum=THREADS_API`

3. Fill in **all three callback URLs** (all are required to save):

   | Field                    | Value                                |
   | ------------------------ | ------------------------------------ |
   | Redirect callback URL    | `https://localhost:1337/callback`    |
   | Deauthorize callback URL | `https://localhost:1337/deauthorize` |
   | Delete callback URL      | `https://localhost:1337/delete`      |

### HTTPS Requirement

Meta requires HTTPS for OAuth redirects, even for localhost.

ndl automatically generates a self-signed certificate at runtime using `rcgen` and `rustls` - no manual setup required. When you run `ndl login`, it:

1. Generates a self-signed cert for `localhost` and `127.0.0.1`
2. Starts an HTTPS server on port 1337
3. Opens your browser to authorize

Your browser will show a certificate warning (expected for self-signed certs). Accept it to continue.

#### Alternative: Use ngrok

If you prefer not to accept self-signed cert warnings:

```bash
ngrok http 1337
# Update your Meta app redirect URLs with the ngrok HTTPS URL
```

### Local OAuth Flow

1. `ndl login` starts a local HTTPS server on port 1337
2. Browser opens to Threads authorization:
   ```
   https://threads.net/oauth/authorize?client_id=APP_ID&redirect_uri=https://localhost:1337/callback&scope=threads_basic&response_type=code
   ```
3. User authorizes the app
4. Threads redirects to `https://localhost:1337/callback?code=AUTH_CODE`
5. ndl exchanges the code for an access token:

   ```bash
   POST https://graph.threads.net/oauth/access_token
   Content-Type: application/x-www-form-urlencoded

   client_id=APP_ID&client_secret=APP_SECRET&grant_type=authorization_code&redirect_uri=https://localhost:1337/callback&code=AUTH_CODE
   ```

6. Token saved to `~/.config/ndl/config.toml`

## Available Scopes

| Scope                     | Description                |
| ------------------------- | -------------------------- |
| `threads_basic`           | Read profile info          |
| `threads_content_publish` | Create and publish threads |
| `threads_manage_insights` | Read insights/analytics    |
| `threads_manage_replies`  | Read and manage replies    |
| `threads_read_replies`    | Read replies only          |

## References

- [Threads API docs](https://developers.facebook.com/docs/threads)
- [Access token guide](https://developers.facebook.com/docs/threads/get-started/get-access-tokens-and-permissions)
- [Manage website permissions](https://www.threads.com/settings/website_permissions)
