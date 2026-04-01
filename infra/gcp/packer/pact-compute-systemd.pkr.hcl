// pact-compute-systemd.pkr.hcl — GCP image for compute nodes (systemd variant).
//
// Standard Debian 12 with pact-agent as a systemd service.
// Used by variants V2 (pact only) and V4 (pact+lattice).

packer {
  required_plugins {
    googlecompute = {
      version = ">= 1.1.0"
      source  = "github.com/hashicorp/googlecompute"
    }
  }
}

variable "project_id" {
  type        = string
  description = "GCP project ID"
}

variable "zone" {
  type    = string
  default = "europe-west1-b"
}

variable "pact_version" {
  type        = string
  description = "Pact release version"
}

variable "with_lattice_agent" {
  type    = bool
  default = false
}

source "googlecompute" "compute_systemd" {
  project_id   = var.project_id
  zone         = var.zone
  machine_type = "e2-standard-2"

  source_image_family = "debian-12"
  source_image_project_id = ["debian-cloud"]

  image_name        = "pact-compute-systemd-${replace(var.pact_version, ".", "-")}"
  image_description = "Pact compute node (agent under systemd)"
  image_family      = "pact-compute-systemd"

  ssh_username = "packer"
  disk_size    = 10
}

build {
  sources = ["source.googlecompute.compute_systemd"]

  // Install base packages
  provisioner "shell" {
    inline = [
      "sudo apt-get update -qq",
      "sudo apt-get install -y -qq ca-certificates openssl iproute2",
    ]
  }

  // Create directory structure
  provisioner "shell" {
    inline = [
      "sudo mkdir -p /opt/pact/{bin,systemd} /etc/pact/certs /run/pact /var/lib/pact",
    ]
  }

  // Upload pact-agent release archive (from GitHub release)
  provisioner "file" {
    source      = "artifacts/pact-agent-x86_64-systemd.tar.gz"
    destination = "/tmp/pact-agent.tar.gz"
  }

  provisioner "shell" {
    inline = [
      "cd /tmp && tar xzf pact-agent.tar.gz",
      "sudo mv /tmp/pact-agent /opt/pact/bin/pact-agent",
      "sudo chmod +x /opt/pact/bin/pact-agent",
      "sudo ln -sf /opt/pact/bin/pact-agent /usr/local/bin/pact-agent",
    ]
  }

  // Upload systemd unit
  provisioner "file" {
    source      = "../../../infra/systemd/pact-agent.service"
    destination = "/tmp/pact-agent.service"
  }

  provisioner "shell" {
    inline = [
      "sudo mv /tmp/pact-agent.service /opt/pact/systemd/",
      "sudo cp /opt/pact/systemd/pact-agent.service /etc/systemd/system/",
    ]
  }

  // Create pact user
  provisioner "shell" {
    inline = [
      "sudo useradd --system --no-create-home --shell /usr/sbin/nologin pact || true",
      "sudo chown -R pact:pact /var/lib/pact",
    ]
  }

  // Upload default agent config (supervisor backend = systemd)
  provisioner "file" {
    source      = "config/agent-systemd.toml"
    destination = "/tmp/agent.toml"
  }

  provisioner "shell" {
    inline = [
      "sudo mv /tmp/agent.toml /etc/pact/agent.toml",
    ]
  }

  // Clean up
  provisioner "shell" {
    inline = [
      "sudo apt-get clean",
      "sudo rm -rf /tmp/* /var/tmp/*",
    ]
  }
}
