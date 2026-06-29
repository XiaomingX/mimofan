# Docker

mimo-tui publishes a multi-arch Linux image to GitHub Container Registry
for each release.

```bash
docker pull ghcr.io/hmbown/mimo-tui:latest
```

## Quick start

Run the published image with a Docker-managed data volume:

```bash
docker volume create mimo-tui-home

docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v mimo-tui-home:/home/mimo-tui/.mimo-tui \
  -v "$PWD:/workspace" \
  -w /workspace \
  ghcr.io/hmbown/mimo-tui:latest
```

Use a pinned release tag for reproducible installs:

```bash
docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v mimo-tui-home:/home/mimo-tui/.mimo-tui \
  -v "$PWD:/workspace" \
  -w /workspace \
  ghcr.io/hmbown/mimo-tui:vX.Y.Z
```

Replace `vX.Y.Z` with a tag from
[GitHub Releases](https://github.com/XiaomingX/mimo-tui/releases).

## Default image contract

`ghcr.io/hmbown/mimo-tui:latest` and the semver tags are conservative runtime
images:

- the container runs as the non-root `mimo-tui` user with UID/GID `1000:1000`
- the image does not grant passwordless `sudo`
- the image is meant to run mimo-tui against mounted workspaces, not to mutate
  the base operating system at runtime
- user state belongs in a volume mounted at `/home/mimo-tui/.mimo-tui`

That default is intentional. Keep using it for the smallest trust boundary. If a
project needs `apt-get`, compiler toolchains, Node/Python package managers,
custom CA certificates, or other host-like setup inside Docker, build an
explicit toolbox image instead of changing the default image contract.

## Opt-in toolbox/custom image

The repository includes an example
[`docs/examples/Dockerfile.toolbox`](examples/Dockerfile.toolbox) that extends
the official image with passwordless `sudo` and common development packages.
Build it with a pinned mimo-tui tag when you want repeatable project
environments:

```bash
docker build -f docs/examples/Dockerfile.toolbox \
  --build-arg CODEWHALE_IMAGE=ghcr.io/hmbown/mimo-tui:vX.Y.Z \
  --build-arg TOOLBOX_PACKAGES="git openssh-client curl build-essential pkg-config python3 python3-pip nodejs npm" \
  -t mimo-tui-toolbox:my-project .
```

Use `latest` only for throwaway testing. For shared projects, keep the
`CODEWHALE_IMAGE` value pinned and review package additions like any other
development-environment change.

Run the toolbox image with the same workspace and state mounts:

```bash
docker volume create mimo-tui-my-project-home

docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v mimo-tui-my-project-home:/home/mimo-tui/.mimo-tui \
  -v "$PWD:/workspace" \
  -w /workspace \
  mimo-tui-toolbox:my-project
```

Inside this opt-in image, mimo-tui can use commands such as
`sudo apt-get update` and `sudo apt-get install -y <package>`. For repeatable
containers, prefer baking those packages into the toolbox Dockerfile instead of
letting a long-lived container drift.

Do not bake API keys, SSH private keys, or other secrets into custom images.
Pass API keys at runtime and mount any SSH material deliberately, preferably
read-only and only for projects that need it.

### Compose toolbox template

If you prefer a repeatable `docker compose` entry point, use
[`docs/examples/compose.toolbox.yml`](examples/compose.toolbox.yml). It builds
the toolbox image from [`docs/examples/Dockerfile.toolbox`](examples/Dockerfile.toolbox)
and keeps the project state volume explicit:

```bash
CODEWHALE_IMAGE=ghcr.io/hmbown/mimo-tui:vX.Y.Z \
CODEWHALE_TOOLBOX_IMAGE=mimo-tui-toolbox:my-project \
CODEWHALE_HOME_VOLUME=mimo-tui-my-project-home \
CODEWHALE_WORKSPACE="$PWD" \
docker compose -f docs/examples/compose.toolbox.yml run --rm mimo-tui
```

Use a different `CODEWHALE_TOOLBOX_IMAGE` and `CODEWHALE_HOME_VOLUME` for each
project that needs an independent toolchain or independent `.mimo-tui` state.
The Compose file also shows opt-in, read-only mounts for SSH material and local
CA certificates; keep those commented out unless the project needs them.

## Multiple independent projects

Use one named state volume per project so sessions, config, skills, memory, and
the offline queue do not bleed across workspaces:

```bash
project="$(basename "$PWD")"
image="mimo-tui-toolbox:${project}"
docker volume create "mimo-tui-${project}-home"

docker run --rm -it \
  --name "mimo-tui-${project}" \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v "mimo-tui-${project}-home:/home/mimo-tui/.mimo-tui" \
  -v "$PWD:/workspace" \
  -w /workspace \
  "$image"
```

For projects with different toolchains, build different toolbox tags, for
example `mimo-tui-toolbox:frontend` and `mimo-tui-toolbox:backend`. The
separate launcher idea discussed in issue #2217 can build on this contract, but
it is intentionally outside the core Docker image.

## Project bootstrap scripts

mimo-tui does not automatically execute `.mimo-tui/setup.sh` or legacy
`.deepseek/setup.sh`. If you keep one of those files as a local project recipe,
run it explicitly. For shared team setup, prefer a committed project script or
the toolbox Dockerfile so the environment can be reviewed and rebuilt.

For example, to run a committed bootstrap script before starting mimo-tui:

```bash
docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v mimo-tui-my-project-home:/home/mimo-tui/.mimo-tui \
  -v "$PWD:/workspace" \
  -w /workspace \
  --entrypoint bash \
  mimo-tui-toolbox:my-project \
  -lc './scripts/bootstrap-dev.sh && exec mimo-tui'
```

Use the toolbox image for bootstrap scripts that need `sudo`. The default image
will not elevate privileges.

## Custom CA certificates and proxies

For corporate proxies, dev-sidecar, or self-signed internal services, prefer
baking trusted CA certificates into a custom toolbox image:

```dockerfile
USER root
COPY docker/certs/*.crt /usr/local/share/ca-certificates/
RUN update-ca-certificates
USER mimo-tui
```

All files copied into `/usr/local/share/ca-certificates/` must use the `.crt`
extension. Keep private CA material out of public images.

For a local-only run, mount certificates read-only and update the trust store at
container start:

```bash
docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v mimo-tui-my-project-home:/home/mimo-tui/.mimo-tui \
  -v "$PWD:/workspace" \
  -v "$PWD/docker/certs:/usr/local/share/ca-certificates/local:ro" \
  -w /workspace \
  --entrypoint bash \
  mimo-tui-toolbox:my-project \
  -lc 'sudo update-ca-certificates && exec mimo-tui'
```

This CA workflow requires the opt-in toolbox image because the default image
does not include passwordless `sudo`.

## Local build

Build the image locally from a checkout:

```bash
docker build -t mimo-tui .
```

Then run it with the same Docker-managed data volume:

```bash
docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v mimo-tui-home:/home/mimo-tui/.mimo-tui \
  -v "$PWD:/workspace" \
  -w /workspace \
  mimo-tui
```

Docker Hub publishing is not configured; GHCR is the supported prebuilt image
registry.

## Environment variables

| Variable              | Required | Description                                      |
|-----------------------|----------|--------------------------------------------------|
| `DEEPSEEK_API_KEY`    | yes      | DeepSeek API key                                 |
| `DEEPSEEK_BASE_URL`   | no       | Custom API base URL (e.g. `https://api.deepseek.com`) |
| `DEEPSEEK_NO_COLOR`   | no       | Set to `1` to disable terminal colour output     |

## Volumes

Mount `/home/mimo-tui/.mimo-tui` to persist sessions, config, skills, memory,
and the offline queue across container restarts. The image also keeps
`/home/mimo-tui/.deepseek` available for legacy compatibility. A
Docker-managed named volume is the safest default because Docker creates it with
ownership the container can write:

```bash
-v mimo-tui-home:/home/mimo-tui/.mimo-tui
```

Without this mount the container starts fresh each time.

If you bind-mount an existing host directory instead, the image runs as the
non-root `mimo-tui` user with UID/GID `1000:1000`. The mounted directory must be
writable by that user, or startup can fail while creating runtime directories
under `.mimo-tui/tasks`. On Linux hosts, either use the named volume above or
prepare the bind mount explicitly:

```bash
mkdir -p ~/.mimo-tui
sudo chown -R 1000:1000 ~/.mimo-tui

docker run --rm -it \
  -e DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
  -v ~/.mimo-tui:/home/mimo-tui/.mimo-tui \
  ghcr.io/hmbown/mimo-tui:latest
```

That `chown` changes ownership of the host `~/.mimo-tui` directory. Skip it if
you do not want the container UID to own your local config, and use a named
volume instead.

## Non-interactive / pipeline usage

When stdin is not a TTY, `mimo-tui` drops to the dispatcher's one-shot mode
(`mimo-tui -c "…"`). Pipe a prompt on stdin:

```bash
echo "Explain the Cargo.toml in structured English." | \
  docker run --rm -i -e DEEPSEEK_API_KEY ghcr.io/hmbown/mimo-tui:latest
```

## Building locally

```bash
# Single platform (your host architecture)
docker build -t mimo-tui .

# Multi-platform (requires a builder with emulation)
docker buildx create --use
docker buildx build --platform linux/amd64,linux/arm64 -t mimo-tui .
```

## Devcontainer

The repository includes a [`.devcontainer/devcontainer.json`](../.devcontainer/devcontainer.json)
configuration for VS Code / GitHub Codespaces. It pre-installs the Rust toolchain,
rust-analyzer, and the `mimo-tui` binary. Open the repo in a devcontainer to get a
ready-to-use development environment.

## Release status

Docker image publishing is part of the release gate. The image is published to
GHCR for `linux/amd64` and `linux/arm64` with semver tags plus `latest`.
