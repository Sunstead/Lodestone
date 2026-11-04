# Deploying Lodestone

Lodestone runs on one Docker host. Each Minecraft server runs as a sibling
container from a stock `eclipse-temurin` image. Sign-in goes through
Authentik, like the other Sunstead services (Helion, Solstice). Every person
who signs in gets their own servers, worlds and configurations.

## 1. Authentik

Create the application in Authentik:

1. **Providers → Create → OAuth2/OpenID Provider**
   - Name: `Lodestone`
   - Client type: **Public** (Lodestone holds no secret; it uses PKCE)
   - Client ID: `lodestone` (or set `LODESTONE_OIDC_CLIENT_ID`)
   - Redirect URIs: **Strict**, `https://lodestone.example.com/auth/callback`
   - Signing key: any RSA or EC certificate. Lodestone refuses HS256.
   - Scopes: `openid`, `profile`, `email`. Authentik's `profile` mapping
     includes `groups`, which Lodestone reads.
2. **Applications → Create**: name `Lodestone`, slug `lodestone`, provider as above.
   - The issuer is `https://auth.example.com/application/o/lodestone/`.
3. **Groups**:
   - Members of the groups in `LODESTONE_OIDC_ADMIN_GROUPS` are admins. Admins
     change Lodestone's settings, see everyone, and can open anyone's assets.
   - Set `LODESTONE_OIDC_ALLOWED_GROUPS` to limit who can sign in at all.
     Authentik's application bindings work too.

Users are keyed by the token's `sub`, so renaming someone in Authentik keeps
their servers. The username shown is `preferred_username`, never the
editable display name.

## 2. The container

```bash
sudo mkdir -p /srv/lodestone && sudo chown "$USER": /srv/lodestone
cp compose.example.yaml compose.yaml   # set the domains and groups
docker compose up -d --build
```

What `compose.example.yaml` sets up, and why:

- **The Docker socket.** Lodestone creates, starts and stops the server containers.
- **`network_mode: host`.** Lodestone reaches each server's RCON on
  `127.0.0.1` (RCON is never published beyond the host). It also checks
  which game ports are free on the host.
- **The data dir at the same path inside and out.** Lodestone hands those
  paths to Docker as bind mounts. If the paths must differ, set
  `LODESTONE_HOST_DATA_DIR` to the host path.
- **Server files belong to the data dir's owner.** Containers run as that
  user, so world files on the host stay editable.

Put Lodestone behind your reverse proxy for TLS. It listens on
`127.0.0.1:7878` in the example. With Caddy:

```
lodestone.example.com {
    reverse_proxy 127.0.0.1:7878
}
```

`LODESTONE_PUBLIC_URL` must be the address browsers use:

- Sign-in redirects back to it.
- State-changing requests must come from its origin.
- Over HTTPS the session cookie is `__Host-lodestone_session` (Secure, HttpOnly).

## 3. First run

1. Sign in. If anything existed before sign-in was set up (for example, from
   `--dev-user`), the first admin to sign in becomes its owner.
2. Each person accepts the Minecraft EULA in **Settings** before their
   servers can start. Lodestone writes `eula.txt` only for them.
3. Admins set the **public address** players connect to.
4. Players connect to the game port shown on each server. Game ports are
   published on every interface; open them in your firewall.

## Configuration reference

| Variable | Default | |
|---|---|---|
| `LODESTONE_DATA` | `/srv/lodestone` (image) | Worlds, configurations, backups, the download cache and the database. |
| `LODESTONE_BIND` | `0.0.0.0:7878` (image) | HTTP address. |
| `LODESTONE_PUBLIC_URL` | — | Required for browser sign-in. |
| `LODESTONE_OIDC_ISSUER` | — | Required unless `LODESTONE_DEV_USER` is set. |
| `LODESTONE_OIDC_CLIENT_ID` | `lodestone` | |
| `LODESTONE_OIDC_SCOPES` | `openid profile email` | |
| `LODESTONE_OIDC_ADMIN_GROUPS` | — | Comma-separated. |
| `LODESTONE_OIDC_ALLOWED_GROUPS` | — | Comma-separated; empty lets in anyone Authentik does. |
| `LODESTONE_OIDC_DISCOVERY_URL` | issuer + `.well-known/...` | When Lodestone must reach Authentik at another address. |
| `LODESTONE_HOST_DATA_DIR` | — | The data dir's host path, when it differs. |
| `LODESTONE_CURSEFORGE_API_KEY` | — | Enables CurseForge as a source. Admins can also set it in Settings. Modrinth and Hangar need no key. |
| `LODESTONE_DEV_USER` | — | No sign-in; everyone is this admin. Development only. |

The API also accepts an Authentik access token for the same client as
`Authorization: Bearer`, for scripts.
