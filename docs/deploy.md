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
docker compose up -d
```

The image is published to `ghcr.io/sunstead/lodestone` by
`.github/workflows/image.yml`:
- `:latest` and `:master` follow the default branch.
- Tags like `v0.2.0` publish `:0.2.0` and `:0.2`.
- Pin a version in production. `build: .` in the example builds it locally instead.

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

## 4. Servers on the internet

Servers run mods and plugins that anyone with an account can upload, which
is arbitrary code. On a LAN that's a small risk. Before game ports are
forwarded from the internet, set these:

| Variable | Example | What it does |
|---|---|---|
| `LODESTONE_GAME_PORTS` | `30000-30049` | Game ports are allocated only in this range, and a port outside it is refused. Forward exactly this range. RCON ports stay outside it (on `127.0.0.1`), and the UDP query port is refused, since it would be published on every interface. |
| `LODESTONE_PUBLIC_SERVERS` | `true` | `online-mode` stays on for every server. Offline mode lets anyone join under any name, an operator's included. The configuration page shows it locked, with the reason. |
| `LODESTONE_DOCKER_NETWORK` | `lodestone` | Servers and loader installers join this bridge network instead of Docker's default. Lodestone creates it when it's missing, with traffic between containers off (`enable_icc=false`). Its Linux bridge has the same name, so the name must be a valid interface name (up to 15 characters). Lodestone refuses to use an existing network that allows traffic between containers. |
| `LODESTONE_CONTAINER_DNS` | `1.1.1.1,9.9.9.9` | DNS servers for the containers. Set it when the host's resolver is on a private address, which the rule below blocks. |

Every server and installer container also runs with no Linux capabilities
(`cap_drop: ALL`), `no-new-privileges` and a limit of 4096 processes. Java
needs none of the capabilities: it runs as the data dir's owner and listens
above port 1024.

### Address names

A server can have an address name, e.g. `survival`: players then join at
`survival.<public address>` (`survival.play.example.com`) with no port, while
the server keeps an unremarkable port. It works through an SRV record, which
Minecraft: Java Edition looks up before anything else:

```
_minecraft._tcp.survival.play.example.com.  SRV  0 5 30001 play.example.com.
```

The public address in Settings must be a domain name with an A record (it's
the record's target). Names are one DNS label and unique across the host.

| Variable | |
|---|---|
| `LODESTONE_CLOUDFLARE_API_TOKEN` | A Cloudflare API token with **Zone → DNS → Edit** on the public address's zone. Lodestone then creates, moves and removes the records itself, and the server page says when one is live. |
| `LODESTONE_CLOUDFLARE_ZONE_ID` | That zone's id (its Overview page). |

Lodestone only lists, changes or deletes SRV records whose comment starts
with `lodestone:`, and it re-checks them every ten minutes. Without a token,
each server's page shows the record to add at your DNS provider.

### Keep servers off the LAN

Docker lets containers reach anything the host can route to, including the
LAN, the tailnet and other containers' published ports. Servers need the
internet (Mojang's session servers, mod and library downloads) and nothing
else. This nftables table drops the rest. It matches the network's bridge
by name, so it's independent of the subnet Docker picks, and it lives in its
own table, so Docker's rules are untouched:

```
#!/usr/sbin/nft -f
# Game servers (the "lodestone" bridge) reach the internet, not the LAN,
# the tailnet, other containers or this host.
table inet lodestone
delete table inet lodestone

table inet lodestone {
    set private4 {
        type ipv4_addr
        flags interval
        elements = { 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16,
                     100.64.0.0/10, 169.254.0.0/16 }
    }

    # Through the host: anything private is dropped. Replies to connections
    # made to a server (players, Lodestone's RCON) are let through first.
    chain forward {
        type filter hook forward priority filter - 10; policy accept;
        iifname "lodestone" ct state established,related accept
        iifname "lodestone" ip daddr @private4 drop
        iifname "lodestone" meta nfproto ipv6 drop
    }

    # To the host itself (its services on the host network, the bridge
    # gateway): nothing a server starts.
    chain input {
        type filter hook input priority filter - 10; policy accept;
        iifname "lodestone" ct state established,related accept
        iifname "lodestone" drop
    }
}
```

Load it with `sudo nft -f lodestone.nft` and at boot (for example from
`/etc/nftables.conf` with `include`, if that file doesn't `flush ruleset`
over Docker's rules). With it in place, check from a running server:

```bash
c=$(docker ps -q --filter label=lodestone.instance | head -1)
docker exec "$c" bash -c 'timeout 5 bash -c "</dev/tcp/1.1.1.1/443" && echo internet ok'
docker exec "$c" bash -c 'timeout 5 bash -c "</dev/tcp/192.168.1.1/80" || echo LAN blocked'
```

### Real client addresses

If the game ports are forwarded through a TCP proxy, servers see the proxy's
address, so a ban by IP bans the proxy. Paper can read the client's address
from the PROXY protocol (`proxies.proxy-protocol: true` in
`config/paper-global.yml`), but vanilla, Fabric and Forge can't, and a
server that isn't expecting it refuses every connection. Leave it off
unless every server is Paper, and ban by name.

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
| `LODESTONE_GAME_PORTS` | — | e.g. `30000-30049`. Game ports only in this range; no query port ([§4](#4-servers-on-the-internet)). |
| `LODESTONE_PUBLIC_SERVERS` | `false` | Keep `online-mode` on. |
| `LODESTONE_DOCKER_NETWORK` | — | An isolated bridge network for servers, created when missing. |
| `LODESTONE_CONTAINER_DNS` | — | Comma-separated DNS servers for server containers. |
| `LODESTONE_CLOUDFLARE_API_TOKEN`, `LODESTONE_CLOUDFLARE_ZONE_ID` | — | Keep servers' address names (SRV records) in Cloudflare ([Address names](#address-names)). |
| `LODESTONE_CURSEFORGE_API_KEY` | — | Enables CurseForge as a source. Admins can also set it in Settings. Modrinth and Hangar need no key. |
| `LODESTONE_DEV_USER` | — | No sign-in; everyone is this admin. Development only. |

The API also accepts an Authentik access token for the same client as
`Authorization: Bearer`, for scripts.
