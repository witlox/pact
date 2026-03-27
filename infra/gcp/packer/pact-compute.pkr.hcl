// pact-compute.pkr.hcl — GCP image for compute nodes (PID 1 variant).
//
// Minimal Debian with pact-agent as init. No systemd, no SSH.
// GRUB configured with init=/usr/local/bin/pact-agent.
//
// Used by variants V1 (pact only) and V3 (pact+lattice).

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

source "googlecompute" "compute_pid1" {
  project_id   = var.project_id
  zone         = var.zone
  machine_type = "e2-standard-2"

  source_image_family = "debian-12"
  source_image_project_ids = ["debian-cloud"]

  image_name        = "pact-compute-pid1-${replace(var.pact_version, ".", "-")}"
  image_description = "Pact compute node (agent as PID 1, no systemd)"
  image_family      = "pact-compute-pid1"

  ssh_username = "packer"
  disk_size    = 10
}

build {
  sources = ["source.googlecompute.compute_pid1"]

  // Install minimal packages (no unnecessary services)
  provisioner "shell" {
    inline = [
      "sudo apt-get update -qq",
      "sudo apt-get install -y -qq ca-certificates openssl iproute2",
    ]
  }

  // Create directory structure
  provisioner "shell" {
    inline = [
      "sudo mkdir -p /opt/pact/bin /etc/pact/certs /run/pact /var/lib/pact",
    ]
  }

  // Upload pact-agent binary
  provisioner "file" {
    source      = "artifacts/pact-agent"
    destination = "/tmp/pact-agent"
  }

  provisioner "shell" {
    inline = [
      "sudo mv /tmp/pact-agent /usr/local/bin/pact-agent",
      "sudo chmod +x /usr/local/bin/pact-agent",
    ]
  }

  // Upload default agent config (will be overwritten by deploy script)
  provisioner "file" {
    source      = "config/agent-pid1.toml"
    destination = "/tmp/agent.toml"
  }

  provisioner "shell" {
    inline = [
      "sudo mv /tmp/agent.toml /etc/pact/agent.toml",
    ]
  }

  // Configure GRUB to use pact-agent as init
  // The agent handles mounting /proc, /sys, /dev (devtmpfs) via PlatformInit.
  provisioner "shell" {
    inline = [
      // Set kernel init parameter
      "sudo sed -i 's|GRUB_CMDLINE_LINUX_DEFAULT=\".*\"|GRUB_CMDLINE_LINUX_DEFAULT=\"quiet init=/usr/local/bin/pact-agent -- --config /etc/pact/agent.toml\"|' /etc/default/grub",
      "sudo update-grub",
    ]
  }

  // Remove SSH server — pact shell is the only access (ADR-007)
  // Keep GCP guest agent for serial console and metadata
  provisioner "shell" {
    inline = [
      "sudo apt-get remove -y openssh-server || true",
      "sudo apt-get autoremove -y",
    ]
  }

  // Optionally install lattice-agent binary (started as supervised service)
  provisioner "shell" {
    inline = var.with_lattice_agent ? [
      "echo 'Lattice agent will be installed from artifacts'"
    ] : [
      "echo 'Skipping lattice-agent (not requested)'"
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
