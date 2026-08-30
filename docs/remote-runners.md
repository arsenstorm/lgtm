# Remote and ephemeral runners

## Why

`lgtm runner` connects out to `lgtm serve` over a WebSocket. It does not need
an inbound port, so it can run on a laptop, a spare box, a cloud VM, or in a
container next to your CI. This doc covers three things: securing that
connection with TLS, running runners that exit on their own, and having the
orchestrator start runners for you when the queue needs them.

`lgtm serve` generates a token on first run and stores it at
`~/.lgtm/token`, so every `lgtm` command on that machine picks it up
automatically. It also prints a ready-to-paste join line, e.g. `lgtm runner
ws://<ip>:4750 --token <token>`, for adding another machine.

## TLS with a self-signed certificate

`lgtm serve` can terminate TLS itself. rustls (used by every LGTM client)
refuses a certificate that is both the trust anchor and the server's own
certificate, so make a small CA and sign a server certificate with it:

```sh
# 1. A CA the runners will trust.
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout ca-key.pem -out ca.pem -subj "/CN=lgtm-ca" \
  -addext "basicConstraints=critical,CA:TRUE"

# 2. A server certificate for the host the runners connect to.
openssl req -newkey rsa:2048 -nodes -keyout key.pem -out server.csr -subj "/CN=host"
printf 'subjectAltName=DNS:host,IP:203.0.113.10\n' > san.cnf
openssl x509 -req -in server.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial \
  -days 825 -extfile san.cnf -out cert.pem
```

Replace `host` and the SAN entries with the real hostname or IP the runners
will use to reach the orchestrator. Keep `ca-key.pem` on the orchestrator
host only; `ca.pem` is what you copy to runners.

Start the orchestrator with the server certificate:

```sh
lgtm serve --tls-cert cert.pem --tls-key key.pem
```

A runner on another machine connects with `wss://` and trusts the CA:

```sh
lgtm runner wss://host:4750 --token <token> --ca ca.pem
```

The `lgtm` CLI does the same over `https://`:

```sh
LGTM_CA=ca.pem lgtm --orchestrator https://host:4750 runners
```

## Ephemeral runners

`lgtm runner <url> --ephemeral` marks the runner as one that is expected to
go away. It still connects and runs tasks normally, but:

- `--max-tasks N` makes it exit cleanly after finishing `N` tasks, instead
  of running forever.
- On exit it sends a `Goodbye` message before closing the connection. The
  orchestrator removes the runner immediately, rather than waiting for a
  connection timeout, so the slot is free right away and nothing lingers in
  `lgtm runners`.

Use `--ephemeral` for any runner whose process (or container, or VM) you
intend to throw away after it finishes: a `docker run --rm`, a spot
instance, a CI runner.

## Provisioning

`lgtm serve --provision <cmd>` lets the orchestrator start runners itself,
instead of you starting them by hand. Every 30 seconds it checks whether any
queued task has nowhere to run — no connected runner with a free slot and
the right executor — and if so, and the number of connected ephemeral
runners is under `--provision-max`, it runs `<cmd>` through `sh -c`.

To avoid launching a pile of runners for one queued task, only one
provision command is in flight at a time: once launched, the orchestrator
waits for the ephemeral runner count to go up (the new runner connected) or
for five minutes to pass, before it will provision again.

The command runs with three environment variables set:

- `LGTM_ORCHESTRATOR_URL` — the value of `--public-url`, i.e. the address a
  newly started runner should connect back to.
- `LGTM_TOKEN` — the orchestrator's token (the one stored at `~/.lgtm/token`
  and printed in the `lgtm runner …` join line).
- `LGTM_QUEUED` — how many tasks are currently queued, in case the command
  wants to size what it starts.

`--public-url` is required whenever `--provision` is set, since it's what
the orchestrator tells new runners to dial.

Examples. Start a container:

```sh
lgtm serve --provision \
  'docker run --rm -e LGTM_TOKEN -e ANTHROPIC_API_KEY lgtm-agent $LGTM_ORCHESTRATOR_URL' \
  --provision-max 3 --public-url wss://host:4750
```

Start one over SSH on a box that's already up:

```sh
lgtm serve --provision \
  "ssh box 'LGTM_TOKEN=$LGTM_TOKEN nohup lgtm runner $LGTM_ORCHESTRATOR_URL --ephemeral >/dev/null 2>&1 &'" \
  --provision-max 3 --public-url wss://host:4750
```

Or try it locally without any of that, just to see it fire:

```sh
lgtm runner ws://127.0.0.1:4750 --token <token> --ephemeral --name pod-$$ &
```

## Tailscale

If the orchestrator and its runners are all on the same tailnet, skip TLS.
Tailscale already encrypts the connection between them at the network
layer, so plain `ws://` is fine:

```sh
lgtm serve --bind 0.0.0.0:4750
lgtm runner ws://orchestrator.tailnet-name.ts.net:4750 --token <token>
```

This is the easiest setup for a personal fleet of runners and avoids
managing a certificate at all.

## Pushing with a GitHub App

By default a push carries the orchestrator's own `GITHUB_TOKEN`, which can do
everything that token can do in every repository. Configure a GitHub App and
the orchestrator instead mints a token per repository, good for an hour, that
can only push:

- `LGTM_GITHUB_APP_ID` — the app's numeric id.
- `LGTM_GITHUB_APP_KEY` — path to the app's private key PEM.

Both must be set, alongside `GITHUB_TOKEN` (the API client still reads issues
and opens pull requests with it). Signing the app's JWT shells out to
`openssl`, so it has to be on the orchestrator's PATH.

What the installation token can do:

| Action | Allowed |
| --- | --- |
| Read the repository | yes |
| Push the task branch | yes |
| Create a pull request | yes |
| Merge a pull request | no |
| Delete a branch or repository | no |
| Change repository configuration | no |

The token is fetched when a task completes and cached for 50 minutes, so
approving a task does not wait on GitHub. If anything about the app is wrong —
no installation on the repository, an unreadable key — the push falls back to
`GITHUB_TOKEN` and the failure is logged.
