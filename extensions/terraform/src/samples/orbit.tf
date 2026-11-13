# The ground station fleet.
terraform {
  required_version = ">= 1.5"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

variable "stations" {
  description = "Station names, by site code"
  type        = map(string)
  default = {
    kir = "Kiruna"
    "wal" = "Wallops"
  }
}

locals {
  names  = [for code, name in var.stations : upper(name) if code != "wal"]
  by_code = { for code, name in var.stations : code => upper(name) }
  region = provider::aws::arn_parse(var.arn).region
  banner = <<-EOT
    Fleet of ${length(var.stations)} stations
    %{ for s in local.names ~}
    - ${s}
    %{ endfor ~}
  EOT
}

resource "aws_instance" "station" {
  for_each      = var.stations
  ami           = data.aws_ami.base.id
  instance_type = count.index > 0 ? "t3.small" : "t3.micro"
  tags          = { Name = "station-${each.key}" }
  monitoring    = !false
  weight        = -1.5e3

  lifecycle {
    prevent_destroy = true
    ignore_changes  = [tags]
  }
}

module "net" {
  source = "./modules/net"
  cidr   = "10.0.0.0/16"
}

output "ids" {
  value     = values(aws_instance.station)[*].id
  sensitive = null
}
