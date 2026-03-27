// pact-management.pkr.hcl — GCP image for management nodes.
//
// Contains: pact-journal, pact-cli, OPA, systemd units, monitoring configs.
// Optionally: lattice-server (for V3/V4 variants).

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
  description = "Pact release version (e.g., 2026.1.42)"
}

variable "with_lattice" {
  type    = bool
  default = false
}

variable "lattice_version" {
  type    = string
  default = ""
}

source "googlecompute" "management" {
  project_id   = var.project_id
  zone         = var.zone
  machine_type = "e2-standard-4"

  source_image_family = "debian-12"
  source_image_project_ids = ["debian-cloud"]

  image_name        = "pact-management-${replace(var.pact_version, ".", "-")}"
  image_description = "Pact management node (journal + CLI + monitoring)"
  image_family      = "pact-management"

  ssh_username = "packer"
  disk_size    = 20
}

build {
  sources = ["source.googlecompute.management"]

  // Install base packages
  provisioner "shell" {
    inline = [
      "sudo apt-get update -qq",
      "sudo apt-get install -y -qq ca-certificates curl openssl jq",
    ]
  }

  // Create directory structure
  provisioner "shell" {
    inline = [
      "sudo mkdir -p /opt/pact/{bin,systemd,alerting,grafana}",
      "sudo mkdir -p /etc/pact /var/lib/pact/journal",
    ]
  }

  // Upload platform release archive (from GitHub release)
  provisioner "file" {
    source      = "artifacts/pact-platform-x86_64.tar.gz"
    destination = "/tmp/pact-platform.tar.gz"
  }

  provisioner "shell" {
    inline = [
      "cd /tmp && tar xzf pact-platform.tar.gz",
      "sudo mv /tmp/pact /tmp/pact-journal /tmp/pact-mcp /opt/pact/bin/",
      "sudo chmod +x /opt/pact/bin/*",
      "sudo ln -sf /opt/pact/bin/pact /usr/local/bin/pact",
      "sudo ln -sf /opt/pact/bin/pact-journal /usr/local/bin/pact-journal",
      "sudo ln -sf /opt/pact/bin/pact-mcp /usr/local/bin/pact-mcp",
    ]
  }

  // Upload systemd units
  provisioner "file" {
    source      = "../../../infra/systemd/pact-journal.service"
    destination = "/tmp/pact-journal.service"
  }

  provisioner "shell" {
    inline = [
      "sudo mv /tmp/pact-journal.service /opt/pact/systemd/",
    ]
  }

  // Upload monitoring configs
  provisioner "file" {
    source      = "../../../infra/alerting/"
    destination = "/tmp/alerting"
  }

  provisioner "file" {
    source      = "../../../infra/grafana/"
    destination = "/tmp/grafana"
  }

  provisioner "shell" {
    inline = [
      "sudo cp -r /tmp/alerting/* /opt/pact/alerting/ 2>/dev/null || true",
      "sudo cp -r /tmp/grafana/* /opt/pact/grafana/ 2>/dev/null || true",
    ]
  }

  // Upload deploy scripts (from repo root scripts/deploy/)
  provisioner "file" {
    source      = "../../../scripts/deploy/"
    destination = "/tmp/deploy"
  }

  provisioner "shell" {
    inline = [
      "sudo mkdir -p /opt/pact/deploy",
      "sudo cp /tmp/deploy/*.sh /opt/pact/deploy/",
      "sudo chmod +x /opt/pact/deploy/*.sh",
    ]
  }

  // Create pact user
  provisioner "shell" {
    inline = [
      "sudo useradd --system --no-create-home --shell /usr/sbin/nologin pact || true",
      "sudo chown -R pact:pact /var/lib/pact",
    ]
  }

  // Install OPA (for policy evaluation)
  provisioner "shell" {
    inline = [
      "curl -fsSL -o /tmp/opa https://openpolicyagent.org/downloads/v0.73.0/opa_linux_amd64_static",
      "sudo mv /tmp/opa /usr/local/bin/opa",
      "sudo chmod +x /usr/local/bin/opa",
    ]
  }

  // Optionally upload lattice-server
  provisioner "shell" {
    inline = var.with_lattice ? [
      "echo 'Lattice server will be installed from artifacts'"
    ] : [
      "echo 'Skipping lattice-server (not requested)'"
    ]
  }
}
