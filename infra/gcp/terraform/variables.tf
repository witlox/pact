variable "project_id" {
  type        = string
  description = "GCP project ID"
}

variable "region" {
  type    = string
  default = "europe-west1"
}

variable "zone" {
  type    = string
  default = "europe-west1-b"
}

variable "variant" {
  type        = string
  description = "Deployment variant: v1, v2, v3, v4"
  validation {
    condition     = contains(["v1", "v2", "v3", "v4"], var.variant)
    error_message = "Variant must be v1, v2, v3, or v4."
  }
}

variable "management_count" {
  type    = number
  default = 3
}

variable "compute_count" {
  type    = number
  default = 2
}

variable "management_machine_type" {
  type    = string
  default = "e2-standard-4"
}

variable "compute_machine_type" {
  type    = string
  default = "e2-standard-2"
}

variable "admin_machine_type" {
  type    = string
  default = "e2-standard-2"
}

variable "pact_version" {
  type        = string
  description = "Pact version for image selection"
}
