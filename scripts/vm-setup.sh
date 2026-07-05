#!/usr/bin/env bash
# One-time setup ON the GPU VM: Docker + NVIDIA Container Toolkit.
# Idempotent — safe to re-run; persists across stop/start (baked into the disk),
# so the "come back later" restart needs no re-setup.
set -euo pipefail

if command -v docker >/dev/null 2>&1 && sudo docker info >/dev/null 2>&1; then
  echo "docker: already present"
else
  echo "docker: installing…"
  curl -fsSL https://get.docker.com | sudo sh
fi

if dpkg -l nvidia-container-toolkit >/dev/null 2>&1; then
  echo "nvidia-container-toolkit: already present"
else
  echo "nvidia-container-toolkit: installing…"
  curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey \
    | sudo gpg --dearmor -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg
  curl -s -L https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list \
    | sed 's#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://#g' \
    | sudo tee /etc/apt/sources.list.d/nvidia-container-toolkit.list >/dev/null
  sudo apt-get update -qq
  sudo apt-get install -y nvidia-container-toolkit
  sudo nvidia-ctk runtime configure --runtime=docker
  sudo systemctl restart docker
fi

# gcsfuse — lets `gpu.sh serve` load model weights from a GCS bucket (QM_GPU_MODEL=gs://…).
if command -v gcsfuse >/dev/null 2>&1; then
  echo "gcsfuse: already present"
else
  echo "gcsfuse: installing…"
  export GCSFUSE_REPO="gcsfuse-$(lsb_release -c -s)"
  echo "deb https://packages.cloud.google.com/apt $GCSFUSE_REPO main" \
    | sudo tee /etc/apt/sources.list.d/gcsfuse.list >/dev/null
  curl -fsSL https://packages.cloud.google.com/apt/doc/apt-key.gpg \
    | sudo gpg --dearmor -o /usr/share/keyrings/cloud.google.gpg
  sudo sed -i 's#deb https://#deb [signed-by=/usr/share/keyrings/cloud.google.gpg] https://#' /etc/apt/sources.list.d/gcsfuse.list
  sudo apt-get update -qq && sudo apt-get install -y gcsfuse
fi

echo "verifying docker can see the GPU…"
sudo docker run --rm --gpus all nvidia/cuda:12.4.0-base-ubuntu22.04 nvidia-smi -L
echo "vm-setup: ok"
