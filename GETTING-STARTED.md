# Getting Started

``` sh
docker build -f docker/Dockerfile . -t ghcr.io/queuetue/janet-world:main
PAT=$(cat ~/.private/mulamda_ci_pat | tr -d '[:space:]') && podman push --creds "queuetue:$PAT" ghcr.io/queuetue/janet-world:main
helm upgrade --install -n rally world ./helm && kubectl -n rally rollout restart deployment world-janet-world
```
