qq on oauth: if i need to oauth a TUI, should it connect to server with (client id + secret), redirect to mint token to public endpoint, then pass token down to client (which it can then refresh at will?or am i misunderstanding? right now it's all redirected to localhost but i can't publish the client id+secret, right?going to ask claude next lol
11:56 AM

You're on the right track! Here's the deal:

You're correct: don't publish client secret in a TUI/CLI app. Anything distributed to users is considered a "public client" and can be decompiled/inspected.

Two common patterns for CLI/TUI OAuth:

    Device Authorization Flow (best for CLIs)
        User runs your CLI → CLI requests a device code from auth server
        CLI displays: "Go to https://example.com/device and enter code: ABCD-1234"
        User authenticates in browser
        CLI polls until token is granted
        No client secret needed, no localhost redirect
        This is what gh auth login, aws sso login, etc. use
    Authorization Code + PKCE (public client variant)
        CLI generates a random code_verifier + hashed code_challenge
        Redirects to localhost (like you're doing now)
        Exchanges auth code using code_verifier instead of client secret
        No secret needed—PKCE proves the same client that started the flow is completing it

Your proposed server-side approach (server holds secret, mints token, passes to client) works but adds complexity—now you're running infrastructure, and you need to securely pass that token to the CLI anyway.

TL;DR: Look into Device Authorization Flow if your OAuth provider supports it (most do now). It's purpose-built for CLIs and avoids the localhost redirect dance entirely.

What OAuth provider are you working with? Some have quirks.
