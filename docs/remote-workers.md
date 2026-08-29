# Remote and ephemeral workers

## Why

`lgtm-agent` connects out to `lgtm serve` over a WebSocket. It does not need
an inbound port, so it can run on a laptop, a spare box, a cloud VM, or in a
container next to your CI. This doc covers three things: securing that
connection with TLS, running workers that exit on their own, and having the
orchestrator start workers for you when the queue needs them.

## TLS with a self-signed certificate

`lgtm serve` can terminate TLS itself. rustls (used by every LGTM client)
refuses a certificate that is both the trust anchor and the server's own
certificate, so make a small CA and sign a server certificate with it:

```sh
# 1. A CA the workers will trust.
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout ca-key.pem -out ca.pem -subj "/CN=lgtm-ca" \
  -addext "basicConstraints=critical,CA:TRUE"

# 2. A server certificate for the host the workers connect to.
openssl req -newkey rsa:2048 -nodes -keyout key.pem -out server.csr -subj "/CN=host"
printf 'subjectAltName=DNS:host,IP:203.0.113.10\n' > san.cnf
openssl x509 -req -in server.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial \
  -days 825 -extfile san.cnf -out cert.pem
```

Replace `host` and the SAN entries with the real hostname or IP the workers
will use to reach the orchestrator. Keep `ca-key.pem` on the orchestrator
host only; `ca.pem` is what you copy to workers.

Start the orchestrator with the server certificate:

```sh
lgtm serve --tls-cert cert.pem --tls-key key.pem
```

A worker on another machine connects with `wss://` and trusts the CA:

```sh
lgtm-agent --orchestrator wss://host:4750 --ca ca.pem
```

The `lgtm` CLI does the same over `https://`:

```sh
LGTM_CA=ca.pem lgtm --orchestrator https://host:4750 workers
```

## Ephemeral workers

`lgtm-agent --ephemeral` marks the worker as one that is expected to go
away. It still connects and runs tasks normally, but:

- `--max-tasks N` makes it exit cleanly after finishing `N` tasks, instead
  of running forever.
- On exit it sends a `Goodbye` message before closing the connection. The
  orchestrator removes the worker immediately, rather than waiting for a
  connection timeout, so the slot is free right away and nothing lingers in
  `lgtm workers`.

Use `--ephemeral` for any worker whose process (or container, or VM) you
intend to throw away after it finishes: a `docker run --rm`, a spot
instance, a CI runner.

## Provisioning

`lgtm serve --provision <cmd>` lets the orchestrator start workers itself,
instead of you starting them by hand. Every 30 seconds it checks whether any
queued task has nowhere to run — no connected worker with a free slot and
the right executor — and if so, and the number of connected ephemeral
workers is under `--provision-max`, it runs `<cmd>` through `sh -c`.

To avoid launching a pile of workers for one queued task, only one
provision command is in flight at a time: once launched, the orchestrator
waits for the ephemeral worker count to go up (the new worker connected) or
for five minutes to pass, before it will provision again.

The command runs with three environment variables set:

- `LGTM_ORCHESTRATOR_URL` — the value of `--public-url`, i.e. the address a
  newly started worker should connect back to.
- `LGTM_TOKEN` — the orchestrator's token.
- `LGTM_QUEUED` — how many tasks are currently queued, in case the command
  wants to size what it starts.

`--public-url` is required whenever `--provision` is set, since it's what
the orchestrator tells new workers to dial.

Examples. Start a container:

```sh
lgtm serve --provision \
  'docker run --rm -e LGTM_ORCHESTRATOR=$LGTM_ORCHESTRATOR_URL -e LGTM_TOKEN -e ANTHROPIC_API_KEY lgtm-agent' \
  --provision-max 3 --public-url wss://host:4750
```

Start one over SSH on a box that's already up:

```sh
lgtm serve --provision \
  "ssh box 'LGTM_ORCHESTRATOR=$LGTM_ORCHESTRATOR_URL LGTM_TOKEN=$LGTM_TOKEN nohup lgtm-agent --ephemeral >/dev/null 2>&1 &'" \
  --provision-max 3 --public-url wss://host:4750
```

Or try it locally without any of that, just to see it fire:

```sh
lgtm-agent --ephemeral --name pod-$$ &
```

## Tailscale

If the orchestrator and its workers are all on the same tailnet, skip TLS.
Tailscale already encrypts the connection between them at the network
layer, so plain `ws://` is fine:

```sh
lgtm serve --bind 0.0.0.0:4750
lgtm-agent --orchestrator ws://orchestrator.tailnet-name.ts.net:4750
```

This is the easiest setup for a personal fleet of workers and avoids
managing a certificate at all.
