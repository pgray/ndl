# Privacy Policy

**ndl** and **ndld** do not track, collect, or store any personal information.

## What we don't do

- We don't collect analytics or usage data
- We don't track users
- We don't store access tokens on the server (tokens are returned to your local client immediately)
- We don't log IP addresses or user agents
- We don't use cookies
- We don't share any data with third parties

## What happens during authentication

When you use ndld to authenticate:

1. You're redirected to Threads (threads.net) to authorize the app
2. Threads sends an authorization code back to ndld
3. ndld exchanges that code for an access token
4. The token is immediately returned to your local ndl client
5. ndld discards the token - nothing is stored server-side

Your access token is only stored locally on your machine in `~/.config/ndl/config.toml`.

## Data deletion

Since we don't store any data, there's nothing to delete. You can revoke ndl's access to your Threads account at any time through your Threads settings.

## Contact

Questions? Open an issue at https://github.com/pgray/ndl

---

Last updated: 2025
