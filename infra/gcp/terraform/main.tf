// main.tf — GCP infrastructure for pact test deployment.
//
// Creates: VPC, firewall rules, management VMs, compute VMs, admin VM.
// Variant-aware: selects PID 1 or systemd compute image based on var.variant.

terraform {
  required_providers {
    google = {
      source  = "hashicorp/google"
      version = "~> 5.0"
    }
  }
}

provider "google" {
  project = var.project_id
  region  = var.region
  zone    = var.zone
}

locals {
  // V1 and V3 use PID 1 image; V2 and V4 use systemd image
  compute_image_family = contains(["v1", "v3"], var.variant) ? "pact-compute-pid1" : "pact-compute-systemd"
  // V3 and V4 include lattice
  with_lattice = contains(["v3", "v4"], var.variant)

  name_prefix = "pact-test-${var.variant}"
}

// --- Networking ---

resource "google_compute_network" "pact" {
  name                    = "${local.name_prefix}-network"
  auto_create_subnetworks = false
}

resource "google_compute_subnetwork" "pact" {
  name          = "${local.name_prefix}-subnet"
  ip_cidr_range = "10.0.0.0/24"
  network       = google_compute_network.pact.id
}

// --- Firewall ---

resource "google_compute_firewall" "internal" {
  name    = "${local.name_prefix}-internal"
  network = google_compute_network.pact.id

  allow {
    protocol = "tcp"
    ports = [
      "9443",  // pact gRPC
      "9444",  // pact Raft
      "9445",  // pact shell
      "9091",  // pact metrics
      "5556",  // Dex OIDC
      "50051", // lattice gRPC
      "9000",  // lattice Raft
      "8080",  // lattice REST
    ]
  }

  allow {
    protocol = "icmp"
  }

  source_ranges = ["10.0.0.0/24"]
}

resource "google_compute_firewall" "admin" {
  name    = "${local.name_prefix}-admin"
  network = google_compute_network.pact.id

  allow {
    protocol = "tcp"
    ports = [
      "22",   // SSH to admin node only
      "3000", // Grafana
      "9090", // Prometheus
      "3100", // Loki
    ]
  }

  // Restrict to your IP (override in tfvars)
  source_ranges = ["0.0.0.0/0"]
  target_tags   = ["admin"]
}

// --- Management nodes (journal quorum) ---

resource "google_compute_instance" "management" {
  count        = var.management_count
  name         = "${local.name_prefix}-mgmt-${count.index + 1}"
  machine_type = var.management_machine_type

  tags = ["pact-management"]

  boot_disk {
    initialize_params {
      image = "projects/${var.project_id}/global/images/family/pact-management"
      size  = 20
    }
  }

  network_interface {
    subnetwork = google_compute_subnetwork.pact.id
    access_config {} // ephemeral public IP for setup
  }

  metadata = {
    pact-node-id      = count.index + 1
    pact-variant      = var.variant
    pact-with-lattice = local.with_lattice
  }

  // startup script runs install-management.sh
  metadata_startup_script = templatefile("${path.module}/templates/mgmt-startup.sh.tpl", {
    node_id      = count.index + 1
    peer_list    = join(",", [for i in range(var.management_count) : "${i + 1}=${local.name_prefix}-mgmt-${i + 1}:9443"])
    with_lattice = local.with_lattice
    admin_ip     = google_compute_instance.admin.network_interface[0].network_ip
  })
}

// --- Compute nodes ---

resource "google_compute_instance" "compute" {
  count        = var.compute_count
  name         = "${local.name_prefix}-compute-${count.index + 1}"
  machine_type = var.compute_machine_type

  tags = ["pact-compute"]

  boot_disk {
    initialize_params {
      image = "projects/${var.project_id}/global/images/family/${local.compute_image_family}"
      size  = 10
    }
  }

  network_interface {
    subnetwork = google_compute_subnetwork.pact.id
    // No public IP for compute nodes — access only via pact shell
  }

  metadata = {
    pact-node-id              = "compute-${count.index + 1}"
    pact-variant              = var.variant
    pact-journal-endpoints    = join(",", [for i in range(var.management_count) : "\"${local.name_prefix}-mgmt-${i + 1}:9443\""])
    pact-admin-ip             = google_compute_instance.admin.network_interface[0].network_ip
  }
}

// --- Admin / monitoring node ---

resource "google_compute_instance" "admin" {
  name         = "${local.name_prefix}-admin"
  machine_type = var.admin_machine_type

  tags = ["admin", "pact-admin"]

  boot_disk {
    initialize_params {
      image = "projects/${var.project_id}/global/images/family/pact-management"
      size  = 20
    }
  }

  network_interface {
    subnetwork = google_compute_subnetwork.pact.id
    access_config {} // public IP for admin access
  }

  metadata = {
    pact-variant = var.variant
  }

  metadata_startup_script = templatefile("${path.module}/templates/admin-startup.sh.tpl", {
    journal_hosts = join(",", [for i in range(var.management_count) : "${local.name_prefix}-mgmt-${i + 1}"])
  })
}
