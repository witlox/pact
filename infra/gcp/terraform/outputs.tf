output "management_ips" {
  description = "Internal IPs of management nodes"
  value = {
    for i, inst in google_compute_instance.management :
    inst.name => inst.network_interface[0].network_ip
  }
}

output "compute_ips" {
  description = "Internal IPs of compute nodes"
  value = {
    for i, inst in google_compute_instance.compute :
    inst.name => inst.network_interface[0].network_ip
  }
}

output "admin_ip" {
  description = "Public IP of admin node"
  value       = google_compute_instance.admin.network_interface[0].access_config[0].nat_ip
}

output "admin_internal_ip" {
  description = "Internal IP of admin node"
  value       = google_compute_instance.admin.network_interface[0].network_ip
}

output "variant" {
  value = var.variant
}

output "compute_image" {
  value = local.compute_image_family
}

output "journal_endpoint" {
  description = "First journal node endpoint for CLI usage"
  value       = "${google_compute_instance.management[0].network_interface[0].network_ip}:9443"
}

output "compute_nodes" {
  description = "Comma-separated compute hostnames for validate.sh"
  value       = join(",", [for inst in google_compute_instance.compute : inst.name])
}
